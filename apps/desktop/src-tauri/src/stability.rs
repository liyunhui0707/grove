//! Recovery regressions use owned generated fixtures only.
use crate::{ScanEvent, grant, imaging, safety_tests::state, scan_directory};
use std::{
    fs,
    sync::{Arc, Mutex},
};
use tauri::ipc::Channel;

fn events() -> (Channel<ScanEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let values = Arc::new(Mutex::new(Vec::new()));
    let recorded = values.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            recorded
                .lock()
                .unwrap()
                .push(serde_json::from_str(&text).unwrap());
        }
        Ok(())
    });
    (channel, values)
}

#[test]
fn unavailable_scan_root_is_an_error_not_an_empty_success() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let s = state(&base.join("runtime"));
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    let file = source.join("generated.png");
    image::RgbImage::new(48, 32).save(&file).unwrap();
    let folder = grant(&s, source.clone()).unwrap();
    let (channel, _) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    let before: String = s
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT id FROM assets", [], |r| r.get(0))
        .unwrap();
    s.catalog
        .lock()
        .unwrap()
        .rate(std::slice::from_ref(&before), Some(4))
        .unwrap();
    fs::rename(&source, base.join("offline-sources")).unwrap();
    let (channel, recorded) = events();
    assert!(scan_directory(&s, &source, folder.id, 0, &channel).is_err());
    assert!(!recorded.lock().unwrap().iter().any(|e| e["kind"] == "done"));
    let retained: (String, i32) = s
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT id,rating FROM assets", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(retained, (before, 4));
    fs::write(&source, b"not a directory").unwrap();
    assert!(scan_directory(&s, &source, folder.id, 0, &channel).is_err());
}

#[cfg(unix)]
#[test]
fn permission_denied_root_fails_but_unreadable_child_keeps_partial_results() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let s = state(&base.join("runtime"));
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    image::RgbImage::new(48, 32)
        .save(source.join("visible.png"))
        .unwrap();
    let child = source.join("unreadable");
    fs::create_dir(&child).unwrap();
    image::RgbImage::new(48, 32)
        .save(child.join("hidden.png"))
        .unwrap();
    let folder = grant(&s, source.clone()).unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
    let denied = fs::read_dir(&source).is_err();
    let (channel, recorded) = events();
    let result = scan_directory(&s, &source, folder.id, 0, &channel);
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        denied,
        "This regression must run as an ordinary non-root user"
    );
    assert!(result.is_err());
    assert!(recorded.lock().unwrap().is_empty());
    fs::set_permissions(&child, fs::Permissions::from_mode(0o000)).unwrap();
    let (channel, recorded) = events();
    let result = scan_directory(&s, &source, folder.id, 0, &channel);
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    result.unwrap();
    let recorded = recorded.lock().unwrap();
    let done = recorded.iter().find(|e| e["kind"] == "done").unwrap();
    assert_eq!(done["count"], 1);
    assert!(!done["warnings"].as_array().unwrap().is_empty());
    assert_eq!(recorded.iter().filter(|e| e["kind"] == "batch").count(), 1);
}

#[test]
fn empty_readable_folder_completes_without_false_permission_warning() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let s = state(&base.join("runtime"));
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    let folder = grant(&s, source.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    assert_eq!(
        *recorded.lock().unwrap(),
        vec![serde_json::json!({
            "kind": "done", "count": 0, "warnings": [],
        })]
    );
}

#[test]
fn explicit_preview_retry_replaces_bad_cache_without_touching_media_or_ratings() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let s = state(&base.join("runtime"));
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    let file = source.join("generated.png");
    image::RgbImage::new(48, 32).save(&file).unwrap();
    let original = fs::read(&file).unwrap();
    let folder = grant(&s, source.clone()).unwrap();
    let id = s
        .catalog
        .lock()
        .unwrap()
        .upsert(folder.id, &file, &imaging::identity(&file).unwrap(), None)
        .unwrap()
        .0;
    s.catalog
        .lock()
        .unwrap()
        .rate(std::slice::from_ref(&id), Some(4))
        .unwrap();
    let good = imaging::render(&s, &id, 480).unwrap();
    let cache = fs::read_dir(s.data.join("cache"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&cache, b"generated corrupt cache").unwrap();
    assert!(image::load_from_memory(&imaging::render(&s, &id, 480).unwrap()).is_err());
    let repaired = imaging::render_with_refresh(&s, &id, 480, true).unwrap();
    assert!(image::load_from_memory(&repaired).is_ok());
    assert_eq!(repaired, good);
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(fs::read(&cache).unwrap(), repaired);
    let c = s.catalog.lock().unwrap();
    let rating: i32 =
        c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&id], |r| r.get(0))
            .unwrap();
    assert_eq!(rating, 4);
    let pending: i64 =
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
            .unwrap();
    assert_eq!(pending, 1);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Requires a manifest-verified, attributed CC BY-SA 3.0 private QA fixture on Air"]
fn licensed_camera_photo_and_sdr_conversions_preserve_sources() {
    use std::process::Command;
    let sample = std::env::var_os("MG_LICENSED_SDR").expect("Owned QA fixture path is required");
    let input = std::path::Path::new(&sample);
    let original = fs::read(input).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("照片 样本");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("01-camera.jpg"), &original).unwrap();
    let convert = |args: &[&str], output: &std::path::Path| {
        let result = Command::new("/usr/bin/sips")
            .args(args)
            .arg(input)
            .arg("--out")
            .arg(output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    convert(&["-s", "format", "png"], &source.join("02-converted.png"));
    image::open(input)
        .unwrap()
        .save(source.join("03-converted.webp"))
        .unwrap();
    convert(&["-s", "format", "heic"], &source.join("04-converted.heic"));
    convert(
        &[
            "-s",
            "format",
            "png",
            "-m",
            "/System/Library/ColorSync/Profiles/Display P3.icc",
        ],
        &source.join("05-converted-p3.png"),
    );
    image::open(input)
        .unwrap()
        .to_luma8()
        .save(source.join("06-converted-gray.png"))
        .unwrap();
    let before: Vec<_> = fs::read_dir(&source)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    let s = state(&base.join("runtime"));
    let folder = grant(&s, source.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    let recorded = recorded.lock().unwrap();
    let assets: Vec<_> = recorded
        .iter()
        .filter(|e| e["kind"] == "batch")
        .flat_map(|e| e["items"].as_array().unwrap())
        .collect();
    assert_eq!(assets.len(), 6);
    let camera = assets
        .iter()
        .find(|a| a["name"] == "01-camera.jpg")
        .unwrap();
    assert!(
        camera["captured"].is_string(),
        "Camera EXIF date must remain visible"
    );
    let mut previews = 0;
    for asset in &assets {
        for size in [480, 1600, 8192] {
            let bytes = imaging::render(&s, asset["id"].as_str().unwrap(), size).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert!((i64::from(decoded.width()) * 2 - i64::from(decoded.height()) * 3).abs() <= 3);
            assert!(decoded.width() <= size && decoded.width() <= 3072);
            assert_eq!(
                imaging::render(&s, asset["id"].as_str().unwrap(), size).unwrap(),
                bytes
            );
            previews += 1;
        }
    }
    for (p, bytes) in &before {
        assert_eq!(&fs::read(p).unwrap(), bytes);
    }
    assert_eq!(fs::read(input).unwrap(), original);
    assert_eq!(
        s.catalog
            .lock()
            .unwrap()
            .db
            .query_row("SELECT count(*) FROM pending_xmp", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    println!(
        "LICENSED_SDR {{\"sources\":6,\"previews\":{previews},\"warm_reads\":{previews},\"originals_unchanged\":true,\"camera_exif_date\":true,\"note\":\"one camera JPEG plus five conversions; not six camera encoders\"}}"
    );
}
