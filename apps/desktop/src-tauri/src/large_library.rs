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
#[ignore = "Generated mixed library; explicit MG_LARGE_QA_ROOT and MG_LARGE_RUN required"]
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
    let total: usize = std::env::var("MG_LARGE_PHOTOS")
        .unwrap_or_else(|_| "3000".into())
        .parse()
        .unwrap();
    assert!([3000, 10000].contains(&total));
    let run = std::env::var("MG_LARGE_RUN").unwrap();
    assert!(["baseline", "optimized", "volume", "phone"].contains(&run.as_str()));
    let reliability = std::env::var_os("MG_PREVIEW_MATRIX").is_some();
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
        let png16 = root.join("gradient.png");
        image::ImageBuffer::from_fn(320, 240, |x, _| {
            image::Rgb([((x * 197) as u16), 32000, 65535])
        })
        .save(&png16)
        .unwrap();
        let high = fs::read(png16).unwrap();
        let mut files = Vec::new();
        for i in 0..total {
            let ext = if i % 20 == 1 {
                "png"
            } else if i % 20 == 2 {
                "webp"
            } else {
                "jpg"
            };
            let p = sources.join(format!("照片_{i:05}.{ext}"));
            let data: &[u8] = if reliability && i % 100 == 21 {
                &high
            } else if reliability && i % 100 == 41 {
                b"intentionally unreadable generated PNG"
            } else if i % 100 == 0 {
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
    assert_eq!(files.len(), total);
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
    assert_eq!(found.len(), total);
    let mut ids: Vec<_> = found.iter().map(|a| a.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total);
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
    assert_eq!(resumed.lock().unwrap().len(), total);
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
    let mut report = serde_json::json!({"run":run,"photos":total,"formats":{"jpeg":total*18/20,"png":total/20,"webp":total/20},"twelveMegapixelJpegs":total/100,"otherDimensions":[320,240],"coldScanMs":cold_ms,"firstBatchMs":*first.lock().unwrap(),"restartScanMs":warm_ms,"idsRatingsRevisionsPreserved":true,"originalsUnchanged":total,"integrity":"ok","metric":"real scanner/catalog, not UI latency"});
    if reliability {
        let mut sample = found.iter().collect::<Vec<_>>();
        sample.sort_by_key(|a| &a.name);
        let view = crate::workspace::View {
            branch: String::new(),
            mode: "review".into(),
            active: sample[20].id.clone(),
            filter: "all".into(),
            sort: "name".into(),
            search: String::new(),
            size: 260,
            auto_advance: false,
            theme: "light".into(),
            offset: 0.,
        };
        crate::workspace::save(&s, Some(folder.id), view).unwrap();
        let mut failures = std::collections::BTreeMap::new();
        let mut times = Vec::new();
        for pass in 0..2 {
            let mut ms = Vec::new();
            for asset in sample.iter().take(120) {
                let at = Instant::now();
                match crate::imaging::render(&s, &asset.id, 1600) {
                    Ok(_) => ms.push(at.elapsed().as_secs_f64() * 1000.),
                    Err(reason) if pass == 0 => *failures.entry(reason).or_insert(0) += 1,
                    Err(_) => (),
                }
            }
            ms.sort_by(f64::total_cmp);
            times.push(ms[ms.len() * 95 / 100]);
        }
        let compare = Instant::now();
        for asset in sample.iter().skip(201).take(8) {
            crate::imaging::render(&s, &asset.id, 480).unwrap();
        }
        report["previewSample"] = serde_json::json!({"count":120,"coldWarmP95Ms":times,
            "failedCold":failures,"eightThumbnailMs":compare.elapsed().as_secs_f64()*1000.});
    }
    fs::write(
        root.join(format!("{run}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("LARGE_LIBRARY {report}");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Owned APFS image only; explicit MG_VOLUME_QA_ROOT and seed/offline/verify phase"]
fn external_volume_restart_preserves_occurrences_and_exports() {
    use crate::{exports, imaging, watermark, workspace};
    use std::{os::unix::fs::MetadataExt, sync::atomic::AtomicBool};
    let base = PathBuf::from(std::env::var("MG_VOLUME_QA_ROOT").unwrap());
    assert_eq!(base.canonicalize().unwrap(), base);
    assert_eq!(base.parent().unwrap(), std::path::Path::new("/private/tmp"));
    assert!(
        base.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("moment-grove-volume-080-")
    );
    assert_eq!(fs::metadata(&base).unwrap().mode() & 0o777, 0o700);
    let source = base.join("mount/sources");
    let data = base.join("runtime");
    let phase = std::env::var("MG_VOLUME_PHASE").unwrap();
    assert!(["seed", "offline", "verify"].contains(&phase.as_str()));
    let snapshot = |s: &crate::AppState| {
        let c = s.catalog.lock().unwrap();
        let mut q =
            c.db.prepare("SELECT id,path,identity,rating,revision FROM assets ORDER BY path")
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
        .map(Result::unwrap)
        .collect::<Vec<_>>()
    };
    if phase == "seed" {
        assert!(
            !data.join("catalog.sqlite").exists(),
            "Preserve prior trial"
        );
    }
    let s = state(&data);
    if phase == "seed" {
        assert_ne!(
            fs::metadata(&source).unwrap().dev(),
            fs::metadata(&base).unwrap().dev(),
            "Actual mounted test volume required"
        );
        let f = grant(&s, source.clone()).unwrap();
        let mut files: Vec<_> = fs::read_dir(&source)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 10000);
        // Seed the exact legacy device/inode/birth identity used by 0.7.0.
        for (i, p) in files.iter().enumerate() {
            let mut c = s.catalog.lock().unwrap();
            let key = imaging::identity_from_metadata(&fs::metadata(p).unwrap()).unwrap();
            let id = c.upsert(f.id, p, &key, None).unwrap().0;
            if i < 6 {
                c.rate(&[id], Some(i as i32)).unwrap();
            }
        }
        let before = snapshot(&s);
        let view: workspace::View = serde_json::from_value(serde_json::json!({"mode":"review","active":before[9999].0,"filter":"all","sort":"name","search":"","size":260,"autoAdvance":false,"theme":"dark","offset":0})).unwrap();
        workspace::save(&s, Some(f.id), view.clone()).unwrap();
        let record = data.join("workspace.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
        legacy["identity"] = imaging::identity_from_metadata(&fs::metadata(&source).unwrap())
            .unwrap()
            .into();
        fs::write(&record, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(workspace::resume(&s).unwrap().unwrap().id, f.id);
        scan_directory(&s, &source, f.id, 0, &Channel::new(|_| Ok(()))).unwrap();
        let after = snapshot(&s);
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(
                (&old.0, &old.1, old.3, old.4),
                (&new.0, &new.1, new.3, new.4)
            );
            assert!(new.2.starts_with("volume:"));
        }
        workspace::save(&s, Some(f.id), view).unwrap();
        fs::write(
            base.join("expected-catalog.json"),
            serde_json::to_vec(&after).unwrap(),
        )
        .unwrap();
        println!(
            "VOLUME_SEED {}",
            serde_json::json!({"photos":after.len(),"legacyIdsAndRatingsPreserved":true,"device":fs::metadata(&source).unwrap().dev()})
        );
        return;
    }
    let expected: Vec<(String, String, String, Option<i32>, i64)> =
        serde_json::from_slice(&fs::read(base.join("expected-catalog.json")).unwrap()).unwrap();
    if phase == "offline" {
        assert!(!source.exists());
        assert!(workspace::resume(&s).is_err());
        assert_eq!(snapshot(&s), expected);
        println!("VOLUME_OFFLINE {{\"catalogUnchanged\":true,\"resumeRefused\":true}}");
        return;
    }
    let f = workspace::resume(&s).unwrap().unwrap();
    assert_eq!(
        workspace::load(&s).unwrap().unwrap().view.active,
        expected[9999].0
    );
    scan_directory(&s, &source, f.id, 0, &Channel::new(|_| Ok(()))).unwrap();
    assert_eq!(snapshot(&s), expected);
    let c = s.catalog.lock().unwrap();
    let roots = s.allowed.lock().unwrap().clone();
    let targets: Vec<_> = expected[3..6]
        .iter()
        .map(|r| exports::Target {
            id: r.0.clone(),
            revision: r.4,
        })
        .collect();
    let out = base.join("outputs");
    let copy = exports::prepare(&c, &roots, f.id, &targets, &out, &data).unwrap();
    let options = watermark::Options {
        fallback_date: Some("2026-11-17".into()),
        output_max_edge: Some(2048),
        ..Default::default()
    };
    let mark =
        exports::prepare_watermark(&c, &roots, f.id, &targets[..1], &out, &data, &options).unwrap();
    drop(c);
    let no = AtomicBool::new(false);
    let copied = exports::run(copy, &data, &no, |_| {}, || Ok(())).unwrap();
    let marked = exports::run(mark, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!(
        (
            copied.status.as_str(),
            copied.verified,
            marked.status.as_str(),
            marked.verified
        ),
        ("completed", 3, "completed", 1)
    );
    for row in &copied.items {
        assert_eq!(
            hash(&std::path::Path::new(&copied.destination).join(&row.output)),
            hash(&source.join(&row.name))
        );
    }
    assert_eq!(snapshot(&s), expected);
    println!(
        "VOLUME_RECONNECTED {}",
        serde_json::json!({"photos":10000,"catalogUnchanged":true,"device":fs::metadata(&source).unwrap().dev(),"copies":copied.verified,"watermarks":marked.verified,"normalWorkspaceResume":true})
    );
}
