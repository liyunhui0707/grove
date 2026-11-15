//! Presentation preferences only. Batch dates, cities and destinations must
//! never leak into a new export through this record.
use crate::{catalog::Result, watermark::Options};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Style {
    font: String,
    size_percent: f64,
    color: String,
    opacity: f64,
    margin_percent: f64,
    shadow: bool,
}
impl Style {
    fn validate(&self) -> Result<()> {
        Options {
            font: self.font.clone(),
            size_percent: self.size_percent,
            color: self.color.clone(),
            opacity: self.opacity,
            margin_percent: self.margin_percent,
            shadow: self.shadow,
            ..Default::default()
        }
        .normalized()
        .map(|_| ())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    style: Style,
}

fn check_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 4096 => Ok(true),
        Ok(_) => Err("水印样式记录不是有效的普通文件；未读取或覆盖".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("无法访问已保存的水印样式".into()),
    }
}

pub fn load(data: &Path) -> Result<Option<Style>> {
    let path = data.join("watermark-style.json");
    if !check_file(&path)? {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "无法读取水印样式")?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取水印样式")?;
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|_| "水印样式记录损坏或版本不支持；本次使用默认样式")?;
    if record.version != 1 {
        return Err("水印样式版本不支持；本次使用默认样式".into());
    }
    record.style.validate()?;
    Ok(Some(record.style))
}

pub fn save(data: &Path, style: Style) -> Result<()> {
    style.validate()?;
    let path = data.join("watermark-style.json");
    check_file(&path)?;
    let bytes = serde_json::to_vec(&Record { version: 1, style }).map_err(|e| e.to_string())?;
    let temp = data.join(format!("watermark-style-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|_| "无法保存水印样式；本次仍可预览和导出")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "无法保存水印样式")?;
    fs::rename(temp, path).map_err(|_| "无法保存水印样式")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn style() -> Style {
        serde_json::from_value(serde_json::json!({
            "font":"serif","sizePercent":3.1,"color":"#fedcba",
            "opacity":0.65,"marginPercent":4.2,"shadow":false
        }))
        .unwrap()
    }

    #[test]
    fn restart_and_isolation_preserve_only_valid_style() {
        let t = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert_eq!(load(t.path()).unwrap(), None);
        save(t.path(), style()).unwrap();
        assert_eq!(load(t.path()).unwrap(), Some(style()));
        assert_eq!(load(other.path()).unwrap(), None);
        let raw = fs::read_to_string(t.path().join("watermark-style.json")).unwrap();
        let record: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for excluded in ["city", "fallbackDate", "destination", "rating"] {
            assert!(!record["style"].as_object().unwrap().contains_key(excluded));
        }
        let before = raw.as_bytes();
        let mut invalid = style();
        invalid.opacity = f64::NAN;
        assert!(save(t.path(), invalid).is_err());
        assert_eq!(
            fs::read(t.path().join("watermark-style.json")).unwrap(),
            before
        );
        let mut extra = serde_json::to_value(style()).unwrap();
        extra["city"] = "unwanted".into();
        assert!(serde_json::from_value::<Style>(extra).is_err());
    }

    #[test]
    fn corrupt_oversized_and_unknown_versions_do_not_overwrite_on_load() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("watermark-style.json");
        for bytes in [
            b"broken".to_vec(),
            vec![b' '; 4097],
            serde_json::to_vec(&Record {
                version: 99,
                style: style(),
            })
            .unwrap(),
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(load(t.path()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_and_unwritable_directories_are_not_silent_success() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let t = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let target = other.path().join("untouched");
        fs::write(&target, b"keep").unwrap();
        symlink(&target, t.path().join("watermark-style.json")).unwrap();
        assert!(load(t.path()).is_err());
        assert!(save(t.path(), style()).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
        fs::set_permissions(other.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = save(other.path(), style());
        fs::set_permissions(other.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
    }
}
