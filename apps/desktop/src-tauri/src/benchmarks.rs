//! Explicit opt-in benchmarks. Generated files live in an owned temporary directory.
use super::*;
use std::time::Instant;
#[path = "benchmarks/batch.rs"]
mod batch;
#[cfg(target_os = "macos")]
#[path = "benchmarks/review_batch.rs"]
mod review_batch;
#[path = "benchmarks/trial.rs"]
mod trial;

#[test]
#[ignore = "30-minute generated 12MP backend soak; run explicitly on the test host"]
fn high_resolution_preview_soak() {
    high_resolution(true);
}

#[test]
#[ignore = "generated 12MP cold/warm baseline; run explicitly on the test host"]
fn high_resolution_cold_and_warm_baseline() {
    high_resolution(false);
}

fn high_resolution(soak: bool) {
    use std::{
        hash::{DefaultHasher, Hasher},
        io::Read,
        time::Duration,
    };
    fn digest(path: &Path) -> u64 {
        let mut file = fs::File::open(path).unwrap();
        let mut h = DefaultHasher::new();
        let mut buf = [0; 65536];
        loop {
            let n = file.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            h.write(&buf[..n]);
        }
        h.finish()
    }
    fn cache_bytes(path: &Path) -> u64 {
        fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum()
    }
    fn p95(mut samples: Vec<f64>) -> f64 {
        samples.sort_by(f64::total_cmp);
        samples[((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)]
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("runtime");
    fs::create_dir_all(data.join("cache")).unwrap();
    let media = root.join("generated-12mp");
    fs::create_dir(&media).unwrap();
    let mut sources = Vec::new();
    for i in 0..8u32 {
        let (w, h) = if i % 2 == 0 {
            (4032, 3024)
        } else {
            (3024, 4032)
        };
        let image = image::RgbImage::from_fn(w, h, |x, y| {
            let noise =
                (x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ i.wrapping_mul(83492791))
                    % 37;
            image::Rgb([
                ((x / 17 + y / 29 + i * 13 + noise) % 256) as u8,
                ((y / 11 + i * 31 + noise) % 256) as u8,
                ((x / 23 + y / 37 + noise) % 256) as u8,
            ])
        });
        let path = media.join(format!("generated-{i}.jpg"));
        image::codecs::jpeg::JpegEncoder::new_with_quality(fs::File::create(&path).unwrap(), 85)
            .encode_image(&image)
            .unwrap();
        sources.push((path.clone(), digest(&path)));
    }
    let state = AppState {
        catalog: Mutex::new(Catalog::open(&data.join("catalog.sqlite")).unwrap()),
        allowed: Mutex::new(HashMap::new()),
        granted_identities: Mutex::new(HashMap::new()),
        data,
        generation: AtomicU64::new(0),
        decode: Arc::new(tokio::sync::Semaphore::new(2)),
        queue: Arc::new(tokio::sync::Semaphore::new(24)),
        relocation: Mutex::new(None),
        export_plan: Mutex::new(None),
        export_busy: std::sync::atomic::AtomicBool::new(false),
        export_cancel: std::sync::atomic::AtomicBool::new(false),
        watermark_render: Arc::new(tokio::sync::Semaphore::new(1)),
        move_plan: std::sync::Mutex::new(None),
        move_busy: std::sync::atomic::AtomicBool::new(false),
        move_cancel: std::sync::atomic::AtomicBool::new(false),
    };
    let folder = grant(&state, media.clone()).unwrap();
    scan_directory(&state, &media, folder.id, 0, &Channel::new(|_| Ok(()))).unwrap();
    let ids: Vec<String> = {
        let c = state.catalog.lock().unwrap();
        let mut q = c.db.prepare("SELECT id FROM assets ORDER BY path").unwrap();
        q.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    };
    assert_eq!(ids.len(), 8);
    let mut cold_thumb = Vec::new();
    let mut cold_preview = Vec::new();
    let mut warm_preview = Vec::new();
    for id in &ids {
        let start = Instant::now();
        imaging::render(&state, id, 480).unwrap();
        cold_thumb.push(start.elapsed().as_secs_f64() * 1000.);
        let start = Instant::now();
        imaging::render(&state, id, 1600).unwrap();
        cold_preview.push(start.elapsed().as_secs_f64() * 1000.);
    }
    for i in 0..80 {
        let start = Instant::now();
        imaging::render(&state, &ids[i % 8], 1600).unwrap();
        warm_preview.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let start = Instant::now();
    imaging::render(&state, &ids[0], 8192).unwrap();
    let detail = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "HD_BASELINE {}",
        serde_json::json!({"files":8,"dimensions":[[4032,3024],[3024,4032]],"build":if cfg!(debug_assertions){"debug"}else{"release"},"cold_thumb_p95_ms":p95(cold_thumb),"cold_preview_p95_ms":p95(cold_preview),"rust_warm_preview_p95_ms":p95(warm_preview),"cold_detail_ms":detail,"scope":"backend only; two decode workers during soak; no WebView/GPU/IPC"})
    );
    if !soak {
        for (path, before) in sources {
            assert_eq!(digest(&path), before, "Generated original changed");
        }
        return;
    }
    let start = Instant::now();
    let duration = Duration::from_secs(1800);
    let mut cycles = 0;
    let mut last_cold = Instant::now();
    let mut last_report = Instant::now();
    while start.elapsed() < duration {
        if last_cold.elapsed() >= Duration::from_secs(60) {
            imaging::prune_cache(&state.data.join("cache"), 0).unwrap();
            last_cold = Instant::now();
        }
        for pair in ids.chunks(2) {
            std::thread::scope(|scope| {
                let handles: Vec<_> = pair
                    .iter()
                    .map(|id| scope.spawn(|| imaging::render(&state, id, 1600).unwrap()))
                    .collect();
                // Compressed preview bodies are held together for an eight-pane simulation below.
                for h in handles {
                    assert!(!h.join().unwrap().is_empty());
                }
            });
        }
        let previews: Vec<_> = ids
            .iter()
            .map(|id| imaging::render(&state, id, 1600).unwrap())
            .collect();
        assert_eq!(previews.len(), 8);
        drop(previews);
        if cycles % 10 == 0 {
            imaging::render(&state, &ids[cycles % 8], 8192).unwrap();
        }
        let bytes = cache_bytes(&state.data.join("cache"));
        assert!(bytes <= 256 * 1024 * 1024);
        cycles += 1;
        if last_report.elapsed() >= Duration::from_secs(30) {
            println!(
                "SOAK_PROGRESS {}",
                serde_json::json!({"seconds":start.elapsed().as_secs_f64(),"cycles":cycles,"cache_bytes":bytes})
            );
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    for (path, before) in sources {
        assert_eq!(digest(&path), before, "Generated original changed");
    }
    println!(
        "HD_SOAK {}",
        serde_json::json!({"seconds":start.elapsed().as_secs_f64(),"cycles":cycles,"files":8,"cache_bytes":cache_bytes(&state.data.join("cache")),"original_checksums_unchanged":true,"scope":"backend only; not native GUI or total app memory"})
    );
}

#[test]
#[ignore = "generates and scans 10,000 files; run explicitly"]
fn generated_library_scan_and_warm_cache() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("runtime");
    fs::create_dir_all(data.join("cache")).unwrap();
    let media = temp.path().join("generated");
    fs::create_dir(&media).unwrap();
    let image = image::RgbImage::from_pixel(128, 96, image::Rgb([48, 98, 72]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image)
        .unwrap();
    for i in 0..10000 {
        fs::write(media.join(format!("generated-{i:05}.jpg")), &jpeg).unwrap();
    }
    let state = AppState {
        catalog: Mutex::new(Catalog::open(&data.join("catalog.sqlite")).unwrap()),
        allowed: Mutex::new(HashMap::new()),
        granted_identities: Mutex::new(HashMap::new()),
        data,
        generation: AtomicU64::new(1),
        decode: Arc::new(tokio::sync::Semaphore::new(2)),
        queue: Arc::new(tokio::sync::Semaphore::new(24)),
        relocation: Mutex::new(None),
        export_plan: Mutex::new(None),
        export_busy: std::sync::atomic::AtomicBool::new(false),
        export_cancel: std::sync::atomic::AtomicBool::new(false),
        watermark_render: Arc::new(tokio::sync::Semaphore::new(1)),
        move_plan: std::sync::Mutex::new(None),
        move_busy: std::sync::atomic::AtomicBool::new(false),
        move_cancel: std::sync::atomic::AtomicBool::new(false),
    };
    let folder = grant(&state, media).unwrap();
    let first_batch = Arc::new(Mutex::new(None));
    let batches = Arc::new(Mutex::new(0usize));
    let first = first_batch.clone();
    let count = batches.clone();
    let start = Instant::now();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(text) = body {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            if value["kind"] == "batch" {
                let mut first = first.lock().unwrap();
                if first.is_none() {
                    *first = Some(start.elapsed());
                }
                *count.lock().unwrap() += value["items"].as_array().unwrap().len();
            }
        }
        Ok(())
    });
    let root = state.allowed.lock().unwrap()[&folder.id].clone();
    scan_directory(&state, &root, folder.id, 1, &channel).unwrap();
    let cold = start.elapsed();
    assert_eq!(*batches.lock().unwrap(), 10000);
    let start = Instant::now();
    scan_directory(&state, &root, folder.id, 1, &Channel::new(|_| Ok(()))).unwrap();
    let rescan = start.elapsed();
    let id: String = state
        .catalog
        .lock()
        .unwrap()
        .db
        .query_row("SELECT id FROM assets LIMIT 1", [], |r| r.get(0))
        .unwrap();
    imaging::render(&state, &id, 480).unwrap();
    let mut samples = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        imaging::render(&state, &id, 480).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "BENCHMARK {}",
        serde_json::json!({"files":10000,"source_dimensions":[128,96],"first_batch_ms":first_batch.lock().unwrap().unwrap().as_secs_f64()*1000.,"cold_scan_seconds":cold.as_secs_f64(),"rescan_seconds":rescan.as_secs_f64(),"rust_warm_cache_p95_ms":samples[94],"scope":"backend only; no WebView/GPU/frame-time claim"})
    );
}
