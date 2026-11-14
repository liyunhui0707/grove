//! All sidecar I/O is relative to a pinned, authorized parent directory.
use crate::catalog::Result;
use std::path::Path;
pub const MAX_XML: usize = 1_048_576;
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "linux"));
pub fn default_target(source: &Path) -> String {
    format!(
        "{}.xmp",
        source.file_name().unwrap_or_default().to_string_lossy()
    )
}
#[cfg(unix)]
fn leaf(name: &str) -> Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || !matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err("XMP 文件名无效".into());
    }
    Ok(())
}

#[cfg(unix)]
mod platform {
    use super::*;
    use crate::imaging;
    use rustix::fs::{self, Mode, OFlags};
    use std::{
        fs::File,
        io::{Read, Write},
        path::Component,
    };
    pub struct Files {
        dir: File,
        source: String,
    }
    impl Files {
        pub fn open(root: &Path, source: &Path, identity: &str) -> Result<Self> {
            let relative = source
                .strip_prefix(root)
                .map_err(|_| "XMP 目标不在授权目录内")?;
            let mut parts: Vec<_> = relative.components().collect();
            let Some(Component::Normal(name)) = parts.pop() else {
                return Err("源文件名无效".into());
            };
            let name = name.to_str().ok_or("暂不支持此文件名编码")?.to_string();
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let mut dir: File = fs::open(root, flags, Mode::empty())
                .map_err(|e| e.to_string())?
                .into();
            for part in parts {
                let Component::Normal(part) = part else {
                    return Err("源文件路径无效".into());
                };
                dir = fs::openat(&dir, part, flags, Mode::empty())
                    .map_err(|_| "照片父目录不可读或包含链接")?
                    .into();
            }
            let original: File = fs::openat(
                &dir,
                &name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| "照片已离线或被替换")?
            .into();
            let meta = original.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() || imaging::identity_from_metadata(&meta)? != identity {
                return Err("照片身份已变化，请重新打开目录".into());
            }
            Ok(Self { dir, source: name })
        }
        pub fn read(&self, name: &str) -> Result<Option<String>> {
            leaf(name)?;
            let fd = match fs::openat(
                &self.dir,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::NOENT) => return Ok(None),
                Err(_) => return Err("XMP 不可读或为符号链接".into()),
            };
            let file: File = fd.into();
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.len() > MAX_XML as u64 {
                return Err("XMP 非普通文件或超过 1MiB 上限".into());
            }
            let mut bytes = Vec::new();
            file.take(MAX_XML as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > MAX_XML {
                return Err("XMP 超过 1MiB 上限".into());
            }
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "暂不支持此 XMP 文本编码".into())
        }
        pub fn regular(&self, name: &str) -> Result<bool> {
            leaf(name)?;
            match fs::statat(&self.dir, name, fs::AtFlags::SYMLINK_NOFOLLOW) {
                Ok(meta) => {
                    Ok(fs::FileType::from_raw_mode(meta.st_mode) == fs::FileType::RegularFile)
                }
                Err(rustix::io::Errno::NOENT) => Ok(false),
                Err(e) => Err(e.to_string()),
            }
        }
        pub fn resolve(&self) -> Result<(String, Option<String>)> {
            let full = format!("{}.xmp", self.source);
            let stem = Path::new(&self.source)
                .with_extension("xmp")
                .to_string_lossy()
                .into_owned();
            let full_xml = self.read(&full)?;
            let stem_xml = self.read(&stem)?;
            if full_xml.is_some() && stem_xml.is_some() {
                return Err("同时存在完整文件名和同名 XMP，需先消除歧义".into());
            }
            if let Some(xml) = stem_xml {
                self.stem_unique()?;
                Ok((stem, Some(xml)))
            } else {
                Ok((full, full_xml))
            }
        }
        fn stem_unique(&self) -> Result<()> {
            let dir = fs::Dir::read_from(&self.dir).map_err(|e| e.to_string())?;
            for entry in dir {
                let entry = entry.map_err(|e| e.to_string())?;
                let name = entry.file_name().to_string_lossy();
                let path = Path::new(name.as_ref());
                if name != self.source
                    && path.file_stem() == Path::new(&self.source).file_stem()
                    && path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        [
                            "jpg", "jpeg", "png", "webp", "heic", "heif", "dng", "cr2", "cr3",
                            "nef", "arw", "raf", "rw2", "orf", "mp4", "mov",
                        ]
                        .contains(&e.to_ascii_lowercase().as_str())
                    })
                {
                    return Err("同名媒体共享 XMP，暂不自动选择归属".into());
                }
            }
            Ok(())
        }
        pub fn check_target(&self, target: &str) -> Result<()> {
            let full = format!("{}.xmp", self.source);
            let stem = Path::new(&self.source)
                .with_extension("xmp")
                .to_string_lossy()
                .into_owned();
            if target != full && target != stem {
                return Err("XMP 目标文件名无效".into());
            }
            let other = if target == full { &stem } else { &full };
            if self.read(other)?.is_some() {
                return Err("出现另一种命名的 XMP，需先消除歧义".into());
            }
            if target == stem {
                self.stem_unique()?;
            }
            Ok(())
        }
        pub fn stage(&self, name: &str, xml: &str) -> Result<()> {
            if let Some(existing) = self.read(name)? {
                return if existing == xml {
                    Ok(())
                } else {
                    Err("暂存文件已变化，已保留现场".into())
                };
            }
            let writing = format!(".moment-grove-{}.writing.xmp", uuid::Uuid::new_v4());
            let mut file: File = fs::openat(
                &self.dir,
                &writing,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o600),
            )
            .map_err(|e| e.to_string())?
            .into();
            file.write_all(xml.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            self.rename(&writing, name)?;
            self.sync()
        }
        pub fn rename(&self, from: &str, to: &str) -> Result<()> {
            leaf(from)?;
            leaf(to)?;
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                fs::renameat_with(&self.dir, from, &self.dir, to, fs::RenameFlags::NOREPLACE)
                    .map_err(|e| format!("未覆盖目标文件：{e}"))
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                Err("此平台尚未启用安全 XMP 写回".into())
            }
        }
        pub fn sync(&self) -> Result<()> {
            self.dir.sync_all().map_err(|e| e.to_string())
        }
    }
}
#[cfg(not(unix))]
mod platform {
    use super::*;
    pub struct Files;
    impl Files {
        pub fn open(_: &Path, _: &Path, _: &str) -> Result<Self> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn read(&self, _: &str) -> Result<Option<String>> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn regular(&self, _: &str) -> Result<bool> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn resolve(&self) -> Result<(String, Option<String>)> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn check_target(&self, _: &str) -> Result<()> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn stage(&self, _: &str, _: &str) -> Result<()> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn rename(&self, _: &str, _: &str) -> Result<()> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
        pub fn sync(&self) -> Result<()> {
            Err("此平台尚未启用安全 XMP 同步".into())
        }
    }
}
pub use platform::Files;
