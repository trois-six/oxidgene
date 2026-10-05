//! Query strings, percent-encoded by hand: the adapters need only this, and
//! the web build would otherwise link a URL library for it.

/// A query string under construction, in insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Query(Vec<(String, String)>);

impl Query {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.0.push((name.into(), value.into()));
        self
    }
}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (name, value)) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("&")?;
            }
            write!(f, "{}={}", encode(name), encode(value))?;
        }
        Ok(())
    }
}

/// Percent-encodes every byte of `text` but the unreserved characters of
/// RFC 3986, so brackets, pipes and accented letters travel safely in a
/// query.
pub(crate) fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_and_non_ascii_characters() {
        assert_eq!(
            encode("Exampleville-sur-Mer_1.~"),
            "Exampleville-sur-Mer_1.~"
        );
        assert_eq!(encode("Saint Étienne"), "Saint%20%C3%89tienne");
        assert_eq!(
            encode("a[b][q][]=1|2&c"),
            "a%5Bb%5D%5Bq%5D%5B%5D%3D1%7C2%26c"
        );
    }

    #[test]
    fn joins_pairs_in_order() {
        let mut query = Query::new();
        query
            .push("refUnique", "x")
            .push("x--from", "0")
            .push("q[]", "Le Bourg");
        assert_eq!(
            query.to_string(),
            "refUnique=x&x--from=0&q%5B%5D=Le%20Bourg"
        );
        assert_eq!(Query::new().to_string(), "");
    }
}
