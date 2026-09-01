use std::sync::Arc;

use async_graphql::extensions::{
    Extension, ExtensionContext, ExtensionFactory, NextExecute, NextResolve, ResolveInfo,
};
use async_graphql::{Response, ServerResult, Value};
use tracing::Instrument as _;

pub struct Tracing;

impl ExtensionFactory for Tracing {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(TracingExtension)
    }
}

struct TracingExtension;

#[async_trait::async_trait]
impl Extension for TracingExtension {
    async fn execute(
        &self,
        ctx: &ExtensionContext<'_>,
        operation_name: Option<&str>,
        next: NextExecute<'_>,
    ) -> Response {
        next.run(ctx, operation_name)
            .instrument(tracing::info_span!("graphql.execute"))
            .await
    }

    async fn resolve(
        &self,
        ctx: &ExtensionContext<'_>,
        info: ResolveInfo<'_>,
        next: NextResolve<'_>,
    ) -> ServerResult<Option<Value>> {
        if info.is_for_introspection {
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

    use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
    use tracing::Subscriber;
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};
    use tracing_subscriber::registry::LookupSpan;

    use super::Tracing;

    type CapturedSpan = (String, Option<String>);
    type CapturedSpanList = Arc<Mutex<Vec<CapturedSpan>>>;

    #[derive(Clone, Default)]
    struct CapturedSpans(CapturedSpanList);

    impl<S> Layer<S> for CapturedSpans
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            id: &tracing::span::Id,
            context: Context<'_, S>,
        ) {
            let parent = attributes
                .parent()
                .and_then(|parent| context.span(parent))
                .or_else(|| context.lookup_current())
                .map(|span| span.metadata().name().to_string());
            self.0
                .lock()
                .expect("capture lock")
                .push((attributes.metadata().name().to_string(), parent));
            let _ = id;
        }
    }

    struct Query;

    #[Object]
    impl Query {
        async fn value(&self) -> i32 {
            42
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolver_span_is_a_child_of_graphql_execution() {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
            .extension(Tracing)
            .finish();

        let response = schema.execute("{ value }").await;

        assert!(response.errors.is_empty());
        assert!(
            captured
                .0
                .lock()
                .expect("capture lock")
                .iter()
                .any(|(name, parent)| name == "graphql.resolve"
                    && parent.as_deref() == Some("graphql.execute"))
        );
    }
}
