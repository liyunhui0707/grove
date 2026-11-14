//! Opt-in end-to-end scan benchmark. All input files are procedurally generated.
use crate::{catalog::Asset, grant, safety_tests::state, scan_directory};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tauri::ipc::Channel;
fn hash(p: &std::path::Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(p).unwrap()))
}
#[test]
#[ignore = "Generated 3000-file mixed library; explicit MG_LARGE_QA_ROOT and MG_LARGE_RUN required"]
fn mixed_library_scan_restart_and_ratings() {
    let root = PathBuf::from(std::env::var("MG_LARGE_QA_ROOT").unwrap());
    assert!(root.starts_with("/private/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("moment-grove-large-")
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    let run = std::env::var("MG_LARGE_RUN").unwrap();
    assert!(["baseline", "optimized"].contains(&run.as_str()));
    let sources = root.join("sources");
    let manifest = root.join("sources.json");
    if !sources.exists() {
        fs::create_dir(&sources).unwrap();
        let small = image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([x as u8, y as u8, ((x + y) / 3) as u8])
        });
        let hd = image::RgbImage::from_fn(4032, 3024, |x, y| {
            image::Rgb([(x / 16) as u8, (y / 12) as u8, ((x + y) / 24) as u8])
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
            .encode_image(&hd)
            .unwrap();
        let mut encoded = Vec::new();
        for ext in ["jpg", "png", "webp"] {
            let p = root.join(format!("generated.{ext}"));
            small.save(&p).unwrap();
            encoded.push(fs::read(p).unwrap());
        }
        let mut files = Vec::new();
        for i in 0..3000 {
            let ext = if i % 20 == 1 {
                "png"
            } else if i % 20 == 2 {
                "webp"
            } else {
                "jpg"
            };
            let p = sources.join(format!("照片_{i:05}.{ext}"));
            let data = if i % 100 == 0 {
                &bytes
            } else {
                &encoded[if ext == "png" {
                    1
                } else if ext == "webp" {
                    2
                } else {
                    0
                }]
            };
            fs::write(&p, data).unwrap();
            files.push((
                p.file_name().unwrap().to_str().unwrap().to_string(),
                hash(&p),
            ));
        }
        fs::write(&manifest, serde_json::to_vec_pretty(&files).unwrap()).unwrap();
    }
    let files: Vec<(String, String)> =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(files.len(), 3000);
    for (name, h) in &files {
        assert_eq!(&hash(&sources.join(name)), h);
    }
    let data = root.join(format!("runtime-{run}"));
    assert!(!data.exists(), "Preserve existing benchmark");
    let s = state(&data);
    let folder = grant(&s, sources.clone()).unwrap();
    let start = Instant::now();
    let assets = Arc::new(Mutex::new(Vec::<Asset>::new()));
    let first = Arc::new(Mutex::new(None::<f64>));
    let collect = |assets: Arc<Mutex<Vec<Asset>>>, first: Arc<Mutex<Option<f64>>>| {
        Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(text) = body {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                if v["kind"] == "batch" {
                    let mut f = first.lock().unwrap();
                    if f.is_none() {
                        *f = Some(start.elapsed().as_secs_f64() * 1000.0);
                    }
                    assets
                        .lock()
                        .unwrap()
                        .extend(serde_json::from_value::<Vec<Asset>>(v["items"].clone()).unwrap());
                }
            }
            Ok(())
        })
    };
    scan_directory(
        &s,
        &sources,
        folder.id,
        0,
        &collect(assets.clone(), first.clone()),
    )
    .unwrap();
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;
    let found = assets.lock().unwrap().clone();
    assert_eq!(found.len(), 3000);
    let mut ids: Vec<_> = found.iter().map(|a| a.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3000);
    for (i, a) in found.iter().take(7).enumerate() {
        s.catalog
            .lock()
            .unwrap()
            .rate(
                std::slice::from_ref(&a.id),
                if i == 6 { None } else { Some(i as i32) },
            )
            .unwrap();
    }
    let snapshots = |s: &crate::AppState| {
        s.catalog
            .lock()
            .unwrap()
            .db
            .prepare("SELECT id,path,identity,rating,revision FROM assets ORDER BY path")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<i32>>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    let before = snapshots(&s);
    drop(s);
    let s = state(&data);
    let f = grant(&s, sources.clone()).unwrap();
    let resumed = Arc::new(Mutex::new(Vec::<Asset>::new()));
    let again = Instant::now();
    scan_directory(
        &s,
        &sources,
        f.id,
        0,
        &collect(resumed.clone(), Arc::new(Mutex::new(None))),
    )
    .unwrap();
    let warm_ms = again.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(resumed.lock().unwrap().len(), 3000);
    assert_eq!(snapshots(&s), before);
    assert_eq!(
        s.catalog
            .lock()
            .unwrap()
            .db
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    for (name, h) in &files {
        assert_eq!(&hash(&sources.join(name)), h);
    }
    let report = serde_json::json!({"run":run,"photos":3000,"formats":{"jpeg":2700,"png":150,"webp":150},"twelveMegapixelJpegs":30,"otherDimensions":[320,240],"coldScanMs":cold_ms,"firstBatchMs":*first.lock().unwrap(),"restartScanMs":warm_ms,"idsRatingsRevisionsPreserved":true,"originalsUnchanged":3000,"integrity":"ok","metric":"real scanner/catalog, not UI latency"});
    fs::write(
        root.join(format!("{run}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("LARGE_LIBRARY {report}");
}
