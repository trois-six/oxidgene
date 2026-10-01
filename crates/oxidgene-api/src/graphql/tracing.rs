use std::sync::{Arc, Mutex};

use async_graphql::extensions::{
    Extension, ExtensionContext, ExtensionFactory, NextExecute, NextParseQuery, NextResolve,
    ResolveInfo,
};
use async_graphql::parser::types::{ExecutableDocument, OperationType};
use async_graphql::{Response, ServerResult, Value, Variables};
use tracing::Instrument as _;

pub struct Tracing;

impl ExtensionFactory for Tracing {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(TracingExtension::default())
    }
}

/// Spans one GraphQL request: its execution, and each root field below it.
///
/// Created once per request, so it can carry what parsing learned about the
/// document over to the execution span.
#[derive(Default)]
struct TracingExtension {
    /// Each operation of the parsed document: its name, if any, and type.
    operations: Mutex<Vec<(Option<String>, OperationType)>>,
}

impl TracingExtension {
    /// The type of the operation `operation_name` selects: the named one, or
    /// the document's only operation when no name is given.
    fn operation_type(&self, operation_name: Option<&str>) -> Option<&'static str> {
        let operations = self.operations.lock().ok()?;
        let ty = match operation_name {
            Some(name) => operations
                .iter()
                .find(|(candidate, _)| candidate.as_deref() == Some(name))
                .map(|(_, ty)| *ty),
            None if operations.len() == 1 => operations.first().map(|(_, ty)| *ty),
            None => None,
        }?;
        Some(match ty {
            OperationType::Query => "query",
            OperationType::Mutation => "mutation",
            OperationType::Subscription => "subscription",
        })
    }
}

#[async_trait::async_trait]
impl Extension for TracingExtension {
    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        let document = next.run(ctx, query, variables).await?;
        if let Ok(mut operations) = self.operations.lock() {
            *operations = document
                .operations
                .iter()
                .map(|(name, operation)| (name.map(ToString::to_string), operation.node.ty))
                .collect();
        }
        Ok(document)
    }

    async fn execute(
        &self,
        ctx: &ExtensionContext<'_>,
        operation_name: Option<&str>,
        next: NextExecute<'_>,
    ) -> Response {
        // The operation name is chosen by the client and therefore unbounded;
        // only its type, one of three, is recorded.
        let span = tracing::info_span!(
            "graphql.execute",
            graphql.operation.type = self.operation_type(operation_name),
        );
        next.run(ctx, operation_name).instrument(span).await
    }

    async fn resolve(
        &self,
        ctx: &ExtensionContext<'_>,
        info: ResolveInfo<'_>,
        next: NextResolve<'_>,
    ) -> ServerResult<Option<Value>> {
        // Root fields only: a span per nested field is one per row and
        // column of every list, which buries the operation's real boundaries
        // (its root fields and their database calls) under volume.
        if info.is_for_introspection || info.path_node.parent.is_some() {
            return next.run(ctx, info).await;
        }
        let span = tracing::info_span!(
            "graphql.resolve",
            graphql.parent_type = info.parent_type,
            graphql.field.name = info.name,
            otel.status_code = tracing::field::Empty,
        );
        let result = next.run(ctx, info).instrument(span.clone()).await;
        if result.is_err() {
            span.record("otel.status_code", "ERROR");
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
    use tracing::Subscriber;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};
    use tracing_subscriber::registry::LookupSpan;

    use super::Tracing;

    /// A span's name, its parent's name, and its string-valued fields.
    #[derive(Debug, Clone)]
    struct CapturedSpan {
        name: String,
        parent: Option<String>,
        fields: Vec<(String, String)>,
    }

    #[derive(Clone, Default)]
    struct CapturedSpans(Arc<Mutex<Vec<CapturedSpan>>>);

    impl CapturedSpans {
        fn named(&self, name: &str) -> Vec<CapturedSpan> {
            self.0
                .lock()
                .expect("capture lock")
                .iter()
                .filter(|span| span.name == name)
                .cloned()
                .collect()
        }
    }

    struct Fields(Vec<(String, String)>);

    impl Visit for Fields {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push((field.name().to_string(), value.to_string()));
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    impl<S> Layer<S> for CapturedSpans
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            context: Context<'_, S>,
        ) {
            let parent = attributes
                .parent()
                .and_then(|parent| context.span(parent))
                .or_else(|| context.lookup_current())
                .map(|span| span.metadata().name().to_string());
            let mut fields = Fields(Vec::new());
            attributes.record(&mut fields);
            self.0.lock().expect("capture lock").push(CapturedSpan {
                name: attributes.metadata().name().to_string(),
                parent,
                fields: fields.0,
            });
        }
    }

    #[derive(SimpleObject)]
    struct Row {
        a: i32,
        b: i32,
    }

    struct Query;

    #[Object]
    impl Query {
        async fn value(&self) -> i32 {
            42
        }

        async fn rows(&self) -> Vec<Row> {
            (0..3).map(|n| Row { a: n, b: n }).collect()
        }
    }

    async fn capture(query: &str) -> CapturedSpans {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
            .extension(Tracing)
            .finish();
        let response = schema.execute(query).await;
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        captured
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolver_span_is_a_child_of_graphql_execution() {
        let captured = capture("{ value }").await;

        assert!(
            captured
                .named("graphql.resolve")
                .iter()
                .any(|span| span.parent.as_deref() == Some("graphql.execute"))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn only_root_fields_are_spanned() {
        let captured = capture("{ value rows { a b } }").await;

        let fields = captured
            .named("graphql.resolve")
            .into_iter()
            .filter_map(|span| {
                span.fields
                    .into_iter()
                    .find(|(name, _)| name == "graphql.field.name")
                    .map(|(_, value)| value)
            })
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 2, "one span per root field: {fields:?}");
        assert!(fields.contains(&"value".to_string()));
        assert!(fields.contains(&"rows".to_string()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execution_span_records_the_operation_type() {
        let captured = capture("query Named { value }").await;

        let execute = captured.named("graphql.execute");
        assert_eq!(execute.len(), 1);
        assert!(
            execute[0]
                .fields
                .contains(&("graphql.operation.type".to_string(), "query".to_string())),
            "{:?}",
            execute[0].fields
        );
    }
}
