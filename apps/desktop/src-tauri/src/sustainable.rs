//! Explicit licensed device fixture trial, never personal library discovery.
use crate::{
    catalog::Asset, exports, grant, imaging, safety_tests::state, scan_directory, watermark,
    workspace,
};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicBool},
};
use tauri::ipc::Channel;

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Three pinned CC BY-SA4.0 HEIC/HEIF originals via MG_PHONE_FIXTURES"]
fn original_phone_heif_preview_dates_export_and_restart() {
    let input = std::path::PathBuf::from(std::env::var_os("MG_PHONE_FIXTURES").unwrap());
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let source = base.join("sources");
    let output = base.join("outputs");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&output).unwrap();
    let cases = [
        (
            "HMD_Nokia_8.3_5G.heif",
            "c2820b3e4e0368c6f031b28b62945658a97446895cb446dfc37acdcf7109e0f5",
            3456,
            4608,
            false,
        ),
        (
            "HMD_Nokia_8.3_5G_hdr.heif",
            "17fccf795b0e2b749b9f5667ffcea31e665cb6964fb104b001684c60a14acb4d",
            4608,
            3456,
            false,
        ),
        (
            "iphone_13_pro_max.HEIC",
            "e760c80eed310e4f27c092d5487693ca8e104e7cc01d25ba4828deb28f679676",
            3024,
            4032,
            true,
        ),
    ];
    for (name, hash, _, _, _) in &cases {
        let bytes = fs::read(input.join(name)).unwrap();
        assert_eq!(exports::files::hash_bytes(&bytes), *hash);
        fs::write(source.join(name), bytes).unwrap();
    }
    let data = base.join("runtime");
    let s = state(&data);
    let f = grant(&s, source.clone()).unwrap();
    let received = Arc::new(Mutex::new(Vec::<Asset>::new()));
    let values = received.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            if v["kind"] == "batch" {
                values
                    .lock()
                    .unwrap()
                    .extend(serde_json::from_value::<Vec<Asset>>(v["items"].clone()).unwrap());
            }
        }
        Ok(())
    });
    scan_directory(&s, &source, f.id, 0, &channel).unwrap();
    let assets = received.lock().unwrap().clone();
    assert_eq!(assets.len(), 3);
    for a in &assets {
        let case = cases.iter().find(|x| x.0 == a.name).unwrap();
        assert_eq!((a.width, a.height), (case.2, case.3));
        assert_eq!(a.captured.is_some(), case.4);
        for size in [480, 1600] {
            let bytes = imaging::render(&s, &a.id, size).unwrap();
            let preview = image::load_from_memory(&bytes).unwrap();
            assert!(preview.width() <= size && preview.height() <= size);
            assert!(
                (preview.width() as f64 / preview.height() as f64
                    - a.width as f64 / a.height as f64)
                    .abs()
                    < 0.004
            );
        }
        s.catalog
            .lock()
            .unwrap()
            .rate(std::slice::from_ref(&a.id), Some(4))
            .unwrap();
    }
    let view:workspace::View=serde_json::from_value(serde_json::json!({"mode":"review","active":assets[2].id,"filter":"all","sort":"name","search":"","size":260,"autoAdvance":false,"theme":"dark","offset":0})).unwrap();
    workspace::save(&s, Some(f.id), view).unwrap();
    drop(s);
    let s = state(&data);
    assert_eq!(workspace::resume(&s).unwrap().unwrap().id, f.id);
    assert_eq!(
        workspace::load(&s).unwrap().unwrap().view.active,
        assets[2].id
    );
    let c = s.catalog.lock().unwrap();
    let roots = s.allowed.lock().unwrap().clone();
    let targets: Vec<_> = assets
        .iter()
        .map(|a| exports::Target {
            id: a.id.clone(),
            revision: c.location(&a.id).unwrap().2,
        })
        .collect();
    let missing = targets
        .iter()
        .filter(|t| assets.iter().any(|a| a.id == t.id && a.captured.is_none()))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        exports::prepare_watermark(
            &c,
            &roots,
            f.id,
            &missing,
            &output,
            &data,
            &watermark::Options::default()
        )
        .is_err()
    );
    let options = watermark::Options {
        fallback_date: Some("2000-01-02".into()),
        output_max_edge: Some(2048),
        ..Default::default()
    };
    let copy = exports::prepare(&c, &roots, f.id, &targets, &output, &data).unwrap();
    let mark =
        exports::prepare_watermark(&c, &roots, f.id, &targets, &output, &data, &options).unwrap();
    drop(c);
    let no = AtomicBool::new(false);
    let copy = exports::run(copy, &data, &no, |_| {}, || Ok(())).unwrap();
    let mark = exports::run(mark, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!(
        (
            copy.status.as_str(),
            copy.verified,
            mark.status.as_str(),
            mark.verified
        ),
        ("completed", 3, "completed", 3)
    );
    for row in &copy.items {
        assert_eq!(
            fs::read(Path::new(&copy.destination).join(&row.output)).unwrap(),
            fs::read(source.join(&row.name)).unwrap()
        );
    }
    for row in &mark.items {
        let bytes = fs::read(Path::new(&mark.destination).join(&row.output)).unwrap();
        let photo = image::load_from_memory(&bytes).unwrap();
        assert_eq!(photo.width().max(photo.height()), 2048);
        let date = if row.name.contains("iphone") {
            "2022-02-16"
        } else {
            "2000-01-02"
        };
        assert!(row.caption.as_deref().unwrap().starts_with(date));
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(bytes))
        {
            assert!(
                exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                    .is_none()
            );
        }
    }
    for (name, hash, _, _, _) in &cases {
        assert_eq!(
            exports::files::hash_bytes(&fs::read(source.join(name)).unwrap()),
            *hash
        );
    }
    for a in &assets {
        assert_eq!(
            s.catalog
                .lock()
                .unwrap()
                .db
                .query_row("SELECT rating FROM assets WHERE id=?1", [&a.id], |r| r
                    .get::<_, i32>(0))
                .unwrap(),
            4
        );
    }
    println!(
        "PHONE_ORIGINALS {{\"files\":3,\"previews\":6,\"copies\":3,\"watermarks\":3,\"missingDatesRequireManualFallback\":true,\"restartRatingsPreserved\":true,\"sourceBytesUnchanged\":true}}"
    );
}

#[test]
#[ignore = "Requires the exact three attributed JPEG QA samples via MG_DEVICE_FIXTURES"]
fn licensed_devices_preview_export_and_resume() {
    let input = std::env::var_os("MG_DEVICE_FIXTURES")
        .expect("Explicit licensed fixture directory required");
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let sources = base.join("sources");
    fs::create_dir(&sources).unwrap();
    let output = base.join("outputs");
    fs::create_dir(&output).unwrap();
    let names = [
        (
            "01-iphone13.jpg",
            "a8a4058ca272dc40e802b8d5e102c643f0aa0cbb941db0ebdb0cbddca30bcafa",
            "iPhone 13",
        ),
        (
            "02-meizu20.jpg",
            "beb582443f390037abda607b5dbe506e99afc8b3722cb98d3e36826239b2b22a",
            "MEIZU 20",
        ),
        (
            "03-canon.jpg",
            "5619a95628ab5a3a6f12ee9fc284abe7c9dd64910052543d831af0f167386b14",
            "Canon",
        ),
    ];
    let mut original = Vec::new();
    let mut models = Vec::new();
    for (name, expected, model) in names {
        let bytes = fs::read(Path::new(&input).join(name)).unwrap();
        assert_eq!(exports::files::hash_bytes(&bytes), expected);
        let exif = exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(&bytes))
            .unwrap();
        let camera = exif
            .get_field(exif::Tag::Model, exif::In::PRIMARY)
            .unwrap()
            .display_value()
            .to_string();
        assert!(camera.contains(model), "Expected {model}, found {camera}");
        models.push(camera);
        fs::write(sources.join(name), &bytes).unwrap();
        original.push((name, bytes));
    }
    let data = base.join("runtime");
    let s = state(&data);
    let folder = grant(&s, sources.clone()).unwrap();
    let captured = Arc::new(Mutex::new(Vec::<Asset>::new()));
    let collect = |values: Arc<Mutex<Vec<Asset>>>| {
        Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                if value["kind"] == "batch" {
                    values.lock().unwrap().extend(
                        serde_json::from_value::<Vec<Asset>>(value["items"].clone()).unwrap(),
                    );
                }
            }
            Ok(())
        })
    };
    scan_directory(&s, &sources, folder.id, 0, &collect(captured.clone())).unwrap();
    let assets = captured.lock().unwrap().clone();
    assert_eq!(assets.len(), 3);
    let mut supported = Vec::new();
    for a in &assets {
        assert!(a.captured.is_some());
        supported.push(a.id.clone());
        for size in [480, 1600] {
            let bytes = imaging::render(&s, &a.id, size).unwrap();
            let image = image::load_from_memory(&bytes).unwrap();
            assert!(
                image.width() > 0
                    && image.height() > 0
                    && image.width() <= size
                    && image.height() <= size
            );
            assert!(
                (image.width() as f64 / image.height() as f64 - a.width as f64 / a.height as f64)
                    .abs()
                    < 0.004,
                "{} preview aspect ratio changed",
                a.name
            );
        }
    }
    let ids: Vec<_> = assets.iter().map(|a| a.id.clone()).collect();
    s.catalog.lock().unwrap().rate(&ids, Some(4)).unwrap();
    let view:workspace::View=serde_json::from_value(serde_json::json!({"mode":"review","active":ids[1],"filter":"unreviewed","sort":"name","search":"","size":260,"autoAdvance":false,"theme":"dark","offset":0})).unwrap();
    workspace::save(&s, Some(folder.id), view).unwrap();
    drop(s);
    let s = state(&data);
    assert_eq!(workspace::resume(&s).unwrap().unwrap().id, folder.id);
    assert_eq!(workspace::load(&s).unwrap().unwrap().view.active, ids[1]);
    let c = s.catalog.lock().unwrap();
    let roots = s.allowed.lock().unwrap().clone();
    let targets: Vec<_> = ids
        .iter()
        .map(|id| exports::Target {
            id: id.clone(),
            revision: c.location(id).unwrap().2,
        })
        .collect();
    for id in &ids {
        let rating: i32 =
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
        assert_eq!(rating, 4);
    }
    let plan = exports::prepare(&c, &roots, folder.id, &targets, &output, &data).unwrap();
    drop(c);
    let report = exports::run(plan, &data, &AtomicBool::new(false), |_| {}, || Ok(())).unwrap();
    assert_eq!(report.verified, 3);
    assert_eq!(
        report.status,
        "completed",
        "{}",
        serde_json::to_string(&report).unwrap()
    );
    for row in &report.items {
        let original = original.iter().find(|(name, _)| *name == row.name).unwrap();
        assert_eq!(
            fs::read(Path::new(&report.destination).join(&row.output)).unwrap(),
            original.1
        );
    }
    let c = s.catalog.lock().unwrap();
    let options = watermark::Options {
        city: "试用样本".into(),
        ..Default::default()
    };
    let watermark_targets: Vec<_> = targets
        .iter()
        .filter(|t| supported.contains(&t.id))
        .cloned()
        .collect();
    let plan = exports::prepare_watermark(
        &c,
        &roots,
        folder.id,
        &watermark_targets,
        &output,
        &data,
        &options,
    )
    .unwrap();
    drop(c);
    let report = exports::run(plan, &data, &AtomicBool::new(false), |_| {}, || Ok(())).unwrap();
    assert_eq!(
        report.status,
        "completed",
        "{}",
        serde_json::to_string(&report).unwrap()
    );
    assert_eq!(report.verified, 3);
    for row in &report.items {
        let bytes = fs::read(Path::new(&report.destination).join(&row.output)).unwrap();
        assert!(image::load_from_memory(&bytes).is_ok());
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(bytes))
        {
            assert!(
                exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                    .is_none()
            );
        }
    }
    for (name, bytes) in &original {
        assert_eq!(&fs::read(sources.join(name)).unwrap(), bytes);
    }
    let pending: i64 = s
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT COUNT(*) FROM pending_xmp", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, 3);
    println!(
        "SUSTAINABLE_DEVICES {}",
        serde_json::json!({"models":models,"sourcePhotos":3,"previews":6,"copies":3,"watermarks":3,"unsupported":[],"restartRatingPreserved":true,"pending":pending,"sourceBytesUnchanged":true})
    );
}
