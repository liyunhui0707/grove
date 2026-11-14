use super::*;
pub struct Access;
pub struct Plan;
pub fn access(_: &Path, _: bool) -> Result<Access> {
    Ok(Access)
}
pub fn restore_catalog_allowed(_: &Path) -> Result<()> {
    Ok(())
}
pub fn catalog_restore_bindings(_: &Catalog, _: &Path) -> Result<Vec<(String, String, String)>> {
    Ok(Vec::new())
}
pub fn history(_: &Path) -> Result<Vec<Report>> {
    Ok(Vec::new())
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_with_progress(
    _: &Catalog,
    _: &HashMap<i64, PathBuf>,
    _: i64,
    _: &[Target],
    _: &Path,
    _: &Path,
    _: &str,
    _: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    Err("此平台安全整理尚未验证".into())
}
pub fn review(_: &Plan) -> Report {
    unreachable!("No plan can be prepared on this platform")
}
pub fn locations(_: &Plan) -> (PathBuf, PathBuf) {
    unreachable!("No plan can be prepared on this platform")
}
pub fn resume_with_progress(
    _: &Catalog,
    _: &Path,
    _: &str,
    _: bool,
    _: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    Err("此平台安全整理尚未验证".into())
}
pub fn run(
    _: &mut Catalog,
    _: Plan,
    _: &Path,
    _: &AtomicBool,
    _: impl FnMut(Report),
    _: impl FnMut(&str) -> Result<()>,
) -> Result<Report> {
    Err("此平台安全整理尚未验证".into())
}
