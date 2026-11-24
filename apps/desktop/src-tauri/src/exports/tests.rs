use super::*;
use crate::{catalog::Catalog, imaging, xmp};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn fixture(base: &Path) -> (Catalog, HashMap<i64, std::path::PathBuf>, i64, Vec<Target>) {
    let root = base.join("sources");
    fs::create_dir(&root).unwrap();
    let mut c = Catalog::open(&base.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&root).unwrap();
    let mut targets = Vec::new();
    for (i, rating) in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
        .into_iter()
        .enumerate()
    {
        let p = root.join(format!("generated-{i}.jpg"));
        fs::write(&p, format!("generated original {i}")).unwrap();
        let id = c
            .upsert(folder, &p, &imaging::identity(&p).unwrap(), None)
            .unwrap()
            .0;
        let change = c.rate(std::slice::from_ref(&id), rating).unwrap();
        let revision = change.changes.first().map(|a| a.revision).unwrap_or(0);
        targets.push(Target { id, revision });
    }
    fs::create_dir(base.join("output")).unwrap();
    fs::create_dir(base.join("data")).unwrap();
    (c, HashMap::from([(folder, root)]), folder, targets)
}
fn execute(plan: Plan, data: &Path, cancel: &AtomicBool) -> Report {
    run(plan, data, cancel, |_| {}, || Ok(())).unwrap()
}

#[test]
fn opening_export_requires_a_completed_matching_journal_and_original_directory() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let (catalog, roots, folder, targets) = fixture(&base);
    let data = base.join("data");
    let plan = prepare(
        &catalog,
        &roots,
        folder,
        &targets[..2],
        &base.join("output"),
        &data,
    )
    .unwrap();
    let id = review(&plan).id;
    assert!(completed_destination(&data, folder, &id).is_err());
    assert!(completed_destination(&data, folder, "../../sources").is_err());
    let done = execute(plan, &data, &AtomicBool::new(false));
    let destination = completed_destination(&data, folder, &id).unwrap();
    assert_eq!(destination.to_string_lossy(), done.destination);
    assert!(completed_destination(&data, folder + 1, &id).is_err());
    let expected_count = fs::read_dir(&destination).unwrap().count();
    let retained = base.join("retained-output");
    fs::rename(&destination, &retained).unwrap();
    fs::create_dir(&destination).unwrap();
    assert!(completed_destination(&data, folder, &id).is_err());
    fs::rename(&destination, base.join("replacement-output")).unwrap();
    std::os::unix::fs::symlink(&retained, &destination).unwrap();
    assert!(completed_destination(&data, folder, &id).is_err());
    assert_eq!(fs::read_dir(&retained).unwrap().count(), expected_count);
}

#[cfg(target_os = "macos")]
#[test]
fn legacy_export_resumes_after_online_catalog_identity_upgrade() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let data = b.join("data");
    let parent = b.join("output");
    let legacy = |p: &Path| imaging::identity_from_metadata(&fs::metadata(p).unwrap()).unwrap();
    let plan = prepare(&c, &roots, f, &targets, &parent, &data).unwrap();
    let cancel = AtomicBool::new(false);
    let first = run(
        plan,
        &data,
        &cancel,
        |r| {
            if r.verified == 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!((first.status.as_str(), first.verified), ("cancelled", 1));
    // Recreate the unchanged version1 journal layout written by0.7.0.
    let journal = data.join("exports").join(&first.id);
    let plan_path = journal.join("plan.json");
    let mut job: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    job["parent_key"] = legacy(&parent).into();
    for item in job["items"].as_array_mut().unwrap() {
        let path = Path::new(item["path"].as_str().unwrap());
        let key = legacy(path);
        c.db.execute(
            "UPDATE assets SET identity=?1 WHERE id=?2",
            rusqlite::params![key, item["id"].as_str().unwrap()],
        )
        .unwrap();
        item["identity"] = key.into();
    }
    fs::write(plan_path, serde_json::to_vec(&job).unwrap()).unwrap();
    let checkpoint = fs::read_dir(&journal)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("state-")
        })
        .max()
        .unwrap();
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    state["directory_key"] = legacy(Path::new(&first.destination)).into();
    fs::write(checkpoint, serde_json::to_vec(&state).unwrap()).unwrap();
    for target in &targets {
        let (_, path, _, _) = c.location(&target.id).unwrap();
        let path = Path::new(&path);
        assert_eq!(
            c.upsert(f, path, &imaging::identity(path).unwrap(), None)
                .unwrap()
                .0,
            target.id
        );
    }
    let original = roots[&f].join("generated-1.jpg");
    let retained = roots[&f].join("retained-original");
    let bytes = fs::read(&original).unwrap();
    fs::rename(&original, &retained).unwrap();
    fs::write(&original, &bytes).unwrap();
    assert!(resume(&c, &roots, f, &first.id, &parent, &data).is_err());
    fs::rename(&original, roots[&f].join("retained-replacement")).unwrap();
    fs::rename(retained, original).unwrap();
    let existing = Path::new(&first.destination).join(&first.items[0].output);
    let meta = fs::metadata(&existing).unwrap();
    let second = execute(
        resume(&c, &roots, f, &first.id, &parent, &data).unwrap(),
        &data,
        &AtomicBool::new(false),
    );
    assert_eq!((second.status.as_str(), second.verified), ("completed", 7));
    use std::os::unix::fs::MetadataExt;
    assert_eq!(fs::metadata(existing).unwrap().ino(), meta.ino());
    for target in &targets {
        assert!(c.location(&target.id).unwrap().3.starts_with("volume:"));
    }
}

#[test]
fn less_common_raw_companions_are_not_split_by_copy_export() {
    for ext in ["PEF", "SRW", "RW2"] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (c, roots, f, targets) = fixture(&b);
        fs::write(
            roots[&f].join(format!("generated-0.{ext}")),
            b"generated RAW marker",
        )
        .unwrap();
        assert!(
            prepare(
                &c,
                &roots,
                f,
                &targets[..1],
                &b.join("output"),
                &b.join("data")
            )
            .is_err(),
            "{ext} must not be split"
        );
        assert_eq!(fs::read_dir(b.join("output")).unwrap().count(), 0);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn paired_static_watermark_keeps_original_photo_and_video_in_place() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let photo = roots[&f].join("generated-0.jpg");
    image::RgbImage::new(240, 160).save(&photo).unwrap();
    let before = fs::read(&photo).unwrap();
    let video = roots[&f].join("generated-0.MOV");
    fs::write(
        &video,
        b"generated companion marker; not a real Live Photo video",
    )
    .unwrap();
    let options = crate::watermark::Options {
        fallback_date: Some("2026-11-17".into()),
        ..Default::default()
    };
    assert!(
        prepare(
            &c,
            &roots,
            f,
            &targets[..1],
            &b.join("output"),
            &b.join("data")
        )
        .is_err()
    );
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[..1],
        &b.join("output"),
        &b.join("data"),
        &options,
    )
    .unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!((result.status.as_str(), result.verified), ("completed", 1));
    let generated = fs::read(Path::new(&result.destination).join(&result.items[0].output)).unwrap();
    assert_eq!(image::load_from_memory(&generated).unwrap().width(), 240);
    assert_eq!(fs::read(&photo).unwrap(), before);
    assert_eq!(
        fs::read(&video).unwrap(),
        b"generated companion marker; not a real Live Photo video"
    );
}

#[test]
fn seven_states_export_without_touching_originals_or_pending_queue() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let xml = xmp::merge(None, Some(2)).unwrap().replace(
        "<rdf:Description ",
        "<rdf:Description xmlns:custom=\"urn:test\" custom:Caption=\"keep me\" ",
    );
    fs::write(roots[&f].join("generated-6.jpg.xmp"), &xml).unwrap();
    let before: Vec<_> = (0..7)
        .map(|i| fs::read(roots[&f].join(format!("generated-{i}.jpg"))).unwrap())
        .collect();
    let plan = prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).unwrap();
    let report = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(report.status, "completed");
    assert_eq!(report.verified, 7);
    for (i, row) in report.items.iter().enumerate() {
        assert_eq!(
            fs::read(Path::new(&report.destination).join(&row.output)).unwrap(),
            before[i]
        );
        let out =
            fs::read_to_string(Path::new(&report.destination).join(format!("{}.xmp", row.output)))
                .unwrap();
        assert_eq!(xmp::read(&out).unwrap(), row.rating);
        assert_eq!(
            fs::read(roots[&f].join(format!("generated-{i}.jpg"))).unwrap(),
            before[i]
        );
        if i == 6 {
            assert!(out.contains("keep me"));
        }
    }
    assert_eq!(
        fs::read_to_string(roots[&f].join("generated-6.jpg.xmp")).unwrap(),
        xml
    );
    let pending: i64 =
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
            .unwrap();
    assert_eq!(pending, 7);
    let manifest =
        fs::read_to_string(Path::new(&report.destination).join("grove-export.json")).unwrap();
    assert!(!manifest.contains(b.to_str().unwrap()));
    assert!(manifest.contains("sha256"));
}

#[test]
fn rejects_stale_selection_source_changes_and_nested_destination() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    assert!(prepare(&c, &roots, f, &targets, &roots[&f], &b.join("data")).is_err());
    assert!(
        prepare(
            &c,
            &HashMap::new(),
            f,
            &targets,
            &b.join("output"),
            &b.join("data")
        )
        .is_err()
    );
    c.rate(&[targets[0].id.clone()], Some(5)).unwrap();
    assert!(prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).is_err());
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[1..2],
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    fs::write(roots[&f].join("generated-1.jpg"), b"externally changed").unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.verified, 0);
    assert_eq!(result.status, "partial");
}

#[test]
fn cancellation_resumes_and_external_output_is_never_overwritten() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let plan = prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).unwrap();
    let cancel = AtomicBool::new(false);
    let first = run(
        plan,
        &b.join("data"),
        &cancel,
        |r| {
            if r.verified == 1 {
                cancel.store(true, Ordering::SeqCst)
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!(first.status, "cancelled");
    assert_eq!(first.verified, 1);
    let name = &first.items[0].output;
    let dst = Path::new(&first.destination).join(name);
    fs::write(&dst, b"external new data").unwrap();
    let plan = resume(&c, &roots, f, &first.id, &b.join("output"), &b.join("data")).unwrap();
    let second = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(second.status, "partial");
    assert_eq!(second.verified, 6);
    assert_eq!(fs::read(dst).unwrap(), b"external new data");
}

#[test]
fn paired_resources_links_and_duplicate_selection_are_rejected() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    fs::write(
        roots[&f].join("generated-0.MOV"),
        b"generated paired resource",
    )
    .unwrap();
    assert!(prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).is_err());
    assert!(
        prepare(
            &c,
            &roots,
            f,
            &[targets[1].clone(), targets[1].clone()],
            &b.join("output"),
            &b.join("data")
        )
        .is_err()
    );
    let link = roots[&f].join("linked");
    symlink(b.join("output"), &link).unwrap();
    let id =
        c.db.query_row(
            "SELECT id FROM assets WHERE path LIKE '%generated-1.jpg'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    c.db.execute(
        "UPDATE assets SET path=?1 WHERE id=?2",
        rusqlite::params![link.join("any.jpg").to_str(), id],
    )
    .unwrap();
    assert!(
        prepare(
            &c,
            &roots,
            f,
            &targets[1..2],
            &b.join("output"),
            &b.join("data")
        )
        .is_err()
    );
}

#[test]
fn crash_after_photo_publication_resumes_without_replacing_verified_bytes() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[5..],
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let id = review(&plan).id;
    // Persist the prepared job and exit in an actual child after the first photo is published.
    stage_for_crash_test(&plan, &b.join("data")).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exports::tests::crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MG_EXPORT_CRASH_ROOT", &b)
        .env("MG_EXPORT_CRASH_ID", &id)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(91));
    let plan = resume(&c, &roots, f, &id, &b.join("output"), &b.join("data")).unwrap();
    let final_report = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(final_report.status, "completed");
    assert_eq!(final_report.verified, 2);
}

#[test]
fn duplicate_basenames_are_distinct_copies_and_do_not_collide() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, _) = fixture(&b);
    let mut targets = Vec::new();
    for folder in ["a", "b"] {
        let dir = roots[&f].join(folder);
        fs::create_dir(&dir).unwrap();
        let path = dir.join("same.jpg");
        fs::write(&path, b"identical generated bytes").unwrap();
        let id = c
            .upsert(f, &path, &imaging::identity(&path).unwrap(), Some(4))
            .unwrap()
            .0;
        targets.push(Target { id, revision: 0 });
    }
    let plan = prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.verified, 2);
    assert_ne!(result.items[0].output, result.items[1].output);
    assert_eq!(result.items[0].name, "a/same.jpg");
    assert_eq!(result.items[1].name, "b/same.jpg");
}

#[test]
fn changed_sidecar_and_destination_symlink_are_preserved() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[0..1],
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let xml = xmp::merge(None, Some(3)).unwrap();
    fs::write(roots[&f].join("generated-0.jpg.xmp"), &xml).unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.status, "partial");
    assert_eq!(result.verified, 0);
    assert_eq!(
        fs::read_to_string(roots[&f].join("generated-0.jpg.xmp")).unwrap(),
        xml
    );
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[1..2],
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let preview = review(&plan);
    let outside = b.join("external.txt");
    fs::write(&outside, b"external original").unwrap();
    let result = run(
        plan,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        || {
            symlink(
                &outside,
                Path::new(&preview.destination).join(format!("{}.xmp", preview.items[0].output)),
            )
            .unwrap();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(fs::read(outside).unwrap(), b"external original");
}

#[test]
#[ignore = "Invoked only by the parent crash-recovery test with generated temporary data"]
fn crash_child() {
    let b =
        std::path::PathBuf::from(std::env::var_os("MG_EXPORT_CRASH_ROOT").expect("parent only"));
    let id = std::env::var("MG_EXPORT_CRASH_ID").unwrap();
    let c = Catalog::open(&b.join("catalog.sqlite")).unwrap();
    let f = c.folder(&b.join("sources")).unwrap();
    let roots = HashMap::from([(f, b.join("sources"))]);
    let plan = resume(&c, &roots, f, &id, &b.join("output"), &b.join("data")).unwrap();
    run(
        plan,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        || std::process::exit(91),
    )
    .unwrap();
}

#[test]
fn large_metadata_plan_reopens_and_resumes_with_compact_checkpoints() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let packet = xmp::merge(None, Some(4)).unwrap().replace(
        "<rdf:Description ",
        &format!(
            "<rdf:Description xmlns:test=\"urn:generated\" test:Caption=\"{}\" ",
            "a".repeat(700_000)
        ),
    );
    for n in 0..7 {
        fs::write(roots[&f].join(format!("generated-{n}.jpg.xmp")), &packet).unwrap();
    }
    let plan = prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).unwrap();
    let first = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(first.status, "completed");
    assert_eq!(history(&b.join("data"), f).unwrap()[0].verified, 7);
    let plan = resume(&c, &roots, f, &first.id, &b.join("output"), &b.join("data")).unwrap();
    assert_eq!(
        execute(plan, &b.join("data"), &AtomicBool::new(false)).status,
        "completed"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn watermark_export_has_current_ratings_but_removes_source_metadata() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    for n in [5, 6] {
        fs::write(
            roots[&f].join(format!("generated-{n}.jpg")),
            crate::watermark::tests::fixture(
                640,
                480,
                if n == 5 { 6 } else { 1 },
                Some("2026:10:25 23:59:59"),
            ),
        )
        .unwrap();
    }
    let original_xml=xmp::merge(None,Some(2)).unwrap().replace("<rdf:Description ","<rdf:Description xmlns:custom=\"urn:test\" custom:GPSLatitude=\"generated location\" custom:Caption=\"private source caption\" ");
    fs::write(roots[&f].join("generated-6.jpg.xmp"), &original_xml).unwrap();
    let before = fs::read(roots[&f].join("generated-5.jpg")).unwrap();
    let options = crate::watermark::Options {
        city: "温州".into(),
        ..Default::default()
    };
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[5..],
        &b.join("output"),
        &b.join("data"),
        &options,
    )
    .unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.status, "completed");
    assert_eq!(result.kind, "watermark");
    assert_eq!(result.verified, 2);
    for row in &result.items {
        let photo = fs::read(Path::new(&result.destination).join(&row.output)).unwrap();
        let xml =
            fs::read_to_string(Path::new(&result.destination).join(format!("{}.xmp", row.output)))
                .unwrap();
        assert_eq!(xmp::read(&xml).unwrap(), row.rating);
        assert!(!xml.contains("GPS") && !xml.contains("private source caption"));
        let out = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(photo));
        assert!(out.ok().is_none_or(|e| {
            e.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                .is_none()
        }));
    }
    assert_eq!(fs::read(roots[&f].join("generated-5.jpg")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(roots[&f].join("generated-6.jpg.xmp")).unwrap(),
        original_xml
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    let again = resume(
        &c,
        &roots,
        f,
        &result.id,
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let repeated = execute(again, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(
        repeated.status,
        "completed",
        "{}",
        serde_json::to_string(&repeated).unwrap()
    );
    let changed_output = Path::new(&result.destination).join(&result.items[0].output);
    fs::write(&changed_output, b"external edit to derivative").unwrap();
    let again = resume(
        &c,
        &roots,
        f,
        &result.id,
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let rejected = execute(again, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(rejected.status, "partial");
    assert_eq!(
        fs::read(changed_output).unwrap(),
        b"external edit to derivative"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn watermark_crash_after_publication_reopens_the_saved_derivative_identity() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    fs::write(
        roots[&f].join("generated-5.jpg"),
        crate::watermark::tests::fixture(640, 480, 1, Some("2026:10:25 23:59:59")),
    )
    .unwrap();
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[5..6],
        &b.join("output"),
        &b.join("data"),
        &crate::watermark::Options::default(),
    )
    .unwrap();
    let id = review(&plan).id;
    stage_for_crash_test(&plan, &b.join("data")).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exports::tests::crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MG_EXPORT_CRASH_ROOT", &b)
        .env("MG_EXPORT_CRASH_ID", &id)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(91));
    let plan = resume(&c, &roots, f, &id, &b.join("output"), &b.join("data")).unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.status, "completed");
    assert_eq!(result.verified, 1);
}

#[test]
#[cfg(target_os = "macos")]
fn sharing_crash_recovery_keeps_output_options_and_published_photo() {
    use image::GenericImageView;
    use std::os::unix::fs::MetadataExt;

    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let originals: Vec<_> = [6, 1]
        .into_iter()
        .enumerate()
        .map(|(n, orientation)| {
            let path = roots[&f].join(format!("generated-{}.jpg", n + 5));
            let bytes = crate::watermark::tests::fixture(
                3000,
                2000,
                orientation,
                Some("2026:10:25 23:59:59"),
            );
            fs::write(&path, &bytes).unwrap();
            (path, bytes)
        })
        .collect();
    let options = crate::watermark::Options {
        output_max_edge: Some(2048),
        jpeg_quality: 80,
        ..Default::default()
    };
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[5..],
        &b.join("output"),
        &b.join("data"),
        &options,
    )
    .unwrap();
    let reviewed = review(&plan);
    stage_for_crash_test(&plan, &b.join("data")).unwrap();
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(b.join("data/exports").join(&reviewed.id).join("plan.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["format"], 2);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exports::tests::crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MG_EXPORT_CRASH_ROOT", &b)
        .env("MG_EXPORT_CRASH_ID", &reviewed.id)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(91));
    let published = Path::new(&reviewed.destination).join(&reviewed.items[0].output);
    let bytes = fs::read(&published).unwrap();
    let inode = fs::metadata(&published).unwrap().ino();
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().dimensions(),
        (1365, 2048)
    );

    let plan = resume(
        &c,
        &roots,
        f,
        &reviewed.id,
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    assert_eq!(review(&plan).watermark.as_ref(), Some(&options));
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.status, "completed");
    assert_eq!(result.verified, 2);
    assert_eq!(fs::read(&published).unwrap(), bytes);
    assert_eq!(fs::metadata(&published).unwrap().ino(), inode);
    let second = Path::new(&result.destination).join(&result.items[1].output);
    assert_eq!(
        image::load_from_memory(&fs::read(second).unwrap())
            .unwrap()
            .dimensions(),
        (2048, 1365)
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(&result.destination).join("grove-export.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["watermark"]["outputMaxEdge"], 2048);
    assert_eq!(manifest["watermark"]["jpegQuality"], 80);
    for (path, bytes) in originals {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    for target in &targets {
        assert_eq!(c.location(&target.id).unwrap().2, target.revision);
    }
}

#[test]
fn pre_watermark_copy_journals_still_resume_without_migration() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[0..1],
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let result = execute(plan, &b.join("data"), &AtomicBool::new(false));
    let journal = b.join("data/exports").join(&result.id);
    let p = journal.join("plan.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("watermark");
    for item in value["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("info");
    }
    fs::write(p, serde_json::to_vec(&value).unwrap()).unwrap();
    for e in fs::read_dir(&journal).unwrap() {
        let p = e.unwrap().path();
        if p.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("state-")
        {
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
            value.as_object_mut().unwrap().remove("derivatives");
            fs::write(p, serde_json::to_vec(&value).unwrap()).unwrap();
        }
    }
    let plan = resume(
        &c,
        &roots,
        f,
        &result.id,
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    assert_eq!(
        execute(plan, &b.join("data"), &AtomicBool::new(false)).status,
        "completed"
    );
}

#[test]
fn copy_and_watermark_review_cancel_without_outputs_and_report_real_progress() {
    use crate::preparation::Review;
    for watermark in [false, true] {
        for cutoff in [0, 1, 99] {
            let t = tempfile::tempdir().unwrap();
            let b = t.path().canonicalize().unwrap();
            let (c, roots, f, targets) = fixture(&b);
            for n in 0..7 {
                image::RgbImage::new(48, 32)
                    .save(roots[&f].join(format!("generated-{n}.jpg")))
                    .unwrap();
            }
            let before: Vec<_> = (0..7)
                .map(|n| fs::read(roots[&f].join(format!("generated-{n}.jpg"))).unwrap())
                .collect();
            let pending: i64 =
                c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
                    .unwrap();
            let cancel = AtomicBool::new(cutoff == 0);
            let mut events = Vec::new();
            let options = crate::watermark::Options {
                fallback_date: Some("2026-11-10".into()),
                ..Default::default()
            };
            let result = prepare_with_progress(
                &c,
                &roots,
                f,
                &targets,
                &b.join("output"),
                &b.join("data"),
                watermark.then_some(&options),
                &mut Review::new(&cancel, |p| {
                    if p.checked == cutoff {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    events.push(p);
                }),
            );
            if cutoff < 99 {
                assert!(
                    result.is_err(),
                    "Cancelled review must not return an executable plan"
                );
            } else {
                assert_eq!(review(&result.unwrap()).items.len(), 7);
                assert_eq!(events.last().unwrap().checked, 7);
                assert!(events.iter().all(|p| p.total == 7));
            }
            assert_eq!(fs::read_dir(b.join("output")).unwrap().count(), 0);
            assert_eq!(fs::read_dir(b.join("data")).unwrap().count(), 0);
            assert_eq!(
                c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                pending
            );
            for (n, bytes) in before.iter().enumerate() {
                assert_eq!(
                    &fs::read(roots[&f].join(format!("generated-{n}.jpg"))).unwrap(),
                    bytes
                );
            }
        }
    }
}

#[test]
fn copy_resume_review_can_be_cancelled_without_changing_saved_results() {
    use crate::preparation::Review;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let p = prepare(&c, &roots, f, &targets, &b.join("output"), &b.join("data")).unwrap();
    let stop = AtomicBool::new(false);
    let r = run(
        p,
        &b.join("data"),
        &stop,
        |r| {
            if r.verified == 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!(r.status, "cancelled");
    let before = serde_json::to_string(&history(&b.join("data"), f).unwrap()).unwrap();
    for pre in [true, false] {
        let cancel = AtomicBool::new(pre);
        assert!(
            resume_with_progress(
                &c,
                &roots,
                f,
                &r.id,
                &b.join("output"),
                &b.join("data"),
                &mut Review::new(&cancel, |p| {
                    if p.checked == 1 {
                        cancel.store(true, Ordering::SeqCst);
                    }
                })
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_string(&history(&b.join("data"), f).unwrap()).unwrap(),
            before
        );
    }
    let p = resume_with_progress(
        &c,
        &roots,
        f,
        &r.id,
        &b.join("output"),
        &b.join("data"),
        &mut Review::new(&AtomicBool::new(false), |_| {}),
    )
    .unwrap();
    assert_eq!(
        execute(p, &b.join("data"), &AtomicBool::new(false)).verified,
        7
    );
}

#[test]
#[cfg(target_os = "macos")]
fn sharing_plan_preview_and_resume_preserve_dimensions_quality_and_sources() {
    use image::GenericImageView;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let source = roots[&f].join("generated-5.jpg");
    let before = crate::watermark::tests::fixture(3000, 2000, 6, Some("2026:10:25 23:59:59"));
    fs::write(&source, &before).unwrap();
    let options = crate::watermark::Options {
        output_max_edge: Some(2048),
        jpeg_quality: 80,
        ..Default::default()
    };
    let preview = preview_watermark(&c, &roots, &targets[5].id, &options).unwrap();
    assert_eq!((preview.export_width, preview.export_height), (1365, 2048));
    assert_eq!(
        preview.info.output_width.max(preview.info.output_height),
        1280
    );
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[5..6],
        &b.join("output"),
        &b.join("data"),
        &options,
    )
    .unwrap();
    let stopped = execute(plan, &b.join("data"), &AtomicBool::new(true));
    assert_eq!(stopped.status, "cancelled");
    let plan_path = b.join("data/exports").join(&stopped.id).join("plan.json");
    let original_plan = fs::read(&plan_path).unwrap();
    let mut saved: serde_json::Value = serde_json::from_slice(&original_plan).unwrap();
    assert_eq!(
        saved["format"], 2,
        "old apps only read format1 and must reject this plan"
    );
    saved["format"] = 1.into();
    fs::write(&plan_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(
        resume(
            &c,
            &roots,
            f,
            &stopped.id,
            &b.join("output"),
            &b.join("data")
        )
        .is_err()
    );
    fs::write(&plan_path, original_plan).unwrap();
    let resumed = resume(
        &c,
        &roots,
        f,
        &stopped.id,
        &b.join("output"),
        &b.join("data"),
    )
    .unwrap();
    let result = execute(resumed, &b.join("data"), &AtomicBool::new(false));
    assert_eq!(result.status, "completed");
    assert_eq!(result.verified, 1);
    assert_eq!(result.watermark.as_ref().unwrap(), &options);
    let photo = fs::read(Path::new(&result.destination).join(&result.items[0].output)).unwrap();
    assert_eq!(
        image::load_from_memory(&photo).unwrap().dimensions(),
        (1365, 2048)
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(&result.destination).join("grove-export.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["watermark"]["outputMaxEdge"], 2048);
    assert_eq!(manifest["watermark"]["jpegQuality"], 80);
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
#[cfg(target_os = "macos")]
fn legacy_watermark_plan_without_output_options_resumes_unchanged() {
    use image::GenericImageView;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    fs::write(
        roots[&f].join("generated-5.jpg"),
        crate::watermark::tests::fixture(640, 480, 6, Some("2026:10:25 23:59:59")),
    )
    .unwrap();
    let plan = prepare_watermark(
        &c,
        &roots,
        f,
        &targets[5..6],
        &b.join("output"),
        &b.join("data"),
        &Default::default(),
    )
    .unwrap();
    let stopped = execute(plan, &b.join("data"), &AtomicBool::new(true));
    let path = b.join("data/exports").join(&stopped.id).join("plan.json");
    let mut saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["format"], 1);
    saved["watermark"]
        .as_object_mut()
        .unwrap()
        .remove("outputMaxEdge");
    saved["watermark"]
        .as_object_mut()
        .unwrap()
        .remove("jpegQuality");
    fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let result = execute(
        resume(
            &c,
            &roots,
            f,
            &stopped.id,
            &b.join("output"),
            &b.join("data"),
        )
        .unwrap(),
        &b.join("data"),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, "completed");
    assert_eq!(result.watermark.unwrap().jpeg_quality, 92);
    let bytes = fs::read(Path::new(&result.destination).join(&result.items[0].output)).unwrap();
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().dimensions(),
        (480, 640)
    );
}

#[test]
fn grove_resumes_a_legacy_named_export_without_renaming_its_manifest() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let (c, roots, folder, targets) = fixture(&base);
    let data = base.join("data");
    let plan = prepare(&c, &roots, folder, &targets, &base.join("output"), &data).unwrap();
    let cancel = AtomicBool::new(false);
    let first = run(
        plan,
        &data,
        &cancel,
        |r| {
            if r.verified == 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        || Ok(()),
    )
    .unwrap();
    let output = std::path::PathBuf::from(&first.destination);
    let legacy = output.with_file_name(output.file_name().unwrap().to_str().unwrap().replacen(
        "Grove-",
        "Moment-Grove-",
        1,
    ));
    fs::rename(&output, &legacy).unwrap();
    let job_path = data.join("exports").join(&first.id).join("plan.json");
    let mut job: serde_json::Value = serde_json::from_slice(&fs::read(&job_path).unwrap()).unwrap();
    job["directory"] = legacy.file_name().unwrap().to_str().unwrap().into();
    fs::write(&job_path, serde_json::to_vec(&job).unwrap()).unwrap();
    let plan = resume(&c, &roots, folder, &first.id, &base.join("output"), &data).unwrap();
    let done = execute(plan, &data, &AtomicBool::new(false));
    assert_eq!(done.status, "completed");
    let portable: serde_json::Value =
        serde_json::from_slice(&fs::read(legacy.join("moment-grove-export.json")).unwrap())
            .unwrap();
    assert_eq!(portable["app"], "Moment Grove");
    assert!(!legacy.join("grove-export.json").exists());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Generated local fixtures; requires the Swift toolchain and ffmpeg"]
fn identified_pair_round_trip_and_conflicts() {
    use std::process::Command;
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let (c, roots, folder, _) = fixture(&base);
    let source = &roots[&folder];
    let png = base.join("seed.png");
    image::RgbImage::new(64, 64).save(&png).unwrap();
    let still = source.join("live.jpg");
    let movie = source.join("LIVE.MOV");
    let id = uuid::Uuid::new_v4().to_string();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/qa/live-photo-still.swift");
    assert!(
        Command::new("swift")
            .arg(script)
            .arg(&png)
            .arg(&still)
            .arg(&id)
            .status()
            .unwrap()
            .success()
    );
    let seed = base.join("seed.mov");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:duration=0.2",
                "-c:v",
                "mpeg4"
            ])
            .arg(&seed)
            .status()
            .unwrap()
            .success()
    );
    let make_movie = |path: &Path, value: &str| {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/qa/live-photo-movie.swift");
        assert!(
            Command::new("swift")
                .arg("-suppress-warnings")
                .arg(script)
                .arg(&seed)
                .arg(path)
                .arg(value)
                .status()
                .unwrap()
                .success()
        );
    };
    make_movie(&movie, &id);
    let asset = c
        .upsert(folder, &still, &imaging::identity(&still).unwrap(), None)
        .unwrap()
        .0;
    let targets = [Target {
        id: asset,
        revision: 0,
    }];
    let data = base.join("data");
    let parent = base.join("output");
    let plan = || prepare(&c, &roots, folder, &targets, &parent, &data);
    let first = plan().unwrap();
    let row = &review(&first).items[0];
    assert_eq!(row.motion_output.as_deref(), Some("001-live.mov"));
    // A different video with the same name must block the already reviewed job.
    let retained = base.join("retained.mov");
    fs::rename(&movie, &retained).unwrap();
    make_movie(&movie, &uuid::Uuid::new_v4().to_string());
    assert!(plan().is_err());
    let partial = execute(first, &data, &AtomicBool::new(false));
    assert_eq!(partial.verified, 0);
    assert!(!Path::new(&partial.destination).join(&row.output).exists());
    fs::rename(&movie, base.join("wrong.mov")).unwrap();
    assert!(plan().is_err());
    fs::rename(&retained, &movie).unwrap();
    let second = execute(
        resume(&c, &roots, folder, &partial.id, &parent, &data).unwrap(),
        &data,
        &AtomicBool::new(false),
    );
    assert_eq!((second.status.as_str(), second.verified), ("completed", 1));
    let dest = Path::new(&second.destination);
    assert_eq!(
        fs::read(dest.join(&row.output)).unwrap(),
        fs::read(&still).unwrap()
    );
    assert_eq!(
        fs::read(dest.join("001-live.mov")).unwrap(),
        fs::read(&movie).unwrap()
    );
    assert!(dest.join("001-live.jpg.xmp").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dest.join("grove-export.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["files"][0]["motion"]["sha256"],
        files::hash_bytes(&fs::read(&movie).unwrap())
    );
    assert!(crate::resources::refuse_incomplete_original_operation(&still, "移动").is_err());
    fs::copy(&still, source.join("live.heic")).unwrap();
    assert!(plan().is_err());
}
