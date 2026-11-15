//! Opt-in workflow over an explicitly prepared, authorized copy set.
//! Private fixtures and reports stay outside source control.
use crate::{
    catalog::Asset, exports, grant, imaging, safety_tests::state, scan_directory, watermark,
    workspace, xmp,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tauri::ipc::Channel;

fn scan(s: &crate::AppState, root: &Path, folder: i64) -> Vec<Asset> {
    let rows = Arc::new(Mutex::new(Vec::new()));
    let captured = rows.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            let event: serde_json::Value = serde_json::from_str(&text).unwrap();
            if event["kind"] == "batch" {
                captured
                    .lock()
                    .unwrap()
                    .extend(serde_json::from_value::<Vec<Asset>>(event["items"].clone()).unwrap());
            }
        }
        Ok(())
    });
    scan_directory(s, root, folder, 0, &channel).unwrap();
    let mut rows = rows.lock().unwrap().clone();
    rows.sort_by(|a, b| a.relative.cmp(&b.relative));
    rows
}
fn checksum(path: &Path) -> String {
    exports::files::hash_bytes(&fs::read(path).unwrap())
}

#[test]
#[ignore = "Requires explicitly authorized, prepared copies via MG_DAILY_QA_ROOT; never discovers personal libraries"]
fn authorized_copy_set_survives_review_restart_and_batched_exports() {
    let base = PathBuf::from(
        std::env::var("MG_DAILY_QA_ROOT").expect("Explicit prepared QA root required"),
    );
    assert_eq!(base.canonicalize().unwrap(), base);
    assert!(
        base.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("moment-grove-daily-")
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(base.join("fixtures.json")).unwrap()).unwrap();
    let input = manifest["items"].as_array().unwrap();
    assert!(input.len() >= 7);
    let names: Vec<_> = input.iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert!(
        names
            .iter()
            .all(|n| Path::new(n).components().count() == 1 && n.starts_with("sample-"))
    );
    let sources = base.join("sources");
    let run = base.join(format!("backend-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&run).unwrap();
    let data = run.join("runtime");
    let output = run.join("outputs");
    fs::create_dir(&output).unwrap();
    let s = state(&data);
    let folder = grant(&s, sources.clone()).unwrap();
    let start = Instant::now();
    let assets = scan(&s, &sources, folder.id);
    let scan_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(assets.len(), input.len());
    let mut preview_ms = Vec::new();
    let mut max_reference_mae = 0f64;
    for a in &assets {
        let m = input.iter().find(|m| m["name"] == a.name).unwrap();
        assert_eq!(
            (a.width as u64, a.height as u64),
            (m["width"].as_u64().unwrap(), m["height"].as_u64().unwrap())
        );
        assert!(a.captured.is_some());
        assert_eq!(
            checksum(&sources.join(&a.name)),
            m["sha256"].as_str().unwrap()
        );
        for size in [480, 1600] {
            let start = Instant::now();
            let bytes = imaging::render(&s, &a.id, size).unwrap();
            preview_ms.push(start.elapsed().as_secs_f64() * 1000.);
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert!(decoded.width() <= size && decoded.height() <= size);
            assert!(
                (decoded.width() as f64 / decoded.height() as f64
                    - a.width as f64 / a.height as f64)
                    .abs()
                    < 0.005
            );
            let reference = base.join("references").join(format!("{}.png", a.name));
            if size == 1600 && reference.is_file() {
                let reference = image::open(reference).unwrap().to_rgb8();
                let reference = image::imageops::resize(
                    &reference,
                    decoded.width(),
                    decoded.height(),
                    image::imageops::FilterType::Triangle,
                );
                let mut diff = 0.;
                let mut count = 0;
                for y in (8..decoded.height() - 8).step_by(9) {
                    for x in (8..decoded.width() - 8).step_by(9) {
                        for c in 0..3 {
                            diff += (f64::from(decoded.get_pixel(x, y)[c])
                                - f64::from(reference.get_pixel(x, y)[c]))
                            .abs();
                            count += 1;
                        }
                    }
                }
                let mae = diff / count as f64;
                assert!(mae < 8., "SDR reference mismatch for {}: {mae}", a.name);
                max_reference_mae = max_reference_mae.max(mae);
            }
        }
    }
    let expected: Vec<_> = assets
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let rating = if i % 7 == 0 {
                None
            } else {
                Some((i % 7 - 1) as i32)
            };
            let mut c = s.catalog.lock().unwrap();
            c.rate(std::slice::from_ref(&a.id), rating).unwrap();
            let rev = c.location(&a.id).unwrap().2;
            (a.id.clone(), a.relative.clone(), rating, rev)
        })
        .collect();
    let view=serde_json::from_value(serde_json::json!({"mode":"review","active":expected[expected.len()/2].0,"filter":"all","sort":"name","search":"","size":260,"autoAdvance":false,"theme":"dark","offset":0})).unwrap();
    workspace::save(&s, Some(folder.id), view).unwrap();
    drop(s);
    let s = state(&data);
    assert_eq!(workspace::resume(&s).unwrap().unwrap().id, folder.id);
    assert_eq!(
        workspace::load(&s).unwrap().unwrap().view.active,
        expected[expected.len() / 2].0
    );
    let rescanned = scan(&s, &sources, folder.id);
    assert_eq!(rescanned.len(), expected.len());
    for (a, e) in rescanned.iter().zip(&expected) {
        assert_eq!(
            (&a.id, &a.relative, a.rating, a.revision),
            (&e.0, &e.1, e.2, e.3)
        );
    }
    let roots = s.allowed.lock().unwrap().clone();
    let c = s.catalog.lock().unwrap();
    let targets: Vec<_> = expected
        .iter()
        .map(|e| exports::Target {
            id: e.0.clone(),
            revision: e.3,
        })
        .collect();
    let pending: i64 =
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
            .unwrap();
    let history: i64 =
        c.db.query_row("SELECT count(*) FROM history", [], |r| r.get(0))
            .unwrap();
    let mut copied = 0;
    let mut copy_batches = 0;
    for batch in targets.chunks(100) {
        let plan = exports::prepare(&c, &roots, folder.id, batch, &output, &data).unwrap();
        let report = exports::run(plan, &data, &AtomicBool::new(false), |_| {}, || Ok(())).unwrap();
        assert_eq!(report.status, "completed");
        assert_eq!(report.verified, batch.len());
        for row in report.items {
            let source = sources.join(&row.name);
            let out = Path::new(&report.destination).join(&row.output);
            assert_eq!(checksum(&source), checksum(&out));
            assert_eq!(
                xmp::read(
                    &fs::read_to_string(out.with_file_name(format!("{}.xmp", row.output))).unwrap()
                )
                .unwrap(),
                row.rating
            );
        }
        copied += report.verified;
        copy_batches += 1;
    }
    let selected: Vec<_> = manifest["referenceNames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            let a = assets
                .iter()
                .find(|a| a.name == n.as_str().unwrap())
                .unwrap();
            targets.iter().find(|t| t.id == a.id).unwrap().clone()
        })
        .collect();
    assert!(selected.len() > 2 && selected.len() <= 100);
    let options = watermark::Options {
        city: "测试城市".into(),
        ..Default::default()
    };
    let plan =
        exports::prepare_watermark(&c, &roots, folder.id, &selected, &output, &data, &options)
            .unwrap();
    let cancel = AtomicBool::new(false);
    let first = exports::run(
        plan,
        &data,
        &cancel,
        |r| {
            if r.verified == 2 {
                cancel.store(true, Ordering::SeqCst)
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!(first.status, "cancelled");
    assert_eq!(first.verified, 2);
    let plan = exports::resume(&c, &roots, folder.id, &first.id, &output, &data).unwrap();
    let report = exports::run(plan, &data, &AtomicBool::new(false), |_| {}, || Ok(())).unwrap();
    assert_eq!(report.status, "completed");
    assert_eq!(report.verified, selected.len());
    for row in report.items {
        let bytes = fs::read(Path::new(&report.destination).join(&row.output)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        let original = input.iter().find(|m| m["name"] == row.name).unwrap();
        assert_eq!(
            (decoded.width() as u64, decoded.height() as u64),
            (
                original["width"].as_u64().unwrap(),
                original["height"].as_u64().unwrap()
            )
        );
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&bytes))
        {
            assert!(
                exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                    .is_none()
            );
            assert!(
                exif.get_field(exif::Tag::Model, exif::In::PRIMARY)
                    .is_none()
            );
        }
    }
    for item in input {
        assert_eq!(
            checksum(&sources.join(item["name"].as_str().unwrap())),
            item["sha256"].as_str().unwrap()
        );
        assert!(
            !sources
                .join(format!("{}.xmp", item["name"].as_str().unwrap()))
                .exists()
        );
    }
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        pending
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM history", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        history
    );
    assert_eq!(
        c.db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    preview_ms.sort_by(f64::total_cmp);
    let p95 = preview_ms[(preview_ms.len() * 95 / 100).min(preview_ms.len() - 1)];
    let evidence = serde_json::json!({"photos":assets.len(),"previews":preview_ms.len(),"previewBackendColdP95Ms":p95,"scanBackendMs":scan_ms,"referenceMaxMAE":max_reference_mae,"copies":copied,"copyBatches":copy_batches,"watermarks":selected.len(),"watermarkCancelResume":true,"idsAndSevenRatingStatesPreserved":true,"pending":pending,"history":history,"originalCopiesUnchanged":assets.len(),"integrity":"ok","metricScope":"explicit authorized copies; backend timings, not UI latency; private media not distributed"});
    fs::write(
        run.join("result.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    println!("DAILY_TRIAL {evidence}");
}
