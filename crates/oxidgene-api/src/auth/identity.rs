//! The identity a request carries (docs/authentication.md §5.4). The
//! authentication layer puts one [`Principal`] in every request's
//! extensions; the authorization that follows builds the request's
//! `ViewerContext` (docs/authorization.md §4) on it. Both types live in
//! `oxidgene_core::access`, which every build shares.

pub use oxidgene_core::access::{Identity, Principal};
