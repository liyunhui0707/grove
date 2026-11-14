use crate::{AppState, catalog::Result};
#[cfg(not(target_os = "macos"))]
use image::{DynamicImage, ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::{draw_oriented, unequal_density};
#[cfg(not(target_os = "macos"))]
use std::io::BufReader;

pub fn identity(path: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "照片无法读取")?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("不读取符号链接或非普通文件".into());
    }
    identity_from_metadata(&meta)
}
pub(crate) fn identity_from_metadata(meta: &fs::Metadata) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{:?}",
            meta.dev(),
            meta.ino(),
            meta.created().ok()
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(format!(
            "{:?}",
            meta.created().map_err(|_| "无法读取文件身份")?
        ))
    }
}
pub fn fingerprint(path: &Path) -> Result<String> {
    let m = fs::metadata(path).map_err(|_| "照片已离线")?;
    Ok(format!(
        "{}-{}",
        m.len(),
        m.modified()
            .map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
pub fn allowed_path(state: &AppState, id: &str) -> Result<PathBuf> {
    let (folder, path, _, expected) =
        { state.catalog.lock().map_err(|_| "目录忙碌")?.location(id)? };
    let root = state
        .allowed
        .lock()
        .map_err(|_| "目录忙碌")?
        .get(&folder)
        .cloned()
        .ok_or("请先通过选择器打开此目录")?;
    let original = PathBuf::from(path);
    let actual = original
        .canonicalize()
        .map_err(|_| "照片已移动或离线，请重新扫描")?;
    if !actual.starts_with(root) || identity(&original)? != expected {
        return Err("照片身份已变化，请重新扫描".into());
    }
    Ok(actual)
}

pub fn render(state: &AppState, id: &str, size: u32) -> Result<Vec<u8>> {
    render_with_refresh(state, id, size, false)
}
pub fn render_with_refresh(
    state: &AppState,
    id: &str,
    size: u32,
    refresh: bool,
) -> Result<Vec<u8>> {
    if ![480, 1600, 8192].contains(&size) {
        return Err("预览尺寸无效".into());
    }
    let source = allowed_path(state, id)?;
    let before = fingerprint(&source)?;
    if fs::metadata(&source).map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
        return Err("此文件超过 Spike 单文件限制".into());
    }
    let cache = state
        .data
        .join("cache")
        .join(format!("sdr-v3-{id}-{before}-{size}.jpg"));
    // Explicit retry repairs a damaged cache without weakening grant/identity checks.
    if !refresh && let Ok(bytes) = fs::read(&cache) {
        return Ok(bytes);
    }
    #[cfg(target_os = "macos")]
    let bytes = macos::preview(&source, size)?;
    #[cfg(not(target_os = "macos"))]
    let bytes: Vec<u8> = (|| -> Result<Vec<u8>> {
        let file = fs::File::open(&source).map_err(|_| "无法打开照片")?;
        let mut reader = ImageReader::new(BufReader::new(file))
            .with_guessed_format()
            .map_err(|_| "无法识别图片格式")?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256 * 1024 * 1024);
        limits.max_image_width = Some(30000);
        limits.max_image_height = Some(30000);
        reader.limits(limits);
        let mut decoder = reader
            .into_decoder()
            .map_err(|_| "图片损坏或格式尚不支持")?;
        let orientation = decoder.orientation().map_err(|_| "方向信息损坏")?;
        let mut image =
            DynamicImage::from_decoder(decoder).map_err(|_| "图片解码失败或超过内存限制")?;
        image.apply_orientation(orientation);
        // A single active detail pane can request up to 8192px; other panes stay at 1600px.
        let image = image
            .thumbnail(size.min(image.width()), size.min(image.height()))
            .to_rgb8();
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, 88)
            .encode_image(&image)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })()?;
    if fingerprint(&source)? != before {
        return Err("解码过程中照片发生变化，请重试".into());
    }
    let write_temp = cache.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&write_temp, &bytes).map_err(|e| e.to_string())?;
    fs::rename(write_temp, &cache).map_err(|e| e.to_string())?;
    prune_cache(&state.data.join("cache"), 256 * 1024 * 1024)?;
    Ok(bytes)
}
pub fn prune_cache(path: &Path, budget: u64) -> Result<()> {
    let mut entries = fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            if e.path().extension().and_then(|s| s.to_str()) != Some("jpg") {
                return None;
            }
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((e.path(), m.len(), m.modified().ok()))
        })
        .collect::<Vec<_>>();
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    entries.sort_by_key(|e| e.2);
    for (file, len, _) in entries {
        if total <= budget {
            break;
        }
        if fs::remove_file(file).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}
