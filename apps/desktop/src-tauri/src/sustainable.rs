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
    let mut unsupported = Vec::new();
    for a in &assets {
        assert!(a.captured.is_some());
        if a.name == "01-iphone13.jpg" {
            let error = imaging::render(&s, &a.id, 1600).unwrap_err();
            assert!(error.contains("HDR增益图"));
            unsupported.push(error);
            continue;
        }
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
    assert!(
        exports::prepare_watermark(&c, &roots, folder.id, &targets, &output, &data, &options)
            .is_err()
    );
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
    assert_eq!(report.verified, 2);
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
        serde_json::json!({"models":models,"sourcePhotos":3,"previews":4,"copies":3,"watermarks":2,"unsupported":unsupported,"restartRatingPreserved":true,"pending":pending,"sourceBytesUnchanged":true})
    );
}
