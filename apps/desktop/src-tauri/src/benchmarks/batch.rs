//! One bounded, opt-in acceptance workload. Never reads an existing media library.
use crate::{
    catalog::Catalog, exports, imaging, moves, preparation::Review, sidecar, watermark, xmp,
};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

#[test]
#[ignore = "Explicit 100-photo generated batch acceptance; run on the designated test host"]
fn hundred_photo_workflow() {
    let all_started = Instant::now();
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("sources");
    let output = base.join("output");
    let moved = base.join("moved");
    let data = base.join("runtime");
    for p in [&source, &output, &moved, &data] {
        fs::create_dir(p).unwrap();
    }
    let pixels = image::RgbImage::from_fn(4032, 3024, |x, y| {
        image::Rgb([
            ((x * 13 + y * 7) % 256) as u8,
            ((x * 3 + y * 11) % 256) as u8,
            ((x * 17 + y * 19) % 256) as u8,
        ])
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88)
        .encode_image(&pixels)
        .unwrap();
    drop(pixels);
    let expected = exports::files::hash_bytes(&jpeg);
    let original_bytes = jpeg.len() as u64 * 100;
    assert!(
        exports::files::Dir::open(&base)
            .unwrap()
            .available()
            .unwrap()
            > original_bytes * 3 + 128 * 1024 * 1024
    );
    let mut catalog = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let folder = catalog.folder(&source).unwrap();
    let roots = HashMap::from([(folder, source.clone())]);
    let mut copy_targets = Vec::new();
    let mut move_targets = Vec::new();
    let mut baseline = Vec::new();
    let mut sidecars = Vec::new();
    for n in 0..100 {
        let name = format!("generated-{n:03}.jpg");
        let p = source.join(&name);
        fs::write(&p, &jpeg).unwrap();
        if n % 10 == 0 {
            let xml=xmp::merge(Some("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:qa=\"urn:generated-qa\" qa:Keep=\"original field\"/></rdf:RDF></x:xmpmeta>"),None).unwrap();
            let name = format!("{name}.xmp");
            fs::write(source.join(&name), &xml).unwrap();
            sidecars.push((name, exports::files::hash_bytes(xml.as_bytes())));
        }
        let key = imaging::identity(&p).unwrap();
        let id = catalog.upsert(folder, &p, &key, None).unwrap().0;
        sidecar::remember(&catalog, &id, &sidecar::capture(&source, &p, &key)).unwrap();
        let rating = [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)][n % 7];
        catalog.rate(std::slice::from_ref(&id), rating).unwrap();
        let revision = catalog.location(&id).unwrap().2;
        baseline.push((id.clone(), name, key, rating, revision));
        copy_targets.push(exports::Target {
            id: id.clone(),
            revision,
        });
        move_targets.push(moves::Target { id, revision });
    }
    let pending_before: i64 = catalog
        .db
        .query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
        .unwrap();
    let mut timings = serde_json::Map::new();
    let mut max_gap_ms = 0.0f64;
    let stopped = AtomicBool::new(false);
    let started = Instant::now();
    assert!(
        exports::prepare_with_progress(
            &catalog,
            &roots,
            folder,
            &copy_targets,
            &output,
            &data,
            None,
            &mut Review::new(&stopped, |p| {
                if p.checked == 1 {
                    stopped.store(true, Ordering::SeqCst);
                }
            })
        )
        .is_err()
    );
    timings.insert(
        "cancel_copy_review_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    let no = AtomicBool::new(false);
    let started = Instant::now();
    let mut last = Instant::now();
    let mut observe = |p: crate::preparation::Progress| {
        assert!(p.total == 100 && p.checked <= 100);
        max_gap_ms = max_gap_ms.max(last.elapsed().as_secs_f64() * 1000.0);
        last = Instant::now();
    };

    eprintln!(
        "BATCH_PHASE copy_review {}",
        all_started.elapsed().as_secs_f64()
    );
    let plan = exports::prepare_with_progress(
        &catalog,
        &roots,
        folder,
        &copy_targets,
        &output,
        &data,
        None,
        &mut Review::new(&no, &mut observe),
    )
    .unwrap();
    timings.insert(
        "copy_review_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    let stop = AtomicBool::new(false);
    let r = exports::run(
        plan,
        &data,
        &stop,
        |r| {
            if r.verified == 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!((r.status.as_str(), r.verified), ("cancelled", 1));
    let copy_id = r.id.clone();
    drop(catalog);
    catalog = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let started = Instant::now();
    let plan = exports::resume_with_progress(
        &catalog,
        &roots,
        folder,
        &copy_id,
        &output,
        &data,
        &mut Review::new(&no, |p| assert_eq!(p.total, 100)),
    )
    .unwrap();
    let r = exports::run(plan, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!((r.status.as_str(), r.verified), ("completed", 100));
    timings.insert(
        "copy_resume_execute_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    eprintln!(
        "BATCH_PHASE verify_copies {}",
        all_started.elapsed().as_secs_f64()
    );
    for row in &r.items {
        assert_eq!(
            exports::files::hash_bytes(
                &fs::read(Path::new(&r.destination).join(&row.output)).unwrap()
            ),
            expected
        );
        assert_eq!(
            xmp::read(
                &fs::read_to_string(Path::new(&r.destination).join(format!("{}.xmp", row.output)))
                    .unwrap()
            )
            .unwrap(),
            row.rating
        );
    }
    eprintln!(
        "BATCH_PHASE watermark_review {}",
        all_started.elapsed().as_secs_f64()
    );
    let options = watermark::Options {
        fallback_date: Some("2026-11-10".into()),
        city: "温州".into(),
        ..Default::default()
    };
    let started = Instant::now();
    let plan = exports::prepare_with_progress(
        &catalog,
        &roots,
        folder,
        &copy_targets,
        &output,
        &data,
        Some(&options),
        &mut Review::new(&no, |p| assert_eq!(p.total, 100)),
    )
    .unwrap();
    assert_eq!(exports::review(&plan).items.len(), 100);
    drop(plan);
    timings.insert(
        "watermark_review_only_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    let started = Instant::now();
    eprintln!(
        "BATCH_PHASE move_review {}",
        all_started.elapsed().as_secs_f64()
    );
    let plan = moves::prepare_with_progress(
        &catalog,
        &roots,
        folder,
        &move_targets,
        &moved,
        &data,
        "move",
        &mut Review::new(&no, |p| assert_eq!(p.total, 100)),
    )
    .unwrap();
    timings.insert(
        "move_review_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    let stop = AtomicBool::new(false);
    let r = moves::run(
        &mut catalog,
        plan,
        &data,
        &stop,
        |r| {
            if r.verified == 37 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!((r.status.as_str(), r.verified), ("paused", 37));
    assert!(moves::access(&data, false).is_err());
    let move_id = r.id;
    drop(catalog);
    catalog = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let started = Instant::now();
    let plan = moves::resume_with_progress(
        &catalog,
        &data,
        &move_id,
        false,
        &mut Review::new(&no, |p| assert_eq!(p.total, 100)),
    )
    .unwrap();
    let r = moves::run(&mut catalog, plan, &data, &no, |_| {}, |_| Ok(())).unwrap();
    assert_eq!((r.status.as_str(), r.verified), ("completed", 100));
    timings.insert(
        "move_resume_execute_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    let occupied = source.join("generated-000.jpg");
    fs::write(&occupied, b"Generated occupancy sentinel to preserve").unwrap();
    assert!(
        moves::resume_with_progress(
            &catalog,
            &data,
            &move_id,
            true,
            &mut Review::new(&no, |_| {})
        )
        .is_err()
    );
    let kept = base.join("occupied-file-kept.txt");
    fs::rename(&occupied, &kept).unwrap();
    let started = Instant::now();
    let plan = moves::resume_with_progress(
        &catalog,
        &data,
        &move_id,
        true,
        &mut Review::new(&no, |p| assert_eq!(p.total, 100)),
    )
    .unwrap();
    let r = moves::run(&mut catalog, plan, &data, &no, |_| {}, |_| Ok(())).unwrap();
    assert_eq!((r.status.as_str(), r.verified), ("restored", 100));
    timings.insert(
        "withdraw_review_execute_ms".into(),
        serde_json::json!(started.elapsed().as_secs_f64() * 1000.0),
    );
    assert_eq!(fs::read_dir(&r.destination).unwrap().count(), 0);
    assert_eq!(
        fs::read(kept).unwrap(),
        b"Generated occupancy sentinel to preserve"
    );
    for (id, name, key, rating, revision) in &baseline {
        let (f, p, rev, k) = catalog.location(id).unwrap();
        assert_eq!((f, rev, k), (folder, *revision, key.clone()));
        assert_eq!(Path::new(&p), source.join(name));
        assert_eq!(imaging::identity(Path::new(&p)).unwrap(), *key);
        assert_eq!(exports::files::hash_bytes(&fs::read(p).unwrap()), expected);
        assert_eq!(
            catalog
                .db
                .query_row("SELECT rating FROM assets WHERE id=?1", [id], |r| r
                    .get::<_, Option<i32>>(0))
                .unwrap(),
            *rating
        );
    }
    for (name, hash) in sidecars {
        assert_eq!(
            exports::files::hash_bytes(&fs::read(source.join(name)).unwrap()),
            hash
        );
    }
    assert_eq!(
        catalog
            .db
            .query_row("SELECT count(*) FROM pending_xmp", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        pending_before
    );
    assert_eq!(
        catalog
            .db
            .query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        100
    );
    assert!(
        !fs::read_dir(&source).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with('.'))
    );
    // Check the hash reader's cancellation boundary without timing thread scheduling.
    struct CancelReader<'a> {
        calls: usize,
        cancel: &'a AtomicBool,
    }
    impl Read for CancelReader<'_> {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.calls += 1;
            self.cancel.store(true, Ordering::SeqCst);
            b.fill(7);
            Ok(b.len())
        }
    }
    let flag = AtomicBool::new(false);
    let mut reader = CancelReader {
        calls: 0,
        cancel: &flag,
    };
    assert!(exports::files::digest(&mut reader, &flag).is_err());
    assert_eq!(reader.calls, 1);
    println!(
        "BATCH_PERF {}",
        serde_json::json!({"photos":100,"width":4032,"height":3024,"source_bytes":original_bytes,"build":if cfg!(debug_assertions){"debug"}else{"release"},"copy_pause_count":1,"move_pause_count":37,"verified_copies":100,"verified_restored":100,"pending_before":pending_before,"pending_after":pending_before,"watermark_prepared_only":true,"originals_ids_ratings_and_sidecars_unchanged":true,"max_copy_prepare_event_gap_ms":max_gap_ms,"timings":timings,"seconds":all_started.elapsed().as_secs_f64()})
    );
}

#[test]
#[ignore = "Explicit 100-photo 12MP native watermark generation; designated test host only"]
fn hundred_watermark_derivatives() {
    let started = Instant::now();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("sources");
    let output = root.join("outputs");
    let data = root.join("runtime");
    for p in [&source, &output, &data] {
        fs::create_dir(p).unwrap();
    }
    eprintln!(
        "WATERMARK_PHASE fixtures {}",
        started.elapsed().as_secs_f64()
    );
    let variants: Vec<_> = (1..=8)
        .map(|o| {
            watermark::tests::fixture(
                4032,
                3024,
                o,
                if o == 8 {
                    None
                } else {
                    Some("2026:10:25 23:59:58")
                },
            )
        })
        .collect();
    let mut c = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&source).unwrap();
    let roots = HashMap::from([(folder, source.clone())]);
    let mut targets = Vec::new();
    let mut originals = Vec::new();
    for n in 0..100 {
        let p = source.join(format!("generated-{n:03}.jpg"));
        fs::write(&p, &variants[n % 8]).unwrap();
        let identity = imaging::identity(&p).unwrap();
        let id = c.upsert(folder, &p, &identity, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&source, &p, &identity)).unwrap();
        let rating = [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)][n % 7];
        c.rate(std::slice::from_ref(&id), rating).unwrap();
        let revision = c.location(&id).unwrap().2;
        targets.push(exports::Target {
            id: id.clone(),
            revision,
        });
        originals.push((
            p,
            identity,
            id,
            rating,
            revision,
            exports::files::hash_bytes(&variants[n % 8]),
        ));
    }
    drop(variants);
    eprintln!(
        "WATERMARK_PHASE preparation {}",
        started.elapsed().as_secs_f64()
    );
    let no = AtomicBool::new(false);
    let options = watermark::Options {
        city: "温州".into(),
        fallback_date: Some("2026-11-10".into()),
        ..Default::default()
    };
    let plan = exports::prepare_with_progress(
        &c,
        &roots,
        folder,
        &targets,
        &output,
        &data,
        Some(&options),
        &mut Review::new(&no, |_| {}),
    )
    .unwrap();
    let stop = AtomicBool::new(false);
    eprintln!("WATERMARK_PHASE render {}", started.elapsed().as_secs_f64());
    let first = exports::run(
        plan,
        &data,
        &stop,
        |r| {
            if r.verified == 37 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!((first.status.as_str(), first.verified), ("cancelled", 37));
    drop(c);
    c = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let plan = exports::resume_with_progress(
        &c,
        &roots,
        folder,
        &first.id,
        &output,
        &data,
        &mut Review::new(&no, |_| {}),
    )
    .unwrap();
    let done = exports::run(plan, &data, &no, |_| {}, || Ok(())).unwrap();
    assert_eq!((done.status.as_str(), done.verified), ("completed", 100));
    let destination = Path::new(&done.destination);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("grove-export.json")).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 100);
    for (n, row) in done.items.iter().enumerate() {
        let bytes = fs::read(destination.join(&row.output)).unwrap();
        assert_eq!(
            exports::files::hash_bytes(&bytes),
            files[n]["sha256"].as_str().unwrap()
        );
        let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(
            dimensions,
            if n % 8 >= 4 {
                (3024, 4032)
            } else {
                (4032, 3024)
            }
        );
        let exif = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&bytes));
        if let Ok(exif) = exif {
            assert!(
                exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                    .is_none()
            );
            assert!(
                exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
                    .is_none()
            );
        }
        let date = if n % 8 == 7 {
            "2026-11-10"
        } else {
            "2026-10-25"
        };
        assert_eq!(
            row.caption.as_deref(),
            Some(format!("{date} 温州").as_str())
        );
        let xml = fs::read_to_string(destination.join(format!("{}.xmp", row.output))).unwrap();
        assert_eq!(xmp::read(&xml).unwrap(), originals[n].3);
        assert_eq!(
            exports::files::hash_bytes(xml.as_bytes()),
            files[n]["sidecarSha256"].as_str().unwrap()
        );
    }
    assert_eq!(fs::read_dir(destination).unwrap().count(), 201);
    for (p, key, id, rating, revision, hash) in originals {
        assert_eq!(exports::files::hash_bytes(&fs::read(&p).unwrap()), hash);
        assert_eq!(imaging::identity(&p).unwrap(), key);
        assert_eq!(
            c.location(&id).unwrap(),
            (folder, p.to_string_lossy().into_owned(), revision, key)
        );
        assert_eq!(
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&id], |r| r
                .get::<_, Option<i32>>(0))
                .unwrap(),
            rating
        );
    }
    let pending: i64 =
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
            .unwrap();
    assert_eq!(pending, 100);
    assert_eq!(fs::read_dir(source).unwrap().count(), 100);
    println!(
        "WATERMARK_BATCH {}",
        serde_json::json!({"photos":100,"source_width":4032,"source_height":3024,"orientations":8,"pause_at":37,"verified":100,"outputs":201,"originals_ids_ratings_pending_unchanged":true,"metadata_removed":true,"seconds":started.elapsed().as_secs_f64()})
    );
}
