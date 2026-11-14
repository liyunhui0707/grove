//! Explicit, recoverable same-volume moves. Source bytes are never rewritten or deleted.
use crate::catalog::{Catalog, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

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
    pub rating: Option<i32>,
    pub sidecar: bool,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub review_token: Option<String>,
    pub source: String,
    pub source_folder: i64,
    pub destination: String,
    pub kind: String,
    pub direction: String,
    pub status: String,
    pub verified: usize,
    pub items: Vec<Row>,
    pub safety_backup: Option<String>,
    pub message: Option<String>,
}

#[cfg(target_os = "macos")]
mod engine;
#[cfg(target_os = "macos")]
pub use engine::*;
#[cfg(not(target_os = "macos"))]
#[path = "moves/unsupported.rs"]
mod engine;
#[cfg(not(target_os = "macos"))]
pub use engine::*;
#[cfg(all(test, target_os = "macos"))]
mod tests;
