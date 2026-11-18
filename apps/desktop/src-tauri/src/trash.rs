//! Explicit zero-rating-only Trash requests. Never unlink originals or empty Trash.
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceResult {
    pub name: String,
    pub status: String,
    pub destination: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub status: String,
    pub resources: Vec<ResourceResult>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub created: u64,
    pub source_folder: i64,
    pub source: String,
    pub status: String,
    pub completed: usize,
    pub items: Vec<Row>,
    pub message: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub token: String,
    pub report: Report,
}

#[cfg(target_os = "macos")]
mod engine;
#[cfg(target_os = "macos")]
pub use engine::*;
#[cfg(not(target_os = "macos"))]
pub struct Plan;
#[cfg(not(target_os = "macos"))]
pub fn prepare(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: i64,
    _: &[Target],
    _: &AtomicBool,
) -> Result<Plan> {
    Err("移到废纸篓目前仅支持 macOS；未删除任何文件".into())
}
#[cfg(not(target_os = "macos"))]
pub fn review(_: &Plan) -> Review {
    unreachable!()
}
#[cfg(not(target_os = "macos"))]
pub fn run(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: Plan,
    _: &Path,
    _: &AtomicBool,
    _: impl FnMut(Report),
) -> Result<Report> {
    Err("此平台不支持移到废纸篓".into())
}
#[cfg(not(target_os = "macos"))]
pub fn history(_: &Path) -> Result<Vec<Report>> {
    Ok(vec![])
}
