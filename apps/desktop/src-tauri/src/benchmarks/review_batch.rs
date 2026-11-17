//! Opt-in full-resolution workload; the caller supplies only owned test copies.
use crate::{
    catalog::Asset, exports, grant, imaging, safety_tests::state, scan_directory, watermark,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Instant,
};
use tauri::ipc::Channel;
fn hash(path: &Path) -> String {
    exports::files::hash_bytes(&fs::read(path).unwrap())
}
fn percentile(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[(v.len() * 95).div_ceil(100).saturating_sub(1)]
}
#[test]
#[ignore = "Explicit MG_REVIEW_ROOT owned generated/licensed copies and baseline/optimized pass"]
fn full_resolution_review_workflow() {
    let root = PathBuf::from(std::env::var("MG_REVIEW_ROOT").unwrap());
    assert_eq!(root.canonicalize().unwrap(), root);
    assert_eq!(root.parent().unwrap(), Path::new("/private/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("moment-grove-review-")
    );
    let run = std::env::var("MG_REVIEW_PASS").unwrap();
    assert!(["baseline", "optimized"].contains(&run.as_str()));
    let sources = root.join("sources");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("fixtures.json")).unwrap()).unwrap();
    let verify_sources = || {
        for f in manifest["files"].as_array().unwrap() {
            assert_eq!(
                hash(&sources.join(f["name"].as_str().unwrap())),
                f["sha256"].as_str().unwrap()
            );
        }
    };
    verify_sources();
    let data = root.join(format!("backend-{run}"));
    assert!(!data.exists());
    fs::create_dir(&data).unwrap();
    let s = state(&data);
    let folder = grant(&s, sources.clone()).unwrap();
    let found = Arc::new(Mutex::new(Vec::<Asset>::new()));
    let collected = found.clone();
    let start = Instant::now();
    scan_directory(
        &s,
        &sources,
        folder.id,
        0,
        &Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                if v["kind"] == "batch" {
                    collected
                        .lock()
                        .unwrap()
                        .extend(serde_json::from_value::<Vec<Asset>>(v["items"].clone()).unwrap());
                }
            }
            Ok(())
        }),
    )
    .unwrap();
    let scan_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut assets = found.lock().unwrap().clone();
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(assets.len(), 506);
    assert!(
        assets
            .iter()
            .all(|a| a.width as u64 * a.height as u64 >= 12_000_000)
    );
    let mut thumbs = Vec::new();
    for (i, a) in assets.iter().enumerate() {
        let start = Instant::now();
        let bytes = imaging::render(&s, &a.id, 480).unwrap();
        assert!(!bytes.is_empty());
        thumbs.push(start.elapsed().as_secs_f64() * 1000.);
        if i % 100 == 99 {
            println!("REVIEW_THUMB {run} {}", i + 1);
        }
    }
    let selected: Vec<_> = (0..72)
        .map(|i| &assets[(i * 37) % assets.len()])
        .chain(assets.iter().filter(|a| a.name.starts_with("phone-")))
        .collect();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    let mut preparation_ms = Vec::new();
    for a in &selected {
        if run == "optimized" {
            let start = Instant::now();
            imaging::render(&s, &a.id, 1600).unwrap();
            preparation_ms.push(start.elapsed().as_secs_f64() * 1000.);
        }
        let start = Instant::now();
        let bytes = imaging::render(&s, &a.id, 1600).unwrap();
        assert!(!bytes.is_empty());
        cold.push(start.elapsed().as_secs_f64() * 1000.);
    }
    for a in &selected {
        let start = Instant::now();
        imaging::render(&s, &a.id, 1600).unwrap();
        warm.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let mut cache_maintenance = Vec::new();
    for _ in 0..24 {
        let start = Instant::now();
        imaging::prune_cache(&data.join("cache"), 256 * 1024 * 1024).unwrap();
        cache_maintenance.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let before: Vec<_> = {
        let mut c = s.catalog.lock().unwrap();
        for (i, a) in assets.iter().enumerate() {
            c.rate(
                std::slice::from_ref(&a.id),
                [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)][i % 7],
            )
            .unwrap();
        }
        let b = c.rate(&[assets[0].id.clone()], Some(5)).unwrap();
        c.undo(&b.token).unwrap();
        let mut q =
            c.db.prepare("SELECT id,path,identity,rating,revision FROM assets ORDER BY id")
                .unwrap();
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i32>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    drop(s);
    let s = state(&data);
    grant(&s, sources.clone()).unwrap();
    scan_directory(&s, &sources, folder.id, 0, &Channel::new(|_| Ok(()))).unwrap();
    let c = s.catalog.lock().unwrap();
    let after: Vec<_> = {
        let mut q =
            c.db.prepare("SELECT id,path,identity,rating,revision FROM assets ORDER BY id")
                .unwrap();
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i32>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert_eq!(before, after);
    let targets: Vec<exports::Target> = {
        let mut q = c
            .db
            .prepare("SELECT id,revision FROM assets WHERE rating IN (4,5) ORDER BY path LIMIT 12")
            .unwrap();
        q.query_map([], |r| {
            Ok(exports::Target {
                id: r.get(0)?,
                revision: r.get(1)?,
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert_eq!(targets.len(), 12);
    let outputs = root.join(format!("outputs-{run}"));
    fs::create_dir(&outputs).unwrap();
    let roots = std::collections::HashMap::from([(folder.id, sources.clone())]);
    let no = AtomicBool::new(false);
    let started = Instant::now();
    let p = exports::prepare(&c, &roots, folder.id, &targets, &outputs, &data).unwrap();
    let copy = exports::run(p, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!(copy.verified, 12);
    let copy_ms = started.elapsed().as_secs_f64() * 1000.;
    let options = watermark::Options {
        fallback_date: Some("2026-11-18".into()),
        city: "测试城市".into(),
        output_max_edge: Some(2048),
        jpeg_quality: 80,
        ..Default::default()
    };
    let started = Instant::now();
    let p = exports::prepare_watermark(&c, &roots, folder.id, &targets, &outputs, &data, &options)
        .unwrap();
    let wm = exports::run(p, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!(wm.verified, 12);
    let watermark_ms = started.elapsed().as_secs_f64() * 1000.;
    assert_eq!(
        c.db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(c);
    crate::workspace::save(
        &s,
        Some(folder.id),
        crate::workspace::View {
            mode: "review".into(),
            active: assets[0].id.clone(),
            filter: "all".into(),
            sort: "name".into(),
            search: String::new(),
            size: 260,
            auto_advance: false,
            theme: "light".into(),
            offset: 0.,
        },
    )
    .unwrap();
    verify_sources();
    let result = serde_json::json!({"pass":run,"preparationTotalMs":preparation_ms.iter().sum::<f64>(),"visibleCondition":if run=="optimized" {"after next-photo preparation has completed; background work is counted separately"} else {"on-demand cold preview"},"files":506,"fullResolution":true,"scanMs":scan_ms,"thumbP95Ms":percentile(&thumbs),"visiblePreviewP95Ms":percentile(&cold),"visiblePreviewTotalMs":cold.iter().sum::<f64>(),"warmPreviewP95Ms":percentile(&warm),"cacheMaintenanceP95Ms":percentile(&cache_maintenance),"copyMs":copy_ms,"watermarkMs":watermark_ms,"copied":copy.verified,"watermarks":wm.verified,"ratingsAndIdsRetained":true,"originalsUnchanged":true,"scope":"actual scanner/decoder/catalog/export; no WebView, keyboard throughput or native UI latency"});
    fs::write(
        root.join(format!("{run}.json")),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("FULL_RESOLUTION_REVIEW {result}");
}
