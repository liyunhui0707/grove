//! Pinned directory handles; no overwrite or automatic removal of any output.
use crate::{catalog::Result, imaging};
use rustix::fs::{self, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Dir {
    pub file: File,
    pub path: PathBuf,
}
fn leaf(name: &str) -> Result<()> {
    if name.is_empty()
        || !matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
        || Path::new(name).components().count() != 1
    {
        return Err("导出文件名无效".into());
    }
    Ok(())
}
impl Dir {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err("目录必须为完整路径".into());
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut file: File = fs::open("/", flags, Mode::empty())
            .map_err(|e| e.to_string())?
            .into();
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(n) => {
                    file = fs::openat(&file, n, flags, Mode::empty())
                        .map_err(|_| "目录不可读、已变化或包含链接")?
                        .into()
                }
                _ => return Err("目录路径无效".into()),
            }
        }
        Ok(Self {
            file,
            path: path.into(),
        })
    }
    pub fn key(&self) -> Result<String> {
        imaging::identity_from_file(&self.file)
    }
    pub fn unchanged(&self) -> Result<()> {
        if Self::open(&self.path)?.key()? != self.key()? {
            return Err("导出目录已被替换，保留现场".into());
        }
        Ok(())
    }
    pub fn create(&self, name: &str) -> Result<Self> {
        leaf(name)?;
        self.unchanged()?;
        fs::mkdirat(&self.file, name, Mode::from_bits_truncate(0o700))
            .map_err(|e| format!("未创建或覆盖既有目录：{e}"))?;
        self.sync()?;
        let file: File = fs::openat(
            &self.file,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| e.to_string())?
        .into();
        let result = Self {
            file,
            path: self.path.join(name),
        };
        result.unchanged()?;
        Ok(result)
    }
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all().map_err(|e| e.to_string())
    }
    pub fn available(&self) -> Result<u64> {
        let s = fs::fstatvfs(&self.file).map_err(|e| e.to_string())?;
        Ok(s.f_bavail.saturating_mul(s.f_frsize))
    }
    pub fn read(&self, name: &str) -> Result<Option<File>> {
        leaf(name)?;
        match fs::openat(
            &self.file,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => {
                let f: File = fd.into();
                if !f.metadata().map_err(|e| e.to_string())?.is_file() {
                    return Err("目标不是普通文件".into());
                }
                Ok(Some(f))
            }
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(e) => Err(format!("目标不可读或为链接：{e}")),
        }
    }
    fn new_file(&self, name: &str) -> Result<File> {
        leaf(name)?;
        fs::openat(
            &self.file,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map(File::from)
        .map_err(|e| e.to_string())
    }
    pub fn publish(
        &self,
        name: &str,
        mut source: impl Read,
        expected: &str,
        cancel: &AtomicBool,
    ) -> Result<()> {
        self.unchanged()?;
        leaf(name)?;
        if let Some(file) = self.read(name)? {
            if digest(file, cancel)?.0 == expected {
                return Ok(());
            }
            return Err("同名目标内容不同，已保留且未覆盖".into());
        }
        let partial = format!(".{}.partial", uuid::Uuid::new_v4());
        let mut out = self.new_file(&partial)?;
        let mut hash = Sha256::new();
        let mut buf = [0u8; 256 * 1024];
        loop {
            cancelled(cancel)?;
            let n = source.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            hash.update(&buf[..n]);
        }
        out.sync_all().map_err(|e| e.to_string())?;
        if format!("{:x}", hash.finalize()) != expected {
            return Err("源内容已变化，部分文件保留，未发布".into());
        }
        let staged = self.read(&partial)?.ok_or("暂存副本丢失")?;
        if digest(staged, cancel)?.0 != expected {
            return Err("暂存副本内容核验失败".into());
        }
        self.unchanged()?;
        cancelled(cancel)?;
        fs::renameat_with(
            &self.file,
            &partial,
            &self.file,
            name,
            fs::RenameFlags::NOREPLACE,
        )
        .map_err(|e| format!("发布失败且未覆盖目标：{e}"))?;
        self.sync()?;
        if digest(self.read(name)?.ok_or("已发布副本丢失")?, cancel)?.0 != expected {
            return Err("已发布副本回读不一致".into());
        }
        Ok(())
    }
    pub fn write_record(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.publish(name, bytes, &hash_bytes(bytes), &AtomicBool::new(false))
    }
    pub fn lock(&self) -> Result<File> {
        let fd = fs::openat(
            &self.file,
            "operation.lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| e.to_string())?;
        let file: File = fd.into();
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("操作锁不可用".into());
        }
        fs::flock(&file, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| "该导出仍在其他进程执行")?;
        Ok(file)
    }
}
pub fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err("已取消，已核验副本和部分文件保留".into())
    } else {
        Ok(())
    }
}
pub fn hash_bytes(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn digest(mut source: impl Read, cancel: &AtomicBool) -> Result<(String, u64)> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buf = [0u8; 256 * 1024];
    loop {
        cancelled(cancel)?;
        let n = source.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buf[..n]);
    }
    Ok((format!("{:x}", hash.finalize()), count))
}
pub fn source(root: &Path, path: &Path, key: &str) -> Result<File> {
    let rel = path.strip_prefix(root).map_err(|_| "照片不在授权目录")?;
    let parent = path.parent().ok_or("照片路径无效")?;
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("照片路径无效".into());
    }
    let dir = Dir::open(parent)?;
    let f = dir
        .read(
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("文件名无效")?,
        )?
        .ok_or("照片已离线")?;
    if !imaging::matches_file_identity(&f, key)? {
        return Err("照片身份已变化".into());
    }
    Ok(f)
}
