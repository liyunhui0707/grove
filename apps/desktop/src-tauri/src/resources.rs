//! Conservative resource recognition; never infer that a whole Live Photo is supported.
use crate::catalog::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::Path};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Companions {
    pub videos: usize,
    pub raw: usize,
    pub live_photo: bool,
}
impl Companions {
    pub fn any(&self) -> bool {
        self.videos != 0 || self.raw != 0 || self.live_photo
    }
}
fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}
pub fn directory(path: &Path) -> Result<HashMap<String, Companions>> {
    let mut result = HashMap::<String, Companions>::new();
    for entry in fs::read_dir(path).map_err(|_| "无法核对关联资源，请恢复目录或重新授权")?
    {
        let entry = entry.map_err(|_| "部分关联资源不可读，未执行文件操作")?;
        let kind = entry
            .file_type()
            .map_err(|_| "关联资源已变化，请重新扫描")?;
        if !kind.is_file() && !kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let video = ["mov", "mp4", "m4v", "avi", "mkv", "webm", "3gp"].contains(&ext.as_str());
        let raw = [
            "dng", "cr2", "cr3", "nef", "arw", "raf", "rw2", "orf", "pef", "srw",
        ]
        .contains(&ext.as_str());
        if video || raw {
            let group = result.entry(stem(&path)).or_default();
            group.videos += usize::from(video);
            group.raw += usize::from(raw);
        }
    }
    Ok(result)
}
pub fn for_photo(index: &HashMap<String, Companions>, path: &Path) -> Companions {
    let mut group = index.get(&stem(path)).cloned().unwrap_or_default();
    group.live_photo = crate::imaging::has_live_photo_marker(path);
    group
}
pub fn refuse_incomplete_original_operation(path: &Path, action: &str) -> Result<()> {
    let group = for_photo(&directory(path.parent().ok_or("照片路径无效")?)?, path);
    if group.any() {
        return Err(format!(
            "发现Live Photo标识或同名视频/RAW关联资源，暂不拆分{action}；可生成独立静态水印副本，关联资源保持原位"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sibling_index_is_case_insensitive_and_never_crosses_directories() {
        let t = tempfile::tempdir().unwrap();
        for name in [
            "IMG_1.MOV",
            "img_1.PEF",
            "img_1.SRW",
            "other.mp4",
            "img_10.mov",
        ] {
            fs::write(t.path().join(name), b"generated association marker").unwrap();
        }
        fs::create_dir(t.path().join("nested")).unwrap();
        fs::write(t.path().join("nested/img_1.mov"), b"different directory").unwrap();
        let index = directory(t.path()).unwrap();
        let group = for_photo(&index, &t.path().join("Img_1.JPG"));
        assert_eq!((group.videos, group.raw), (1, 2));
        assert!(group.any());
        assert!(!for_photo(&index, &t.path().join("img_2.jpg")).any());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Explicit generated Apple-marker JPEG from owned MG_PHONE_FIXTURES"]
    fn live_marker_blocks_incomplete_originals_even_without_a_same_name_video() {
        use crate::{exports, grant, imaging, moves, safety_tests::state};
        let input = std::path::PathBuf::from(std::env::var_os("MG_PHONE_FIXTURES").unwrap());
        assert!(input.starts_with("/private/tmp"));
        assert!(
            input
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("moment-grove-phones-080-")
        );
        let bytes = fs::read(input.join("generated-live-marker.jpg")).unwrap();
        assert_eq!(
            exports::files::hash_bytes(&bytes),
            std::env::var("MG_LIVE_MARKER_SHA256").unwrap()
        );
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let source = root.join("source");
        let out = root.join("output");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&out).unwrap();
        let photo = source.join("static.jpg");
        fs::write(&photo, &bytes).unwrap();
        fs::write(
            source.join("renamed-motion.MOV"),
            b"generated renamed companion marker",
        )
        .unwrap();
        let group = for_photo(&directory(&source).unwrap(), &photo);
        assert_eq!(group.videos, 0);
        assert!(group.live_photo);
        let s = state(&root.join("runtime"));
        let f = grant(&s, source.clone()).unwrap();
        let mut c = s.catalog.lock().unwrap();
        let id = c
            .upsert(f.id, &photo, &imaging::identity(&photo).unwrap(), None)
            .unwrap()
            .0;
        c.rate(std::slice::from_ref(&id), Some(4)).unwrap();
        let roots = s.allowed.lock().unwrap().clone();
        let copy = exports::Target {
            id: id.clone(),
            revision: 1,
        };
        let mov = moves::Target { id, revision: 1 };
        assert!(exports::prepare(&c, &roots, f.id, &[copy], &out, &s.data).is_err());
        assert!(moves::prepare(&c, &roots, f.id, &[mov], &out, &s.data, "move").is_err());
        assert_eq!(fs::read(photo).unwrap(), bytes);
        assert_eq!(fs::read_dir(out).unwrap().count(), 0);
        println!(
            "LIVE_MARKER_GUARD {{\"generatedFixture\":true,\"renamedOrAbsentVideoProtected\":true,\"originalCopyRefused\":true,\"moveRefused\":true}}"
        );
    }
}
