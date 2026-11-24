//! Explicit copy-only exports. The catalog and original media are never written here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
pub struct Target {
    pub id: String,
    pub revision: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub output: String,
    pub motion_output: Option<String>,
    pub rating: Option<i32>,
    pub bytes: u64,
    pub status: String,
    pub error: Option<String>,
    pub caption: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub destination: String,
    pub status: String,
    pub bytes: u64,
    pub verified: usize,
    pub items: Vec<Row>,
    pub message: Option<String>,
    pub kind: String,
    pub watermark: Option<crate::watermark::Options>,
}

#[cfg(unix)]
mod engine;
#[cfg(unix)]
pub(crate) use engine::files;
#[cfg(unix)]
pub use engine::*;
#[cfg(not(unix))]
#[path = "exports/unsupported.rs"]
mod engine;
#[cfg(not(unix))]
pub use engine::*;
#[cfg(all(test, unix))]
mod tests;
