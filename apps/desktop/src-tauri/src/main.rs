#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backups;
#[cfg(test)]
mod benchmarks;
mod catalog;
mod demo;
mod exit_guard;
mod exports;
mod imaging;
mod instance;
#[cfg(test)]
mod large_library;
#[cfg(all(test, target_os = "macos"))]
mod mobile;
mod moves;
mod preparation;
mod relocation;
mod resources;
mod sidecar;
#[cfg(test)]
mod stability;
mod startup;
#[cfg(test)]
mod sustainable;
#[cfg(all(test, target_os = "macos"))]
mod trial_workflow;
mod watermark;
mod watermark_style;
mod workspace;
mod xmp;

use catalog::{Asset, Batch, Catalog, Result};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::UNIX_EPOCH,
};
use tauri::{Emitter, Manager, ipc::Channel};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct StyleExitState {
    gate: exit_guard::Gate,
    channel: Option<Channel<u32>>,
}

#[cfg(target_os = "macos")]
fn style_exit_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
    let menu = Menu::default(app)?;
    // Keep the standard About/Services/Hide/Edit/Window menus. The native
    // predefined Quit action bypasses the preventable close handshake, so
    // replace only that final item with an explicit guarded action.
    if let Some(MenuItemKind::Submenu(application)) = menu.items()?.first() {
        let items = application.items()?;
        let position = items
            .len()
            .checked_sub(1)
            .ok_or_else(|| tauri::Error::Io(std::io::Error::other("应用菜单缺少退出入口")))?;
        application.remove_at(position)?;
        application.insert(
            &MenuItem::with_id(
                app,
                "save-style-and-quit",
                "Quit Moment Grove",
                true,
                Some("CmdOrCtrl+Q"),
            )?,
            position,
        )?;
    }
    for item in menu.items()? {
        if let MenuItemKind::Submenu(submenu) = item {
            match submenu.text()?.as_str() {
                "File" => {
                    submenu.set_text("文件")?;
                    for (position, (id, text, shortcut)) in [
                        ("open", "打开照片文件夹…", Some("CmdOrCtrl+O")),
                        ("export", "导出照片…", Some("CmdOrCtrl+E")),
                        ("watermark", "日期城市水印…", Some("CmdOrCtrl+Shift+E")),
                        ("rescan", "重新扫描", Some("CmdOrCtrl+Shift+R")),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        submenu.insert(
                            &MenuItem::with_id(app, format!("grove-{id}"), text, true, shortcut)?,
                            position,
                        )?;
                    }
                    submenu.insert(&PredefinedMenuItem::separator(app)?, 4)?;
                }
                "Edit" => {
                    submenu.insert(
                        &MenuItem::with_id(app, "grove-undo", "撤销评分", true, None::<&str>)?,
                        0,
                    )?;
                    submenu.append(&MenuItem::with_id(
                        app,
                        "grove-search",
                        "搜索照片",
                        true,
                        Some("CmdOrCtrl+F"),
                    )?)?;
                }
                "Help" => {
                    submenu.set_text("帮助")?;
                    submenu.append(&MenuItem::with_id(
                        app,
                        "grove-help",
                        "快捷键",
                        true,
                        None::<&str>,
                    )?)?;
                    submenu.append(&MenuItem::with_id(
                        app,
                        "grove-guide",
                        "使用说明与资料",
                        true,
                        None::<&str>,
                    )?)?;
                }
                _ => {}
            }
        }
    }
    let tools = tauri::menu::Submenu::new(app, "资料库", true)?;
    for (id, text) in [
        ("move", "移动与隔离…"),
        ("sync", "评分同步…"),
        ("backups", "资料库备份…"),
        ("relink", "重新关联文件夹…"),
    ] {
        tools.append(&MenuItem::with_id(
            app,
            format!("grove-{id}"),
            text,
            true,
            None::<&str>,
        )?)?;
    }
    menu.insert(&tools, 3)?;
    Ok(menu)
}

#[tauri::command]
fn register_style_exit_guard(
    window: tauri::WebviewWindow,
    on_request: Channel<u32>,
    state: tauri::State<'_, Mutex<StyleExitState>>,
) -> Result<()> {
    if window.label() != "main" {
        return Err("退出保护仅适用于主窗口".into());
    }
    let mut state = state.lock().map_err(|_| "退出保护忙碌")?;
    state.channel = Some(on_request);
    state.gate.arm();
    Ok(())
}

#[tauri::command]
fn resolve_style_exit(
    window: tauri::WebviewWindow,
    token: u32,
    proceed: bool,
    state: tauri::State<'_, Mutex<StyleExitState>>,
) -> Result<()> {
    if window.label() != "main" {
        return Err("退出保护仅适用于主窗口".into());
    }
    state
        .lock()
        .map_err(|_| "退出保护忙碌")?
        .gate
        .resolve(token, proceed)?;
    if proceed {
        window.app_handle().exit(0);
    }
    Ok(())
}

fn guard_style_exit(app: &tauri::AppHandle) -> bool {
    let state = app.state::<Mutex<StyleExitState>>();
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return true, // Never treat a failed guard as save success.
    };
    match state.gate.request() {
        exit_guard::Request::Allow => false,
        exit_guard::Request::Wait => true,
        exit_guard::Request::Notify(token) => {
            if state
                .channel
                .as_ref()
                .is_none_or(|c| c.send(token).is_err())
            {
                let _ = state.gate.resolve(token, false);
                app.dialog()
                    .message("无法确认退出前的数据保存状态，已保留窗口。请稍后重试；强制退出可能丢失最近的评分、审片位置或样式修改。")
                    .title("Moment Grove 保存保护")
                    .show(|_| {});
            }
            true
        }
    }
}

pub struct AppState {
    catalog: Mutex<Catalog>,
    allowed: Mutex<HashMap<i64, PathBuf>>,
    granted_identities: Mutex<HashMap<i64, String>>,
    data: PathBuf,
    generation: AtomicU64,
    decode: Arc<tokio::sync::Semaphore>,
    queue: Arc<tokio::sync::Semaphore>,
    relocation: Mutex<Option<relocation::Plan>>,
    export_plan: Mutex<Option<exports::Plan>>,
    export_busy: AtomicBool,
    export_cancel: AtomicBool,
    watermark_render: Arc<tokio::sync::Semaphore>,
    move_plan: Mutex<Option<moves::Plan>>,
    move_busy: AtomicBool,
    move_cancel: AtomicBool,
}
struct ExportBusy<'a>(&'a AtomicBool);
impl Drop for ExportBusy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl<'a> ExportBusy<'a> {
    fn take(flag: &'a AtomicBool) -> Result<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "导出仍在处理，请稍后重试")?;
        Ok(Self(flag))
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Folder {
    id: i64,
    name: String,
    demo: bool,
}
#[derive(Clone, Serialize)]
struct SkippedMedia {
    videos: usize,
    raw: usize,
    images: usize,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ScanEvent {
    Batch {
        items: Vec<Asset>,
    },
    Done {
        count: usize,
        warnings: Vec<String>,
        skipped: SkippedMedia,
    },
    Cancelled,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationInfo {
    version: &'static str,
    build_id: &'static str,
    channel: &'static str,
    data_directory: String,
    isolated: bool,
    executable: String,
    startup: Option<startup::Summary>,
}
fn channel() -> &'static str {
    option_env!("MOMENT_GROVE_CHANNEL").unwrap_or("development")
}
fn isolated() -> bool {
    channel() != "public-prerelease" || std::env::var_os("MOMENT_GROVE_DATA_DIR").is_some()
}
#[tauri::command]
fn application_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> ApplicationInfo {
    ApplicationInfo {
        version: env!("CARGO_PKG_VERSION"),
        build_id: option_env!("MOMENT_GROVE_BUILD_ID").unwrap_or("development"),
        channel: channel(),
        data_directory: state.data.to_string_lossy().into_owned(),
        isolated: isolated(),
        executable: std::env::current_exe()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        startup: app
            .try_state::<Arc<startup::Session>>()
            .map(|s| s.summary()),
    }
}
#[tauri::command]
async fn record_startup_stage(
    window: tauri::WebviewWindow,
    stage: startup::FrontendStage,
    session: tauri::State<'_, Arc<startup::Session>>,
) -> Result<()> {
    if window.label() != "main" {
        return Err("启动状态仅适用于主窗口".into());
    }
    let session = session.inner().clone();
    tauri::async_runtime::spawn_blocking(move || session.record(stage.into()))
        .await
        .map_err(|_| "无法记录启动状态".to_string())?;
    Ok(())
}
#[tauri::command]
async fn load_watermark_style(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<watermark_style::Style>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = state.catalog.lock().map_err(|_| "资料库忙碌")?;
        watermark_style::load(&state.data)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_watermark_style(
    style: watermark_style::Style,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = state.catalog.lock().map_err(|_| "资料库忙碌")?;
        watermark_style::save(&state.data, style)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn load_workspace(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<workspace::Saved>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || workspace::load(&state))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_workspace(
    folder_id: Option<i64>,
    view: workspace::View,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || workspace::save(&state, folder_id, view))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn resume_workspace(state: tauri::State<'_, Arc<AppState>>) -> Result<Option<Folder>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || workspace::resume(&state))
        .await
        .map_err(|e| e.to_string())?
}
fn grant(state: &AppState, path: PathBuf) -> Result<Folder> {
    let path = path.canonicalize().map_err(|_| "目录不可用")?;
    if !path.is_dir() {
        return Err("请选择文件夹".into());
    }
    let id = state
        .catalog
        .lock()
        .map_err(|_| "目录忙碌")?
        .folder(&path)?;
    let demo = path == state.data.join("demo");
    let identity = imaging::directory_identity(&path)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "照片文件夹".into());
    state
        .allowed
        .lock()
        .map_err(|_| "目录忙碌")?
        .insert(id, path);
    state
        .granted_identities
        .lock()
        .map_err(|_| "目录忙碌")?
        .insert(id, identity);
    Ok(Folder { id, name, demo })
}
#[tauri::command]
async fn choose_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<Folder>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let window = app.get_webview_window("main").ok_or("主窗口不可用")?;
        let selected = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择测试照片文件夹（只读浏览）")
            .blocking_pick_folder();
        selected
            .map(|s| {
                s.into_path()
                    .map_err(|_| "目录路径无效".to_string())
                    .and_then(|p| {
                        let _access = moves::access(&state.data, true)?;
                        grant(&state, p)
                    })
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn create_demo(state: tauri::State<'_, Arc<AppState>>) -> Result<Folder> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let path = state.data.join("demo");
        demo::generate(&path)?;
        grant(&state, path)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn move_history(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<moves::Report>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _access = moves::access(&state.data, true)?;
        moves::history(&state.data)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects app/state alongside the review inputs and channel.
async fn choose_move_folder(
    app: tauri::AppHandle,
    folder_id: Option<i64>,
    targets: Vec<moves::Target>,
    kind: String,
    resume_id: Option<String>,
    restore: bool,
    on_progress: Channel<preparation::Progress>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<moves::Report>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = ExportBusy::take(&state.move_busy)?;
        let _access = moves::access(&state.data, resume_id.is_some())?;
        *state.move_plan.lock().map_err(|_| "整理忙碌")? = None;
        state.move_cancel.store(false, Ordering::SeqCst);
        let window = app.get_webview_window("main").ok_or("主窗口不可用")?;
        let prior = if let Some(id) = &resume_id {
            Some(
                moves::history(&state.data)?
                    .into_iter()
                    .find(|r| &r.id == id)
                    .ok_or("安全整理记录不存在")?,
            )
        } else {
            None
        };
        if let Some(prior) = &prior {
            let root = PathBuf::from(&prior.source);
            if !state
                .allowed
                .lock()
                .map_err(|_| "目录忙碌")?
                .values()
                .any(|p| p == &root)
            {
                let Some(selected) = app
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_title("重新选择本次整理的原照片目录")
                    .blocking_pick_folder()
                else {
                    return Ok(None);
                };
                let p = selected
                    .into_path()
                    .map_err(|_| "目录无效")?
                    .canonicalize()
                    .map_err(|_| "目录不可读")?;
                if p != root {
                    return Err("请选择记录中的原照片目录；未移动文件".into());
                }
                grant(&state, p)?;
            }
        }
        let Some(selected) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(if prior.is_some() {
                "重新选择原目标父目录（用于继续或撤回）"
            } else {
                "选择整理目标父目录（确认后将移动测试副本）"
            })
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let parent = selected
            .into_path()
            .map_err(|_| "目录无效")?
            .canonicalize()
            .map_err(|_| "目录不可读")?;
        if let Some(prior) = &prior
            && Path::new(&prior.destination).parent() != Some(parent.as_path())
        {
            return Err("请选择原目标父目录，不能把旧任务指向其他位置".into());
        }
        prepare_move_review(
            &state,
            folder_id,
            &targets,
            &kind,
            resume_id.as_deref(),
            restore,
            &parent,
            &on_progress,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[allow(clippy::too_many_arguments)]
fn prepare_move_review(
    state: &AppState,
    folder_id: Option<i64>,
    targets: &[moves::Target],
    kind: &str,
    resume_id: Option<&str>,
    restore: bool,
    parent: &Path,
    progress: &Channel<preparation::Progress>,
) -> Result<Option<moves::Report>> {
    *state.move_plan.lock().map_err(|_| "整理忙碌")? = None;
    let mut review = preparation::Review::new(&state.move_cancel, |p| {
        let _ = progress.send(p);
    });
    if review.check().is_err() {
        return Ok(None);
    }
    let c = state.catalog.lock().map_err(|_| "目录忙碌")?;
    let result = if let Some(id) = resume_id {
        moves::resume_with_progress(&c, &state.data, id, restore, &mut review)
    } else {
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        moves::prepare_with_progress(
            &c,
            &roots,
            folder_id.ok_or("请先打开源照片目录")?,
            targets,
            parent,
            &state.data,
            kind,
            &mut review,
        )
    };
    let mut slot = state.move_plan.lock().map_err(|_| "整理忙碌")?;
    if state.move_cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let plan = result?;
    let report = moves::review(&plan);
    *slot = Some(plan);
    Ok(Some(report))
}
fn cancel_move_state(state: &AppState) -> Result<()> {
    state.move_cancel.store(true, Ordering::SeqCst);
    *state.move_plan.lock().map_err(|_| "整理忙碌")? = None;
    Ok(())
}
#[tauri::command]
async fn confirm_move(
    token: String,
    on_event: Channel<moves::Report>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<moves::Report> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = ExportBusy::take(&state.move_busy)?;
        let plan = {
            let mut slot = state.move_plan.lock().map_err(|_| "整理忙碌")?;
            if slot.as_ref().and_then(|p| moves::review(p).review_token) != Some(token) {
                return Err("整理核对已过期，请重新核对".into());
            }
            slot.take().ok_or("整理核对已过期")?
        };
        let (source, _) = moves::locations(&plan);
        if !state
            .allowed
            .lock()
            .map_err(|_| "目录忙碌")?
            .values()
            .any(|p| p == &source)
        {
            return Err("原目录尚未重新授权".into());
        }
        state.generation.fetch_add(1, Ordering::SeqCst);
        let mut c = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let result = moves::run(
            &mut c,
            plan,
            &state.data,
            &state.move_cancel,
            |r| {
                let _ = on_event.send(r);
            },
            |_| Ok(()),
        )?;
        if result.status == "completed" {
            let path = PathBuf::from(&result.destination);
            let id = c.folder(&path)?;
            state
                .allowed
                .lock()
                .map_err(|_| "目录忙碌")?
                .insert(id, path);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn cancel_move(state: tauri::State<'_, Arc<AppState>>) -> Result<()> {
    cancel_move_state(&state)
}
#[tauri::command]
async fn export_history(
    folder_id: i64,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<exports::Report>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !state
            .allowed
            .lock()
            .map_err(|_| "目录忙碌")?
            .contains_key(&folder_id)
        {
            return Err("请先打开照片目录".into());
        }
        exports::history(&state.data, folder_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_export_folder(
    folder_id: i64,
    id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !state
            .allowed
            .lock()
            .map_err(|_| "目录忙碌")?
            .contains_key(&folder_id)
        {
            return Err("请先打开照片目录".into());
        }
        let path = exports::completed_destination(&state.data, folder_id, &id)?;
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("/usr/bin/open")
                .arg("--")
                .arg(path)
                .status()
                .map_err(|_| "无法打开 Finder，请检查导出文件夹是否在线")?;
            if status.success() {
                Ok(())
            } else {
                Err("无法打开导出文件夹".into())
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err("此平台打开文件夹尚未验证".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn choose_export_folder(
    app: tauri::AppHandle,
    folder_id: i64,
    targets: Vec<exports::Target>,
    resume_id: Option<String>,
    watermark: Option<watermark::Options>,
    on_progress: Channel<preparation::Progress>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<exports::Report>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let _busy = ExportBusy::take(&state.export_busy)?;
        *state.export_plan.lock().map_err(|_| "导出忙碌")? = None;
        state.export_cancel.store(false, Ordering::SeqCst);
        let window = app.get_webview_window("main").ok_or("主窗口不可用")?;
        let Some(selected) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择导出父目录（将新建独立副本文件夹）")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| "目录路径无效")?;
        prepare_export_review(
            &state,
            folder_id,
            &targets,
            resume_id.as_deref(),
            watermark.as_ref(),
            &path,
            &on_progress,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[allow(clippy::too_many_arguments)]
fn prepare_export_review(
    state: &AppState,
    folder_id: i64,
    targets: &[exports::Target],
    resume_id: Option<&str>,
    watermark: Option<&watermark::Options>,
    path: &Path,
    progress: &Channel<preparation::Progress>,
) -> Result<Option<exports::Report>> {
    *state.export_plan.lock().map_err(|_| "导出忙碌")? = None;
    let mut review = preparation::Review::new(&state.export_cancel, |p| {
        let _ = progress.send(p);
    });
    if review.check().is_err() {
        return Ok(None);
    }
    let c = state.catalog.lock().map_err(|_| "目录忙碌")?;
    let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
    let result = if let Some(id) = resume_id {
        exports::resume_with_progress(&c, &roots, folder_id, id, path, &state.data, &mut review)
    } else {
        exports::prepare_with_progress(
            &c,
            &roots,
            folder_id,
            targets,
            path,
            &state.data,
            watermark,
            &mut review,
        )
    };
    let mut slot = state.export_plan.lock().map_err(|_| "导出忙碌")?;
    if state.export_cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let plan = result?;
    let report = exports::review(&plan);
    *slot = Some(plan);
    Ok(Some(report))
}
fn cancel_export_state(state: &AppState) -> Result<()> {
    state.export_cancel.store(true, Ordering::SeqCst);
    *state.export_plan.lock().map_err(|_| "导出忙碌")? = None;
    Ok(())
}
#[tauri::command]
async fn confirm_export(
    token: String,
    on_event: Channel<exports::Report>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<exports::Report> {
    let state = state.inner().clone();
    let render_permit = state
        .watermark_render
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let _render_permit = render_permit;
        let _busy = ExportBusy::take(&state.export_busy)?;
        let plan = {
            let mut slot = state.export_plan.lock().map_err(|_| "导出忙碌")?;
            if slot.as_ref().map(|p| exports::review(p).id) != Some(token) {
                return Err("导出核对已过期，请重新选择目标".into());
            }
            slot.take().ok_or("导出核对已过期")?
        };
        {
            let c = state.catalog.lock().map_err(|_| "目录忙碌")?;
            let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
            exports::validate(&c, &roots, &plan)?;
        }
        exports::run(
            plan,
            &state.data,
            &state.export_cancel,
            |report| {
                let _ = on_event.send(report);
            },
            || Ok(()),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn watermark_preview(
    id: String,
    options: watermark::Options,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<watermark::Preview> {
    let state = state.inner().clone();
    let queue = state
        .queue
        .clone()
        .try_acquire_owned()
        .map_err(|_| "预览队列繁忙，请稍后重试")?;
    let permit = state
        .watermark_render
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let (_queue, _permit) = (queue, permit);
        let c = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        exports::preview_watermark(&c, &roots, &id, &options)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn cancel_export(state: tauri::State<'_, Arc<AppState>>) -> Result<()> {
    cancel_export_state(&state)
}
#[tauri::command]
async fn list_relocation_folders(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<relocation::Entry>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        relocation::list(&catalog)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn choose_relocation_folder(
    app: tauri::AppHandle,
    folder_id: i64,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Option<relocation::Review>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        *state.relocation.lock().map_err(|_| "目录忙碌")? = None;
        let window = app.get_webview_window("main").ok_or("主窗口不可用")?;
        let Some(selected) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择已迁移目录的新位置（不搬动照片）")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| "目录路径无效")?;
        let c = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let plan = relocation::prepare(&c, folder_id, &path)?;
        let review = plan.review.clone();
        *state.relocation.lock().map_err(|_| "目录忙碌")? = Some(plan);
        Ok(Some(review))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Relocated {
    folder: Folder,
    safety_backup: backups::Snapshot,
}
#[tauri::command]
async fn confirm_relocation(
    token: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Relocated> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let mut c = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let plan = {
            let mut slot = state.relocation.lock().map_err(|_| "目录忙碌")?;
            if slot.as_ref().map(|p| p.review.token.as_str()) != Some(token.as_str()) {
                return Err("核对已过期，请重新选择目录".into());
            }
            slot.take().ok_or("核对已过期")?
        };
        let mut allowed = state.allowed.lock().map_err(|_| "目录忙碌")?;
        state.generation.fetch_add(1, Ordering::SeqCst);
        let (id, safety_backup) = relocation::apply(&mut c, &plan, &state.data.join("backups"))?;
        allowed.insert(id, plan.new);
        Ok(Relocated {
            folder: Folder {
                id,
                name: plan.review.new_name,
                demo: false,
            },
            safety_backup,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn scan_folder(
    folder_id: i64,
    on_event: Channel<ScanEvent>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    let root = state
        .allowed
        .lock()
        .map_err(|_| "目录忙碌")?
        .get(&folder_id)
        .cloned()
        .ok_or("目录尚未授权")?;
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        scan_directory(&state, &root, folder_id, generation, &on_event)
    })
    .await
    .map_err(|e| e.to_string())?
}
// Skip our generated batches when browsing a parent folder. An explicit
// open of the output itself still works; similarly named personal albums stay.
fn generated_output_directory(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("Moment-Grove-") else {
        return false;
    };
    let rest = rest
        .strip_prefix("Watermark-")
        .or_else(|| rest.strip_prefix("Move-"))
        .or_else(|| rest.strip_prefix("Quarantine-"))
        .unwrap_or(rest);
    let Some((timestamp, suffix)) = rest.split_once('-') else {
        return false;
    };
    !timestamp.is_empty()
        && timestamp.bytes().all(|b| b.is_ascii_digit())
        && suffix.len() == 8
        && suffix.bytes().all(|b| b.is_ascii_hexdigit())
}
// A path is not a volume/directory identity. Reject an unmounted or replaced
// root throughout a scan, including after the last batch callback.
fn verify_scan_root(state: &AppState, root: &Path, folder: i64) -> Result<()> {
    let meta = fs::symlink_metadata(root)
        .map_err(|_| "文件夹已离线或权限失效，扫描未完成；恢复磁盘后重新扫描或重新授权")?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || root
            .canonicalize()
            .map_err(|_| "文件夹已离线，扫描未完成")?
            != root
        || state
            .granted_identities
            .lock()
            .map_err(|_| "目录忙碌")?
            .get(&folder)
            != Some(&imaging::directory_identity(root)?)
    {
        return Err("目录身份已变化，扫描未完成；请通过打开文件夹重新核对授权".into());
    }
    Ok(())
}
fn scan_directory(
    state: &AppState,
    root: &Path,
    folder_id: i64,
    generation: u64,
    on_event: &Channel<ScanEvent>,
) -> Result<()> {
    let _file_access = moves::access(&state.data, false)?;
    let mut count = 0;
    let mut batch = Vec::new();
    let mut resource_index = HashMap::new();
    let mut warnings = Vec::new();
    let mut skipped = SkippedMedia {
        videos: 0,
        raw: 0,
        images: 0,
    };
    // Root failures are not an empty success; descendant failures remain warnings.
    let root_error = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => "文件夹已移动或离线，请重新选择文件夹".to_string(),
        std::io::ErrorKind::PermissionDenied => {
            "没有读取此文件夹的权限，请通过打开文件夹重新授权".to_string()
        }
        _ => "文件夹无法读取，请重新扫描或选择文件夹".to_string(),
    };
    let metadata = fs::metadata(root).map_err(root_error)?;
    if !metadata.is_dir() {
        return Err("原文件夹位置已被其他文件占用，请重新选择文件夹".into());
    }
    fs::read_dir(root).map_err(root_error)?;
    verify_scan_root(state, root, folder_id)?;
    if cfg!(target_os = "macos") && !imaging::directory_identity(root)?.starts_with("volume:") {
        warnings.push(
            "此磁盘未提供可验证的持久身份；重新连接后可能需要重新选择，暂不保证跨挂载续挑".into(),
        );
    }
    let entries = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.')
                    && !name.starts_with("Moment-Grove-Quarantine-")
                    && !(e.file_type().is_dir() && generated_output_directory(&name))
                    && !matches!(
                        name.as_ref(),
                        "secrets" | "node_modules" | "target" | "exports" | "quarantine"
                    )
            }
        });
    for entry in entries {
        if state.generation.load(Ordering::SeqCst) != generation {
            let _ = on_event.send(ScanEvent::Cancelled);
            return Ok(());
        }
        verify_scan_root(state, root, folder_id)?;
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if e.depth() == 0 {
                    return Err("文件夹无法读取，请重新扫描或选择文件夹".into());
                }
                if warnings.len() < 8 {
                    warnings.push("部分目录无法读取".into());
                }
                continue;
            }
        };
        if !entry.file_type().is_file() || entry.file_type().is_symlink() {
            continue;
        }
        let path = entry.path();
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["jpg", "jpeg", "png", "webp", "heic", "heif"].contains(&format.as_str()) {
            match format.as_str() {
                "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "3gp" => skipped.videos += 1,
                "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "rw2" | "orf" | "pef" | "srw" => {
                    skipped.raw += 1
                }
                "gif" | "tif" | "tiff" | "avif" | "bmp" | "jxl" => skipped.images += 1,
                _ => {}
            }
            continue;
        }
        let result = (|| {
            let meta = fs::metadata(path).map_err(|_| "文件不可读")?;
            let key = imaging::identity(path)?;
            let observed = sidecar::capture(root, path, &key);
            let rating = match observed.rating() {
                Ok(r) => r,
                Err(e) => {
                    if warnings.len() < 8 {
                        warnings.push(format!("{}：{e}", entry.file_name().to_string_lossy()));
                    }
                    None
                }
            };
            let (id, rating, revision) = {
                let catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
                if state.generation.load(Ordering::SeqCst) != generation {
                    return Err("扫描已取消".into());
                }
                let item = catalog.upsert(folder_id, path, &key, rating)?;
                sidecar::remember(&catalog, &item.0, &observed)?;
                item
            };
            let (width, height) = image::image_dimensions(path).unwrap_or((0, 0));
            let exif = fs::File::open(path).ok().and_then(|f| {
                exif::Reader::new()
                    .read_from_container(&mut std::io::BufReader::new(f))
                    .ok()
            });
            let captured = exif.as_ref().and_then(|e| {
                e.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
                    .map(|f| f.display_value().to_string())
            });
            let orientation = exif
                .as_ref()
                .and_then(|e| {
                    e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                        .and_then(|f| f.value.get_uint(0))
                })
                .unwrap_or(1);
            let (width, height) = if (5..=8).contains(&orientation) {
                (height, width)
            } else {
                (width, height)
            };
            #[cfg(target_os = "macos")]
            let (width, height, captured) = if ["heic", "heif"].contains(&format.as_str()) {
                imaging::header(path).unwrap_or((width, height, captured))
            } else {
                (width, height, captured)
            };
            let parent = path.parent().ok_or("照片路径无效")?;
            if !resource_index.contains_key(parent) {
                resource_index.insert(parent.to_path_buf(), resources::directory(parent)?);
            }
            let companions = resources::for_photo(&resource_index[parent], path);
            Ok::<_, String>(Asset {
                id,
                folder_id,
                name: entry.file_name().to_string_lossy().into_owned(),
                relative: path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
                width,
                height,
                bytes: meta.len(),
                modified: meta
                    .modified()
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                captured,
                rating,
                revision,
                format,
                companions,
            })
        })();
        match result {
            Ok(asset) => {
                batch.push(asset);
                count += 1;
            }
            Err(e) => {
                if warnings.len() < 8 {
                    warnings.push(e);
                }
            }
        }
        if batch.len() >= 64 {
            on_event
                .send(ScanEvent::Batch {
                    items: std::mem::take(&mut batch),
                })
                .map_err(|e| e.to_string())?;
        }
    }
    if state.generation.load(Ordering::SeqCst) != generation {
        let _ = on_event.send(ScanEvent::Cancelled);
        return Ok(());
    }
    verify_scan_root(state, root, folder_id)?;
    if !batch.is_empty() {
        on_event
            .send(ScanEvent::Batch { items: batch })
            .map_err(|e| e.to_string())?;
    }
    verify_scan_root(state, root, folder_id)?;
    on_event
        .send(ScanEvent::Done {
            count,
            warnings,
            skipped,
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
fn cancel_scan(state: tauri::State<'_, Arc<AppState>>) {
    state.generation.fetch_add(1, Ordering::SeqCst);
}
fn validate_ids(state: &AppState, ids: &[String]) -> Result<()> {
    for id in ids {
        imaging::allowed_path(state, id)?;
    }
    Ok(())
}
#[tauri::command]
async fn set_rating(
    ids: Vec<String>,
    rating: Option<i32>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Batch> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        validate_ids(&state, &ids)?;
        state
            .catalog
            .lock()
            .map_err(|_| "目录忙碌")?
            .rate(&ids, rating)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn undo_rating(token: String, state: tauri::State<'_, Arc<AppState>>) -> Result<Batch> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let mut catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let ids = {
            let mut q = catalog
                .db
                .prepare("SELECT id FROM history WHERE token=?1")
                .map_err(|e| e.to_string())?;
            q.query_map([&token], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let allowed = state.allowed.lock().map_err(|_| "目录忙碌")?;
        for id in ids {
            if !allowed.contains_key(&catalog.location(&id)?.0) {
                return Err("撤销目标目录尚未打开".into());
            }
        }
        catalog.undo(&token)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn list_backups(state: tauri::State<'_, Arc<AppState>>) -> Result<backups::SnapshotList> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        backups::list(&state.data.join("backups"))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn create_backup(state: tauri::State<'_, Arc<AppState>>) -> Result<backups::Snapshot> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        backups::create(&catalog, &state.data.join("backups"))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn restore_backup(
    id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<backups::Restored> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        moves::restore_catalog_allowed(&state.data)?;
        state.generation.fetch_add(1, Ordering::SeqCst);
        let mut catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let mut allowed = state.allowed.lock().map_err(|_| "目录忙碌")?;
        let bindings = moves::catalog_restore_bindings(&catalog, &state.data)?;
        workspace::forget(&state)?;
        let restored = backups::restore_with_locations(
            &mut catalog,
            &state.data.join("backups"),
            &id,
            &bindings,
        )?;
        allowed.clear();
        state
            .granted_identities
            .lock()
            .map_err(|_| "目录忙碌")?
            .clear();
        *state.relocation.lock().map_err(|_| "目录忙碌")? = None;
        Ok(restored)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn inspect_xmp(
    folder_id: i64,
    offset: u32,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<sidecar::Report> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        sidecar::report_page(&catalog, &roots, folder_id, offset)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn sync_xmp(
    folder_id: i64,
    targets: Vec<sidecar::Target>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<sidecar::Outcome> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let mut catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        sidecar::sync(&mut catalog, &roots, folder_id, &targets)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn adopt_xmp(
    folder_id: i64,
    id: String,
    revision: i64,
    token: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Batch> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let mut catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        if catalog.location(&id)?.0 != folder_id {
            return Err("照片不属于当前目录".into());
        }
        sidecar::adopt(&mut catalog, &roots, &id, revision, &token)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn repair_xmp(
    folder_id: i64,
    id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _file_access = moves::access(&state.data, false)?;
        let mut catalog = state.catalog.lock().map_err(|_| "目录忙碌")?;
        let roots = state.allowed.lock().map_err(|_| "目录忙碌")?.clone();
        if catalog.location(&id)?.0 != folder_id {
            return Err("照片不属于当前目录".into());
        }
        sidecar::repair(&mut catalog, &roots, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn activate(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_app(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let data = instance::data_directory(
        &app.path().app_data_dir()?,
        channel(),
        std::env::var_os("MOMENT_GROVE_DATA_DIR").map(PathBuf::from),
    )?;
    // Claim ownership before cache pruning, catalog opening or workspace
    // writes. Separate QA directories intentionally remain independent.
    let mut owner = match instance::claim(&data)? {
        instance::Claim::Owner(owner) => owner,
        instance::Claim::Forwarded => {
            eprintln!(
                "Moment Grove: 已请求唤回此资料库的现有窗口；版本以该窗口为准。更新前请先退出旧版。"
            );
            std::process::exit(0);
        }
    };
    let startup = Arc::new(startup::Session::new(
        &data,
        env!("CARGO_PKG_VERSION"),
        option_env!("MOMENT_GROVE_BUILD_ID").unwrap_or("development"),
    ));
    app.manage(startup.clone());
    fs::create_dir_all(data.join("cache"))?;
    imaging::prune_cache(&data.join("cache"), 256 * 1024 * 1024)?;
    let state = Arc::new(AppState {
        catalog: Mutex::new(
            Catalog::open(&data.join("catalog.sqlite")).map_err(std::io::Error::other)?,
        ),
        allowed: Mutex::new(HashMap::new()),
        granted_identities: Mutex::new(HashMap::new()),
        data,
        generation: AtomicU64::new(0),
        decode: Arc::new(tokio::sync::Semaphore::new(2)),
        queue: Arc::new(tokio::sync::Semaphore::new(24)),
        relocation: Mutex::new(None),
        export_plan: Mutex::new(None),
        export_busy: AtomicBool::new(false),
        export_cancel: AtomicBool::new(false),
        watermark_render: Arc::new(tokio::sync::Semaphore::new(1)),
        move_plan: Mutex::new(None),
        move_busy: AtomicBool::new(false),
        move_cancel: AtomicBool::new(false),
    });
    app.manage(state);
    startup.record(startup::Stage::BackendReady);
    if let Some(window) = app.get_webview_window("main") {
        window.set_title(&format!(
            "Moment Grove {} · {}",
            env!("CARGO_PKG_VERSION"),
            if isolated() {
                "独立测试"
            } else {
                "日常资料库"
            }
        ))?;
        window.show()?;
        // Opening the app is an explicit request to use its main window.
        // A loaded WebView can remain unpainted until macOS activates it.
        window.set_focus()?;
    }
    let handle = app.handle().clone();
    let startup_handle = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(45));
        if startup.warn_if_unready() {
            let _=startup_handle.clone().run_on_main_thread(move || {
                if !startup.summary().ready {
                    startup_handle.dialog().message("界面打开时间较长。若窗口仍为空白，请退出后重新打开。现有资料会保留。诊断代码：MG-START-UI。")
                        .title("Moment Grove 启动提示").show(|_| {});
                }
            });
        }
    });
    std::thread::spawn(move || {
        // Bounded local mailbox; no network listener or media paths.
        // Owner (and its OS lock) live until this process exits.
        loop {
            if owner.activation_requested().unwrap_or(false) {
                let focus = handle.clone();
                let _ = handle.run_on_main_thread(move || activate(&focus));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(style_exit_menu);
    builder
        .manage(Mutex::new(StyleExitState::default()))
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|webview, payload| {
            if webview.label()=="main" && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && let Some(session)=webview.app_handle().try_state::<Arc<startup::Session>>() {
                session.record(startup::Stage::WebviewLoaded);
            }
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "save-style-and-quit" && !guard_style_exit(app) {
                app.exit(0);
            }
            if let Some(command) = event.id().as_ref().strip_prefix("grove-") {
                let _ = app.emit_to("main", "grove-command", command);
            }
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && guard_style_exit(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .setup(|app| {
            if let Err(error) = setup_app(app) {
                if let Some(session)=app.try_state::<Arc<startup::Session>>() { session.record(startup::Stage::BackendFailed); }
                let handle = app.handle().clone();
                app.dialog().message(format!("无法打开 Moment Grove：{error}\n请先退出旧版，再重新打开。保留现有资料；若仍失败，请附此提示联系支持。"))
                    .title("Moment Grove 启动失败")
                    .show(move |_| handle.exit(1));
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("grove", |context, request, responder| {
            let state = context
                .app_handle()
                .state::<Arc<AppState>>()
                .inner()
                .clone();
            let path = request.uri().path().trim_matches('/').to_string();
            let size = request
                .uri()
                .query()
                .unwrap_or("")
                .split('&')
                .find_map(|q| q.strip_prefix("size="))
                .and_then(|q| q.parse::<u32>().ok());
            let refresh = request
                .uri()
                .query()
                .unwrap_or("")
                .split('&')
                .any(|q| q == "refresh=1");
            tauri::async_runtime::spawn(async move {
                let outcome = async {
                    let _queue = state
                        .queue
                        .clone()
                        .try_acquire_owned()
                        .map_err(|_| "预览队列繁忙，请重试".to_string())?;
                    let _decode = state
                        .decode
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|e| e.to_string())?;
                    if uuid::Uuid::parse_str(&path).is_err() {
                        return Err("资源地址无效".into());
                    }
                    let id = path;
                    let size = size.ok_or("预览尺寸无效")?;
                    tauri::async_runtime::spawn_blocking(move || {
                        if refresh {
                            imaging::render_with_refresh(&state, &id, size, true)
                        } else {
                            imaging::render(&state, &id, size)
                        }
                    })
                    .await
                    .map_err(|e| e.to_string())?
                }
                .await;
                let (status, mime, body) = match outcome {
                    Ok(bytes) => (200, "image/jpeg", bytes),
                    Err(e) => (422, "text/plain; charset=utf-8", e.into_bytes()),
                };
                responder.respond(
                    tauri::http::Response::builder()
                        .status(status)
                        .header("Content-Type", mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .header("Cache-Control", "no-store")
                        .body(body)
                        .expect("valid response"),
                );
            });
        })
        .invoke_handler(tauri::generate_handler![
            application_info,
            record_startup_stage,
            register_style_exit_guard,
            resolve_style_exit,
            load_watermark_style,
            save_watermark_style,
            load_workspace,
            save_workspace,
            resume_workspace,
            choose_folder,
            create_demo,
            scan_folder,
            cancel_scan,
            set_rating,
            undo_rating,
            list_backups,
            create_backup,
            restore_backup,
            inspect_xmp,
            sync_xmp,
            adopt_xmp,
            repair_xmp,
            list_relocation_folders,
            choose_relocation_folder,
            confirm_relocation,
            export_history,
            open_export_folder,
            choose_export_folder,
            confirm_export,
            cancel_export,
            watermark_preview,
            move_history,
            choose_move_folder,
            confirm_move,
            cancel_move
        ])
        .build(tauri::generate_context!())
        .expect("Moment Grove failed to start")
        .run(|_app, _event| {
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = &_event
                && guard_style_exit(_app)
            {
                api.prevent_exit();
            }
            #[cfg(target_os = "macos")]
            if matches!(_event, tauri::RunEvent::Reopen { .. }) {
                activate(_app);
            }
        });
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    pub(super) fn state(data: &Path) -> AppState {
        fs::create_dir_all(data.join("cache")).unwrap();
        AppState {
            catalog: Mutex::new(Catalog::open(&data.join("catalog.sqlite")).unwrap()),
            allowed: Mutex::new(HashMap::new()),
            granted_identities: Mutex::new(HashMap::new()),
            data: data.into(),
            generation: AtomicU64::new(0),
            decode: Arc::new(tokio::sync::Semaphore::new(2)),
            queue: Arc::new(tokio::sync::Semaphore::new(24)),
            relocation: Mutex::new(None),
            export_plan: Mutex::new(None),
            export_busy: AtomicBool::new(false),
            export_cancel: AtomicBool::new(false),
            watermark_render: Arc::new(tokio::sync::Semaphore::new(1)),
            move_plan: Mutex::new(None),
            move_busy: AtomicBool::new(false),
            move_cancel: AtomicBool::new(false),
        }
    }
    pub(super) fn register(s: &AppState, path: &Path) -> (i64, String) {
        let c = s.catalog.lock().unwrap();
        let root = c.folder(path.parent().unwrap()).unwrap();
        let id = c
            .upsert(root, path, &imaging::identity(path).unwrap(), None)
            .unwrap()
            .0;
        (root, id)
    }
    #[test]
    fn native_review_channels_cancel_without_catalog_lock_and_revoke_confirmation() {
        for mode in ["copy", "watermark", "move"] {
            let t = tempfile::tempdir().unwrap();
            let base = t.path().canonicalize().unwrap();
            let s = Arc::new(state(&base.join("runtime")));
            let source = base.join("sources");
            fs::create_dir(&source).unwrap();
            let parent = base.join("outputs");
            fs::create_dir(&parent).unwrap();
            let folder = grant(&s, source.clone()).unwrap();
            let mut copy_targets = Vec::new();
            let mut move_targets = Vec::new();
            for n in 0..3 {
                let p = source.join(format!("generated-{n}.jpg"));
                image::RgbImage::new(48, 32).save(&p).unwrap();
                let (_, id) = register(&s, &p);
                let c = s.catalog.lock().unwrap();
                sidecar::remember(
                    &c,
                    &id,
                    &sidecar::capture(&source, &p, &imaging::identity(&p).unwrap()),
                )
                .unwrap();
                copy_targets.push(exports::Target {
                    id: id.clone(),
                    revision: 0,
                });
                move_targets.push(moves::Target { id, revision: 0 });
            }
            for cutoff in [1, 3, 99] {
                s.move_cancel.store(false, Ordering::SeqCst);
                s.export_cancel.store(false, Ordering::SeqCst);
                let values = Arc::new(Mutex::new(Vec::new()));
                let received = values.clone();
                let observed = s.clone();
                let moving = mode == "move";
                let channel = Channel::new(move |body| {
                    if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                        let p: serde_json::Value = serde_json::from_str(&text).unwrap();
                        if p["phase"] == "checking" && p["checked"] == cutoff {
                            if moving {
                                cancel_move_state(&observed).unwrap();
                            } else {
                                cancel_export_state(&observed).unwrap();
                            }
                        }
                        received.lock().unwrap().push(p);
                    }
                    Ok(())
                });
                let present = if moving {
                    prepare_move_review(
                        &s,
                        Some(folder.id),
                        &move_targets,
                        "move",
                        None,
                        false,
                        &parent,
                        &channel,
                    )
                    .unwrap()
                    .is_some()
                } else {
                    prepare_export_review(
                        &s,
                        folder.id,
                        &copy_targets,
                        None,
                        (mode == "watermark").then_some(&watermark::Options {
                            fallback_date: Some("2026-11-10".into()),
                            ..Default::default()
                        }),
                        &parent,
                        &channel,
                    )
                    .unwrap()
                    .is_some()
                };
                assert_eq!(present, cutoff == 99);
                let events = values.lock().unwrap();
                assert!(!events.is_empty());
                assert!(events.iter().all(|p| p["total"] == 3));
                if moving {
                    assert_eq!(s.move_plan.lock().unwrap().is_some(), present);
                    cancel_move_state(&s).unwrap();
                    assert!(s.move_plan.lock().unwrap().is_none());
                } else {
                    assert_eq!(s.export_plan.lock().unwrap().is_some(), present);
                    cancel_export_state(&s).unwrap();
                    assert!(s.export_plan.lock().unwrap().is_none());
                }
                assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
                assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
            }
        }
    }
    #[test]
    fn pending_move_blocks_scans_and_quarantine_directories_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let b = temp.path().canonicalize().unwrap();
        let s = state(&b.join("app-data"));
        let root = b.join("generated");
        fs::create_dir(&root).unwrap();
        let ignored = root.join("Moment-Grove-Quarantine-generated");
        fs::create_dir(&ignored).unwrap();
        image::RgbImage::new(24, 16)
            .save(root.join("generated.png"))
            .unwrap();
        image::RgbImage::new(24, 16)
            .save(ignored.join("not-scanned.png"))
            .unwrap();
        let f = grant(&s, root.clone()).unwrap();
        let channel = Channel::new(|_| Ok(()));
        scan_directory(&s, &root, f.id, 0, &channel).unwrap();
        let parent = b.join("destination");
        fs::create_dir(&parent).unwrap();
        let mut c = s.catalog.lock().unwrap();
        let (id, revision): (String, i64) =
            c.db.query_row("SELECT id,revision FROM assets", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        let p = moves::prepare(
            &c,
            &HashMap::from([(f.id, root.clone())]),
            f.id,
            &[moves::Target { id, revision }],
            &parent,
            &s.data,
            "move",
        )
        .unwrap();
        let r = moves::run(
            &mut c,
            p,
            &s.data,
            &AtomicBool::new(false),
            |_| {},
            |phase| {
                if phase == "published" {
                    Err("Injected interruption".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(r.status, "partial");
        drop(c);
        assert!(scan_directory(&s, &root, f.id, 0, &channel).is_err());
        let mut c = s.catalog.lock().unwrap();
        assert_eq!(
            c.db.query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let p = moves::resume(&c, &s.data, &r.id, true).unwrap();
        assert_eq!(
            moves::run(
                &mut c,
                p,
                &s.data,
                &AtomicBool::new(false),
                |_| {},
                |_| Ok(())
            )
            .unwrap()
            .status,
            "restored"
        );
        drop(c);
        scan_directory(&s, &root, f.id, 0, &channel).unwrap();
        assert_eq!(
            s.catalog
                .lock()
                .unwrap()
                .db
                .query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn scan_cancellation_stops_catalog_writes_and_does_not_emit_done() {
        let temp = tempfile::tempdir().unwrap();
        let s = Arc::new(state(&temp.path().canonicalize().unwrap()));
        // The command receives a canonical picker grant, including on Macs
        // whose default temporary directory begins with the /var alias.
        let media = temp.path().canonicalize().unwrap().join("generated");
        fs::create_dir(&media).unwrap();
        // Header errors are isolated by the scanner, so tiny synthetic files suffice here.
        for i in 0..100 {
            fs::write(media.join(format!("generated-{i}.jpg")), b"test header").unwrap();
        }
        let folder = grant(&s, media.clone()).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let state = s.clone();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                observed
                    .lock()
                    .unwrap()
                    .push(value["kind"].as_str().unwrap().to_string());
                if value["kind"] == "batch" {
                    state.generation.fetch_add(1, Ordering::SeqCst);
                }
            }
            Ok(())
        });
        scan_directory(&s, &media, folder.id, 0, &channel).unwrap();
        assert_eq!(*events.lock().unwrap(), vec!["batch", "cancelled"]);
        let persisted: i64 = s
            .catalog
            .lock()
            .unwrap()
            .db
            .query_row("SELECT count(*) FROM assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(persisted, 64);
    }
    #[test]
    fn previews_require_granted_roots_and_preserve_original_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let s = state(temp.path());
        let media = temp.path().join("media");
        fs::create_dir(&media).unwrap();
        let path = media.join("test.png");
        image::RgbImage::new(40, 80).save(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let (root, id) = register(&s, &path);
        assert!(imaging::render(&s, &id, 480).is_err());
        s.allowed
            .lock()
            .unwrap()
            .insert(root, media.canonicalize().unwrap());
        let preview = imaging::render(&s, &id, 480).unwrap();
        let decoded = image::load_from_memory(&preview).unwrap();
        assert_eq!(decoded.width() * 2, decoded.height());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(imaging::render(&s, &id, 480).unwrap(), preview);
        assert!(imaging::render(&s, &id, 999).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_or_replace_an_authorized_file() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let s = state(temp.path());
        let media = temp.path().join("media");
        fs::create_dir(&media).unwrap();
        let path = media.join("test.png");
        image::RgbImage::new(4, 4).save(&path).unwrap();
        let (root, id) = register(&s, &path);
        s.allowed
            .lock()
            .unwrap()
            .insert(root, media.canonicalize().unwrap());
        fs::rename(&path, temp.path().join("old.png")).unwrap();
        let outside = temp.path().join("outside.png");
        image::RgbImage::new(8, 8).save(&outside).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(imaging::render(&s, &id, 480).is_err());
    }
    #[test]
    fn corrupt_images_are_explicit_errors_and_cache_has_a_byte_limit() {
        let temp = tempfile::tempdir().unwrap();
        let s = state(temp.path());
        let path = temp.path().join("broken.jpg");
        fs::write(&path, b"not an image").unwrap();
        let (root, id) = register(&s, &path);
        s.allowed
            .lock()
            .unwrap()
            .insert(root, temp.path().canonicalize().unwrap());
        assert!(imaging::render(&s, &id, 480).is_err());
        for i in 0..5 {
            fs::write(s.data.join("cache").join(format!("{i}.jpg")), [0_u8; 100]).unwrap();
        }
        imaging::prune_cache(&s.data.join("cache"), 200).unwrap();
        let bytes: u64 = fs::read_dir(s.data.join("cache"))
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum();
        assert!(bytes <= 200);
    }
    #[test]
    fn transparent_preview_composites_over_white_and_preserves_source() {
        let temp = tempfile::tempdir().unwrap();
        let s = state(temp.path());
        let path = temp.path().join("透明 中文.png");
        image::RgbaImage::from_fn(80, 40, |x, _| {
            if x < 40 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([255, 0, 0, 128])
            }
        })
        .save(&path)
        .unwrap();
        let before = fs::read(&path).unwrap();
        let (folder, id) = register(&s, &path);
        s.allowed
            .lock()
            .unwrap()
            .insert(folder, temp.path().canonicalize().unwrap());
        let output = imaging::render(&s, &id, 480).unwrap();
        let image = image::load_from_memory(&output).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (80, 40));
        assert!(
            image.get_pixel(10, 20).0.iter().all(|c| *c > 245),
            "transparent background must be white"
        );
        let pixel = image.get_pixel(65, 20).0;
        assert!(
            pixel[0] > 245 && (115..=140).contains(&pixel[1]) && (115..=140).contains(&pixel[2]),
            "half-transparent red must blend over white: {pixel:?}"
        );
        assert_eq!(fs::read(path).unwrap(), before);
    }
    #[test]
    fn exif_rotation_is_applied_once_and_small_images_are_not_upscaled() {
        let temp = tempfile::tempdir().unwrap();
        let s = state(temp.path());
        let path = temp.path().join("oriented.jpg");
        let image = image::RgbImage::from_pixel(40, 80, image::Rgb([30, 80, 120]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image)
            .unwrap();
        let exif: Vec<u8> = vec![
            69, 120, 105, 102, 0, 0, 73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6,
            0, 0, 0, 0, 0, 0, 0,
        ];
        let mut oriented = jpeg[..2].to_vec();
        oriented.extend_from_slice(&[255, 225]);
        oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend_from_slice(&exif);
        oriented.extend_from_slice(&jpeg[2..]);
        fs::write(&path, &oriented).unwrap();
        let (root, id) = register(&s, &path);
        s.allowed
            .lock()
            .unwrap()
            .insert(root, temp.path().canonicalize().unwrap());
        let preview = image::load_from_memory(&imaging::render(&s, &id, 480).unwrap()).unwrap();
        assert_eq!((preview.width(), preview.height()), (80, 40));
        assert_eq!(fs::read(path).unwrap(), oriented);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn generated_sdr_heic_decodes_without_modifying_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.jpg");
        image::RgbImage::from_pixel(40, 80, image::Rgb([20, 120, 70]))
            .save(&source)
            .unwrap();
        let heic = temp.path().join("source.heic");
        let output = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "heic"])
            .arg(&source)
            .arg("--out")
            .arg(&heic)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "System HEIC encoder unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let before = fs::read(&heic).unwrap();
        let s = state(&temp.path().join("runtime"));
        let (folder, id) = register(&s, &heic);
        s.allowed
            .lock()
            .unwrap()
            .insert(folder, temp.path().canonicalize().unwrap());
        let preview = image::load_from_memory(&imaging::render(&s, &id, 480).unwrap()).unwrap();
        assert_eq!(preview.width() * 2, preview.height());
        assert_eq!(fs::read(heic).unwrap(), before);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod compatibility;
