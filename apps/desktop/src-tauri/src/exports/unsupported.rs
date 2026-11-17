use super::{Report, Target};
use crate::catalog::{Catalog, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
pub struct Plan;
pub fn review(_: &Plan) -> Report {
    unreachable!("Unsupported platform never prepares exports")
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_with_progress(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: i64,
    _: &[Target],
    _: &Path,
    _: &Path,
    _: Option<&crate::watermark::Options>,
    _: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    Err("此平台导出尚未验证".into())
}
pub fn preview_watermark(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: &str,
    _: &crate::watermark::Options,
) -> Result<crate::watermark::Preview> {
    Err("此平台水印预览尚未验证".into())
}
#[allow(clippy::too_many_arguments)]
pub fn resume_with_progress(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: i64,
    _: &str,
    _: &Path,
    _: &Path,
    _: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    Err("此平台复制导出尚未验证".into())
}
pub fn history(_: &Path, _: i64) -> Result<Vec<Report>> {
    Ok(Vec::new())
}
pub fn completed_destination(_: &Path, _: i64, _: &str) -> Result<PathBuf> {
    Err("此平台导出尚未验证".into())
}
pub fn validate(_: &Catalog, _: &HashMap<i64, PathBuf>, _: &Plan) -> Result<()> {
    Err("此平台复制导出尚未验证".into())
}
pub fn run(
    _: Plan,
    _: &Path,
    _: &AtomicBool,
    _: impl FnMut(Report),
    _: impl FnMut() -> Result<()>,
) -> Result<Report> {
    Err("此平台复制导出尚未验证".into())
}
