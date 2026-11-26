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

#[cfg(target_os = "macos")]
#[test]
fn volume_identity_migrates_legacy_catalog_without_changing_rating_or_id() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    let file = source.join("legacy.png");
    image::RgbImage::new(48, 32).save(&file).unwrap();
    let s = state(&base.join("runtime"));
    let f = grant(&s, source.clone()).unwrap();
    let legacy = imaging::identity_from_metadata(&fs::metadata(&file).unwrap()).unwrap();
    let id = s
        .catalog
        .lock()
        .unwrap()
        .upsert(f.id, &file, &legacy, None)
        .unwrap()
        .0;
    s.catalog
        .lock()
        .unwrap()
        .rate(std::slice::from_ref(&id), Some(5))
        .unwrap();
    let (channel, _) = events();
    scan_directory(&s, &source, f.id, 0, &channel).unwrap();
    let c = s.catalog.lock().unwrap();
    let (new_id, key, rating, revision): (String, String, i32, i64) =
        c.db.query_row("SELECT id,identity,rating,revision FROM assets", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert!(
        key.starts_with("volume:"),
        "Device number alone is not a persistent volume identity"
    );
    assert_eq!((new_id, rating, revision), (id, 5, 1));
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn rescan_rejects_replaced_root_without_cataloguing_the_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    image::RgbImage::new(48, 32)
        .save(source.join("old.png"))
        .unwrap();
    let s = state(&base.join("runtime"));
    let folder = grant(&s, source.clone()).unwrap();
    let (channel, _) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    fs::rename(&source, base.join("retained-source")).unwrap();
    fs::create_dir(&source).unwrap();
    image::RgbImage::new(48, 32)
        .save(source.join("replacement.png"))
        .unwrap();
    let (channel, recorded) = events();
    assert!(scan_directory(&s, &source, folder.id, 0, &channel).is_err());
    assert!(recorded.lock().unwrap().is_empty());
    let count: i64 = s
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT count(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn root_disconnected_mid_scan_is_incomplete_and_reconnect_retains_ratings() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("sources");
    let retained = base.join("retained-source");
    fs::create_dir(&source).unwrap();
    for i in 0..130 {
        image::RgbImage::new(48, 32)
            .save(source.join(format!("{i:03}.png")))
            .unwrap();
    }
    let s = state(&base.join("runtime"));
    let folder = grant(&s, source.clone()).unwrap();
    let disconnected = Arc::new(Mutex::new(false));
    let observed = disconnected.clone();
    let old = source.clone();
    let away = retained.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_ne!(
                value["kind"], "done",
                "Disconnected scan must never announce completion"
            );
            if value["kind"] == "batch" && !*observed.lock().unwrap() {
                fs::rename(&old, &away).unwrap();
                *observed.lock().unwrap() = true;
            }
        }
        Ok(())
    });
    assert!(scan_directory(&s, &source, folder.id, 0, &channel).is_err());
    assert!(*disconnected.lock().unwrap());
    let id: String = s
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT id FROM assets LIMIT 1", [], |r| r.get(0))
        .unwrap();
    s.catalog
        .lock()
        .unwrap()
        .rate(std::slice::from_ref(&id), Some(4))
        .unwrap();
    fs::rename(&retained, &source).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    assert_eq!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "done")
            .unwrap()["count"],
        130
    );
    let c = s.catalog.lock().unwrap();
    let rating: i32 =
        c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&id], |r| r.get(0))
            .unwrap();
    assert_eq!(rating, 4);
    let count: i64 =
        c.db.query_row("SELECT count(*) FROM assets", [], |r| r.get(0))
            .unwrap();
    assert_eq!(count, 130);
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
            "skipped": {"videos":0,"raw":0,"images":0},
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

#[test]
fn mixed_scan_reports_skipped_media_without_cataloguing_them() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    image::RgbImage::new(48, 32)
        .save(source.join("photo.jpg"))
        .unwrap();
    for name in [
        "clip.MP4",
        "clip.mov",
        "photo.DNG",
        "other.AVIF",
        "photo.jpg.xmp",
        "notes.txt",
    ] {
        fs::write(source.join(name), b"generated format marker").unwrap();
    }
    let hidden = source.join(".hidden");
    fs::create_dir(&hidden).unwrap();
    fs::write(hidden.join("ignored.mp4"), b"hidden marker").unwrap();
    let s = state(&base.join("runtime"));
    let f = grant(&s, source.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &source, f.id, 0, &channel).unwrap();
    let recorded = recorded.lock().unwrap();
    let done = recorded.iter().find(|e| e["kind"] == "done").unwrap();
    assert_eq!(done["count"], 1);
    assert_eq!(
        done["skipped"],
        serde_json::json!({"videos":2,"raw":1,"images":1})
    );
    let catalog = s.catalog.lock().unwrap();
    assert_eq!(
        catalog
            .db
            .query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn generated_outputs_are_skipped_under_parent_but_can_be_opened_explicitly() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("sources");
    fs::create_dir(&source).unwrap();
    let pixel = image::RgbImage::new(48, 32);
    pixel.save(source.join("original.jpg")).unwrap();
    let outputs = [
        "Moment-Grove-1794834919-d42deae3",
        "Moment-Grove-Watermark-1794835283-f5daaad7",
        "Moment-Grove-Move-1794835283-aabbccdd",
        "Moment-Grove-Quarantine-1794835283-01020304",
        "Grove-1794834919-d42deae3",
        "Grove-Watermark-1794835283-f5daaad7",
        "Grove-Move-1794835283-aabbccdd",
        "Grove-Quarantine-1794835283-01020304",
    ];
    for name in outputs
        .iter()
        .chain(["Moment-Grove-holiday", "Moment-Grove-2026-album"].iter())
    {
        let child = source.join(name);
        fs::create_dir(&child).unwrap();
        pixel.save(child.join("same-name.jpg")).unwrap();
    }
    let s = state(&base.join("runtime"));
    let folder = grant(&s, source.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &source, folder.id, 0, &channel).unwrap();
    let values = recorded.lock().unwrap();
    assert_eq!(
        values.iter().find(|e| e["kind"] == "done").unwrap()["count"],
        3
    );
    drop(values);
    let output = source.join(outputs[1]);
    let f = grant(&s, output.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &output, f.id, 0, &channel).unwrap();
    assert_eq!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "done")
            .unwrap()["count"],
        1
    );
}

#[test]
fn export_destination_shows_copy_and_watermark_batches_together() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let parent = base.join("exports-for-sharing");
    fs::create_dir(&parent).unwrap();
    for name in [
        "Grove-123-deadbeef",
        "Grove-Watermark-456-aaaabbbb",
        "Grove-Quarantine-456-ccccdddd",
        "Moment-Grove-Move-123-ffffffff",
    ] {
        let child = parent.join(name);
        fs::create_dir(&child).unwrap();
        image::RgbImage::new(48, 32)
            .save(child.join("item.jpg"))
            .unwrap();
    }
    fs::write(parent.join(".DS_Store"), []).unwrap();
    let s = state(&base.join("runtime"));
    let folder = grant(&s, parent.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &parent, folder.id, 0, &channel).unwrap();
    assert_eq!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "done")
            .unwrap()["count"],
        2
    );
}

#[test]
fn jpeg_scan_dimensions_match_oriented_preview_for_all_eight_directions() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let sources = base.join("sources");
    fs::create_dir(&sources).unwrap();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::new(80, 40))
        .unwrap();
    for orientation in 1..=8u8 {
        let exif = [
            69,
            120,
            105,
            102,
            0,
            0,
            73,
            73,
            42,
            0,
            8,
            0,
            0,
            0,
            1,
            0,
            18,
            1,
            3,
            0,
            1,
            0,
            0,
            0,
            orientation,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let mut bytes = jpeg[..2].to_vec();
        bytes.extend_from_slice(&[255, 225]);
        bytes.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(&exif);
        bytes.extend_from_slice(&jpeg[2..]);
        fs::write(sources.join(format!("generated-{orientation}.jpg")), bytes).unwrap();
    }
    let s = state(&base.join("runtime"));
    let f = grant(&s, sources.clone()).unwrap();
    let (channel, recorded) = events();
    scan_directory(&s, &sources, f.id, 0, &channel).unwrap();
    let recorded = recorded.lock().unwrap();
    let items: Vec<crate::catalog::Asset> = recorded
        .iter()
        .filter(|e| e["kind"] == "batch")
        .flat_map(|e| {
            serde_json::from_value::<Vec<crate::catalog::Asset>>(e["items"].clone()).unwrap()
        })
        .collect();
    assert_eq!(items.len(), 8);
    for a in items {
        let preview = image::load_from_memory(&imaging::render(&s, &a.id, 480).unwrap()).unwrap();
        assert_eq!(
            (a.width, a.height),
            (preview.width(), preview.height()),
            "catalog dimensions must describe the displayed orientation"
        );
    }
}
