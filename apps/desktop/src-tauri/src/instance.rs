//! One writer per canonical application-data directory, including independently
//! launched copies of the binary. The OS releases the lock after a crash. Keep
//! the lock file in place: unlinking it would allow a second lock on a new inode.
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub struct Owner {
    _lock: File,
    wake: PathBuf,
    seen: String,
}

pub enum Claim {
    Owner(Owner),
    Forwarded,
}

pub fn claim(data: &Path) -> io::Result<Claim> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data.join("instance.lock"))?;
    let wake = data.join("instance-wake");
    match lock.try_lock() {
        Ok(()) => Ok(Claim::Owner(Owner {
            _lock: lock,
            wake,
            seen: String::new(),
        })),
        Err(TryLockError::WouldBlock) => {
            // Fixed-size messages contain no paths, arguments, or media data.
            // Multiple simultaneous launches may coalesce into one activation.
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(wake)?;
            file.write_all(uuid::Uuid::new_v4().to_string().as_bytes())?;
            Ok(Claim::Forwarded)
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

impl Owner {
    pub fn activation_requested(&mut self) -> io::Result<bool> {
        let file = match File::open(&self.wake) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let mut request = String::new();
        file.take(37).read_to_string(&mut request)?;
        if uuid::Uuid::parse_str(&request).is_ok() && request != self.seen {
            self.seen = request;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Public installations keep their existing catalog. Development and local QA
/// use stable, separate directories across builds; explicit QA overrides must
/// be absolute and never silently fall back to the daily catalog.
pub fn data_directory(
    default: &Path,
    channel: &str,
    override_path: Option<PathBuf>,
) -> io::Result<PathBuf> {
    let path = if let Some(path) = override_path {
        if !path.is_absolute() {
            return Err(io::Error::other(
                "测试资料目录必须是绝对路径，未打开日常资料库",
            ));
        }
        path
    } else if channel == "public-prerelease" {
        default.to_path_buf()
    } else {
        // app_data_dir is already scoped by the bundle ID. Keep a separate
        // suffix even for direct developer binaries using the public ID.
        default.with_extension(if channel == "local-trial" {
            "trial"
        } else {
            "development"
        })
    };
    fs::create_dir_all(&path)?;
    path.canonicalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    #[ignore = "child-process helper; run by instance ownership test"]
    fn child_claim() {
        let data = std::env::var_os("GROVE_INSTANCE_TEST_DIR").unwrap();
        if std::env::var_os("GROVE_INSTANCE_HOLD").is_some() {
            let Claim::Owner(_owner) = claim(Path::new(&data)).unwrap() else {
                panic!()
            };
            fs::write(Path::new(&data).join("ready"), b"ready").unwrap();
            loop {
                std::thread::park();
            }
        }
        let outcome = claim(Path::new(&data)).unwrap();
        assert!(matches!(outcome, Claim::Forwarded));
    }

    #[test]
    fn processes_forward_without_opening_catalog_and_crash_safe_lock_releases() {
        let t = tempfile::tempdir().unwrap();
        let Claim::Owner(mut owner) = claim(t.path()).unwrap() else {
            panic!()
        };
        assert!(!owner.activation_requested().unwrap());
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::tests::child_claim", "--ignored"])
            .env("GROVE_INSTANCE_TEST_DIR", t.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(owner.activation_requested().unwrap());
        assert!(!owner.activation_requested().unwrap());
        assert!(!t.path().join("catalog.sqlite").exists());
        // A distinct data directory can own an independent instance.
        let other = tempfile::tempdir().unwrap();
        assert!(matches!(claim(other.path()).unwrap(), Claim::Owner(_)));
        drop(owner);
        assert!(matches!(claim(t.path()).unwrap(), Claim::Owner(_)));
    }

    #[cfg(unix)]
    #[test]
    fn path_aliases_share_ownership() {
        let t = tempfile::tempdir().unwrap();
        let real = data_directory(&t.path().join("real"), "public-prerelease", None).unwrap();
        let alias = t.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let alias = data_directory(&alias, "public-prerelease", None).unwrap();
        assert_eq!(alias, real);
        let Claim::Owner(_owner) = claim(&real).unwrap() else {
            panic!()
        };
        assert!(matches!(claim(&alias).unwrap(), Claim::Forwarded));
    }

    #[test]
    fn terminated_process_releases_lock_without_deleting_marker() {
        let t = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::tests::child_claim", "--ignored"])
            .env("GROVE_INSTANCE_TEST_DIR", t.path())
            .env("GROVE_INSTANCE_HOLD", "1")
            .spawn()
            .unwrap();
        let mut ready = false;
        for _ in 0..100 {
            if t.path().join("ready").exists() {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Always reap the owned child, even when readiness fails.
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(ready);
        assert!(t.path().join("instance.lock").is_file());
        assert!(matches!(claim(t.path()).unwrap(), Claim::Owner(_)));
    }

    #[test]
    fn catalog_scope_preserves_daily_and_rejects_invalid_override() {
        let t = tempfile::tempdir().unwrap();
        let daily = t.path().join("app.momentgrove.desktop");
        let public = data_directory(&daily, "public-prerelease", None).unwrap();
        let qa = data_directory(&daily, "local-trial", None).unwrap();
        let dev = data_directory(&daily, "development", None).unwrap();
        assert_eq!(public, daily.canonicalize().unwrap());
        assert_ne!(qa, public);
        assert_ne!(dev, public);
        assert_ne!(dev, qa);
        assert_eq!(qa, data_directory(&daily, "local-trial", None).unwrap());
        assert!(data_directory(&daily, "public-prerelease", Some(PathBuf::new())).is_err());
        assert!(data_directory(&daily, "public-prerelease", Some("relative".into())).is_err());
        assert_eq!(
            public,
            data_directory(&daily, "local-trial", Some(public.clone())).unwrap()
        );
    }
}
