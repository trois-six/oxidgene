//! Runtime configuration and graceful shutdown shared by the HTTP server and
//! background worker, and the HTTP application the server binds.

pub mod config;
pub mod http;
pub mod shutdown;
