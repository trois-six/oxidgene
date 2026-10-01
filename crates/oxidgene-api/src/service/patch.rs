//! The patch convention every update shares: `None` keeps a field,
//! `Some(None)` clears it, `Some(Some(v))` sets it.

use serde::Deserialize;

/// Deserializer for update fields that must tell "absent" from `null`.
///
/// serde maps a JSON `null` to `None` for *any* `Option`, so a plain
/// `Option<Option<T>>` collapses `{"x": null}` and `{}` to the same `None` —
/// which reads as "leave unchanged", so no nullable field could ever be
/// cleared. Paired with `#[serde(default)]` this restores the distinction:
/// absent stays `None`, `null` becomes `Some(None)`. GraphQL inputs express
/// the same three cases with `MaybeUndefined`.
pub(crate) fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}
