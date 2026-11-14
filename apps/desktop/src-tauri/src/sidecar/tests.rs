use super::*;
use crate::imaging;
use std::fs;

#[test]
fn rescanning_detects_external_changes_even_when_the_catalog_was_clean() {
    let mut f = Fixture::new(Some(&packet(Some(3))));
    assert_eq!(report(&f.c, &f.roots, f.folder).unwrap().pending, 0);
    fs::write(&f.sidecar, packet(Some(4))).unwrap();
    let observed = capture(
        &f.roots[&f.folder],
        &f.source,
        &imaging::identity(&f.source).unwrap(),
    );
    remember(&f.c, &f.id, &observed).unwrap();
    let report = report(&f.c, &f.roots, f.folder).unwrap();
    assert_eq!(report.pending, 1);
    assert_eq!(report.items[0].state, "conflict");
    assert_eq!(f.sync().failed, 1);
    assert_eq!(
        xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
        Some(4)
    );
}
#[test]
fn old_catalog_without_xml_baseline_must_review_different_disk_ratings() {
    let f = Fixture::new(Some(&packet(Some(3))));
    f.c.db.execute("DELETE FROM xmp_state", []).unwrap();
    fs::write(&f.sidecar, packet(Some(5))).unwrap();
    let observed = capture(
        &f.roots[&f.folder],
        &f.source,
        &imaging::identity(&f.source).unwrap(),
    );
    remember(&f.c, &f.id, &observed).unwrap();
    let report = report(&f.c, &f.roots, f.folder).unwrap();
    assert_eq!(report.pending, 1);
    assert_eq!(report.items[0].state, "conflict");
    assert!(report.items[0].review_token.is_some());
}

#[test]
fn a_rating_changed_after_review_cannot_be_silently_written() {
    let mut f = Fixture::new(None);
    f.rate(Some(5));
    let target = Target {
        id: f.id.clone(),
        revision: local(&f.c, &f.id).unwrap().revision,
    };
    f.rate(Some(0));
    let result = sync(&mut f.c, &f.roots, f.folder, &[target]).unwrap();
    assert_eq!((result.completed, result.failed), (0, 1));
    assert!(!f.sidecar.exists());
    assert_eq!(result.report.items[0].rating, Some(0));
    assert_eq!(f.sync().completed, 1);
}

#[test]
fn non_utf8_external_change_during_write_is_put_back_without_losing_its_bytes() {
    let mut f = Fixture::new(Some(&packet(Some(3))));
    f.rate(Some(5));
    let path = f.sidecar.clone();
    assert!(
        sync_one(&mut f.c, &f.roots, &f.id, &mut |at| {
            if at == Checkpoint::Staged {
                fs::write(&path, [0xff, 0xfe, 0, 1]).unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&f.sidecar).unwrap(), vec![0xff, 0xfe, 0, 1]);
    assert_eq!(
        report(&f.c, &f.roots, f.folder).unwrap().items[0].state,
        "error"
    );
}

#[test]
fn paired_external_rename_relinks_only_the_same_occurrence_and_identical_metadata() {
    let mut f = Fixture::new(Some(&packet(Some(3))));
    f.rate(Some(5));
    assert_eq!(f.sync().completed, 1);
    let renamed = f.source.with_file_name("renamed.jpg");
    let renamed_xmp = renamed.with_file_name(files::default_target(&renamed));
    fs::rename(&f.source, &renamed).unwrap();
    fs::rename(&f.sidecar, &renamed_xmp).unwrap();
    let key = imaging::identity(&renamed).unwrap();
    let obs = capture(&f.roots[&f.folder], &renamed, &key);
    let (id, rating, _) =
        f.c.upsert(f.folder, &renamed, &key, obs.rating().unwrap())
            .unwrap();
    assert_eq!(id, f.id);
    assert_eq!(rating, Some(5));
    remember(&f.c, &id, &obs).unwrap();
    f.rate(Some(0));
    assert_eq!(f.sync().completed, 1);
    assert_eq!(
        xmp::read(&fs::read_to_string(&renamed_xmp).unwrap()).unwrap(),
        Some(0)
    );
}
#[test]
fn copying_photos_with_sidecars_rebuilds_seven_ratings_in_a_fresh_catalog() {
    let mut f = Fixture::new(None);
    let copied = f.temp.path().join("copied");
    fs::create_dir(&copied).unwrap();
    let fresh = Catalog::open(&f.temp.path().join("new-computer.sqlite")).unwrap();
    let folder = fresh.folder(&copied).unwrap();
    let mut ids = HashSet::new();
    for (i, rating) in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
        .into_iter()
        .enumerate()
    {
        f.rate(rating);
        assert_eq!(f.sync().completed, 1);
        let source = copied.join(format!("copy-{i}.jpg"));
        fs::copy(&f.source, &source).unwrap();
        fs::copy(
            &f.sidecar,
            source.with_file_name(files::default_target(&source)),
        )
        .unwrap();
        let key = imaging::identity(&source).unwrap();
        let obs = capture(&copied, &source, &key);
        let (id, imported, _) = fresh
            .upsert(folder, &source, &key, obs.rating().unwrap())
            .unwrap();
        assert_eq!(imported, rating);
        assert_ne!(id, f.id);
        assert!(ids.insert(id));
    }
}

#[test]
#[ignore = "subprocess helper; launched only by process_exit_reopens_durable_write_jobs"]
fn crash_worker() {
    let Some(root) = std::env::var_os("MG_XMP_CRASH_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root).canonicalize().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("crash-fixture-marker")).unwrap(),
        "generated XMP test only"
    );
    let mut c = Catalog::open(&root.join("catalog.sqlite")).unwrap();
    let folder: i64 =
        c.db.query_row("SELECT id FROM folders LIMIT 1", [], |r| r.get(0))
            .unwrap();
    let id: String =
        c.db.query_row("SELECT id FROM assets LIMIT 1", [], |r| r.get(0))
            .unwrap();
    let source = local(&c, &id).unwrap().source;
    assert!(Path::new(&source).starts_with(root.join("generated")));
    let roots = HashMap::from([(folder, root.join("generated"))]);
    let stop = std::env::var("MG_XMP_CRASH_STEP").unwrap();
    sync_one(&mut c, &roots, &id, &mut |at| {
        let name = match at {
            Checkpoint::Staged => "staged",
            Checkpoint::PreviousSaved => "previous",
            Checkpoint::BeforePublish => "before-publish",
            Checkpoint::Published => "published",
        };
        if name == stop {
            std::process::exit(86);
        }
        Ok(())
    })
    .unwrap();
    panic!("crash checkpoint was not reached");
}
#[test]
fn process_exit_reopens_durable_write_jobs() {
    for stop in ["staged", "previous", "before-publish", "published"] {
        let old = packet(Some(3));
        let mut f = Fixture::new(Some(&old));
        f.rate(Some(0));
        fs::write(
            f.temp.path().join("crash-fixture-marker"),
            "generated XMP test only",
        )
        .unwrap();
        // Close every parent connection before the child exits without running destructors.
        f.c = Catalog::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sidecar::tests::crash_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("MG_XMP_CRASH_FIXTURE", f.temp.path())
            .env("MG_XMP_CRASH_STEP", stop)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        f.reopen();
        assert_eq!(f.sync().completed, 1);
        assert_eq!(
            xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
            Some(0)
        );
        let previous: String =
            f.c.db
                .query_row("SELECT previous FROM xmp_jobs", [], |r| r.get(0))
                .unwrap();
        assert_eq!(
            fs::read_to_string(f.source.parent().unwrap().join(previous)).unwrap(),
            old
        );
    }
}

#[test]
fn a_journaled_conflict_can_adopt_the_live_file_and_keep_recovery_records() {
    let mut f = Fixture::new(Some(&packet(Some(3))));
    f.rate(Some(5));
    let path = f.sidecar.clone();
    let external = packet(Some(1));
    assert!(
        sync_one(&mut f.c, &f.roots, &f.id, &mut |at| {
            if at == Checkpoint::BeforePublish {
                fs::write(&path, &external).unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(f.sync().failed, 1);
    let row = report(&f.c, &f.roots, f.folder).unwrap().items.remove(0);
    let batch = adopt(
        &mut f.c,
        &f.roots,
        &f.id,
        row.revision,
        row.review_token.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(batch.changes[0].after, Some(1));
    f.rate(Some(4));
    assert_eq!(f.sync().completed, 1);
    assert_eq!(
        xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
        Some(4)
    );
    let retained: i64 =
        f.c.db
            .query_row(
                "SELECT count(*) FROM xmp_jobs WHERE status='retained'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(retained, 1);
}
#[test]
fn missing_target_recovery_does_not_overwrite_a_new_file() {
    let mut f = Fixture::new(Some(&packet(Some(3))));
    f.rate(Some(5));
    assert!(
        sync_one(
            &mut f.c,
            &f.roots,
            &f.id,
            &mut |at| if at == Checkpoint::PreviousSaved {
                Err("stop".into())
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    f.c.db
        .execute("UPDATE xmp_jobs SET status='conflict'", [])
        .unwrap();
    assert!(report(&f.c, &f.roots, f.folder).unwrap().items[0].can_repair);
    fs::write(&f.sidecar, packet(Some(2))).unwrap();
    assert!(repair(&mut f.c, &f.roots, &f.id).is_err());
    assert_eq!(
        xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
        Some(2)
    );
    fs::rename(&f.sidecar, f.temp.path().join("external-preserved.xmp")).unwrap();
    repair(&mut f.c, &f.roots, &f.id).unwrap();
    assert_eq!(
        xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
        Some(3)
    );
    assert_eq!(f.sync().completed, 1);
}

struct Fixture {
    temp: tempfile::TempDir,
    c: Catalog,
    roots: HashMap<i64, PathBuf>,
    folder: i64,
    id: String,
    source: PathBuf,
    sidecar: PathBuf,
}
impl Fixture {
    fn new(xml: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let media = root.join("generated");
        fs::create_dir(&media).unwrap();
        let source = media.join("测试照片.jpg");
        image::RgbImage::from_pixel(8, 6, image::Rgb([20, 70, 40]))
            .save(&source)
            .unwrap();
        let sidecar = source.with_file_name(files::default_target(&source));
        if let Some(xml) = xml {
            fs::write(&sidecar, xml).unwrap();
        }
        let c = Catalog::open(&root.join("catalog.sqlite")).unwrap();
        let folder = c.folder(&media).unwrap();
        let key = imaging::identity(&source).unwrap();
        let observed = capture(&media, &source, &key);
        let id = c
            .upsert(folder, &source, &key, observed.rating().unwrap_or(None))
            .unwrap()
            .0;
        remember(&c, &id, &observed).unwrap();
        Self {
            temp,
            c,
            roots: HashMap::from([(folder, media)]),
            folder,
            id,
            source,
            sidecar,
        }
    }
    fn rate(&mut self, rating: Option<i32>) {
        self.c.rate(std::slice::from_ref(&self.id), rating).unwrap();
    }
    fn sync(&mut self) -> Outcome {
        let target = Target {
            id: self.id.clone(),
            revision: local(&self.c, &self.id).unwrap().revision,
        };
        sync(
            &mut self.c,
            &self.roots,
            self.folder,
            std::slice::from_ref(&target),
        )
        .unwrap()
    }
    fn reopen(&mut self) {
        self.c = Catalog::open(&self.temp.path().join("catalog.sqlite")).unwrap();
    }
}
fn packet(rating: Option<i32>) -> String {
    xmp::merge(None, rating).unwrap()
}
#[test]
fn seven_states_roundtrip_through_real_sidecars_and_keep_original_bytes() {
    let mut f = Fixture::new(None);
    let original = fs::read(&f.source).unwrap();
    for rating in [Some(0), None, Some(1), Some(2), Some(3), Some(4), Some(5)] {
        f.rate(rating);
        assert_eq!(f.sync().completed, 1);
        assert_eq!(
            xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
            rating
        );
        f.reopen();
        assert_eq!(local(&f.c, &f.id).unwrap().rating, rating);
        assert_eq!(report(&f.c, &f.roots, f.folder).unwrap().pending, 0);
    }
    assert_eq!(fs::read(&f.source).unwrap(), original);
    let recovery: Vec<String> =
        f.c.db
            .prepare("SELECT previous FROM xmp_jobs WHERE expected IS NOT NULL")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
    assert_eq!(recovery.len(), 6);
    for path in recovery {
        assert!(f.source.parent().unwrap().join(path).is_file());
    }
}
#[test]
fn unknown_fields_survive_and_external_conflict_requires_fresh_explicit_adoption() {
    let xml = "<r:RDF xmlns:r='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><r:Description xmlns:q='http://ns.adobe.com/xap/1.0/' xmlns:keep='urn:keep' q:Rating='3' keep:Caption='家庭 &amp; 山海'/></r:RDF>";
    let mut f = Fixture::new(Some(xml));
    f.rate(Some(5));
    assert_eq!(f.sync().completed, 1);
    let written = fs::read_to_string(&f.sidecar).unwrap();
    assert_eq!(written, xml.replace("Rating='3'", "Rating='5'"));
    f.rate(Some(0));
    let external = written.replace("Rating='5'", "Rating='2'");
    fs::write(&f.sidecar, &external).unwrap();
    let r = report(&f.c, &f.roots, f.folder).unwrap();
    assert_eq!(r.items[0].state, "conflict");
    assert_eq!(f.sync().failed, 1);
    assert_eq!(fs::read_to_string(&f.sidecar).unwrap(), external);
    let row = report(&f.c, &f.roots, f.folder).unwrap().items.remove(0);
    fs::write(&f.sidecar, external.replace("Rating='2'", "Rating='1'")).unwrap();
    assert!(
        adopt(
            &mut f.c,
            &f.roots,
            &f.id,
            row.revision,
            row.review_token.as_deref().unwrap()
        )
        .is_err()
    );
    let row = report(&f.c, &f.roots, f.folder).unwrap().items.remove(0);
    let batch = adopt(
        &mut f.c,
        &f.roots,
        &f.id,
        row.revision,
        row.review_token.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(batch.changes[0].after, Some(1));
    assert_eq!(report(&f.c, &f.roots, f.folder).unwrap().pending, 0);
    f.c.undo(&batch.token).unwrap();
    assert_eq!(local(&f.c, &f.id).unwrap().rating, Some(0));
    assert_eq!(f.sync().completed, 1);
    assert!(
        fs::read_to_string(&f.sidecar)
            .unwrap()
            .contains("keep:Caption='家庭 &amp; 山海'")
    );
}
#[test]
fn every_write_checkpoint_can_reopen_and_retry_without_losing_metadata() {
    for stop in [
        Checkpoint::Staged,
        Checkpoint::PreviousSaved,
        Checkpoint::BeforePublish,
        Checkpoint::Published,
    ] {
        let old = packet(Some(3));
        let mut f = Fixture::new(Some(&old));
        f.rate(Some(0));
        let result = sync_one(&mut f.c, &f.roots, &f.id, &mut |at| {
            if at == stop {
                Err("injected process interruption".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        f.reopen();
        assert_eq!(
            report(&f.c, &f.roots, f.folder).unwrap().items[0].state,
            "retry"
        );
        assert_eq!(f.sync().completed, 1);
        assert_eq!(
            xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
            Some(0)
        );
        let previous: String =
            f.c.db
                .query_row("SELECT previous FROM xmp_jobs", [], |r| r.get(0))
                .unwrap();
        assert_eq!(
            fs::read_to_string(f.source.parent().unwrap().join(previous)).unwrap(),
            old
        );
    }
}
#[test]
fn a_newer_rating_is_not_acked_by_recovery_of_an_older_job() {
    let mut f = Fixture::new(None);
    f.rate(Some(5));
    assert!(
        sync_one(
            &mut f.c,
            &f.roots,
            &f.id,
            &mut |at| if at == Checkpoint::Published {
                Err("stop".into())
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    f.reopen();
    f.rate(Some(0));
    assert_eq!(f.sync().completed, 1);
    assert_eq!(report(&f.c, &f.roots, f.folder).unwrap().pending, 1);
    assert_eq!(local(&f.c, &f.id).unwrap().rating, Some(0));
    assert_eq!(f.sync().completed, 1);
    assert_eq!(
        xmp::read(&fs::read_to_string(&f.sidecar).unwrap()).unwrap(),
        Some(0)
    );
}
#[test]
fn another_app_creating_target_in_the_gap_is_never_overwritten() {
    let old = packet(Some(3));
    let external = packet(Some(1));
    let mut f = Fixture::new(Some(&old));
    f.rate(Some(5));
    let path = f.sidecar.clone();
    assert!(
        sync_one(&mut f.c, &f.roots, &f.id, &mut |at| {
            if at == Checkpoint::BeforePublish {
                fs::write(&path, &external).unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(&f.sidecar).unwrap(), external);
    f.reopen();
    assert_eq!(f.sync().failed, 1);
    assert_eq!(fs::read_to_string(&f.sidecar).unwrap(), external);
    let previous: String =
        f.c.db
            .query_row("SELECT previous FROM xmp_jobs", [], |r| r.get(0))
            .unwrap();
    assert_eq!(
        fs::read_to_string(f.source.parent().unwrap().join(previous)).unwrap(),
        old
    );
}
#[test]
fn changed_old_file_between_precheck_and_rename_is_put_back_without_overwrite() {
    let old = packet(Some(3));
    let external = packet(Some(1));
    let mut f = Fixture::new(Some(&old));
    f.rate(Some(5));
    let path = f.sidecar.clone();
    assert!(
        sync_one(&mut f.c, &f.roots, &f.id, &mut |at| {
            if at == Checkpoint::Staged {
                fs::write(&path, &external).unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(&f.sidecar).unwrap(), external);
    assert_eq!(local(&f.c, &f.id).unwrap().rating, Some(5));
}
#[test]
fn malformed_ambiguous_and_linked_sidecars_are_blocked_and_copies_are_distinct() {
    let mut f = Fixture::new(Some("<broken>"));
    f.rate(Some(5));
    assert_eq!(
        report(&f.c, &f.roots, f.folder).unwrap().items[0].state,
        "error"
    );
    assert_eq!(f.sync().failed, 1);
    assert_eq!(fs::read_to_string(&f.sidecar).unwrap(), "<broken>");
    let g = Fixture::new(Some(&packet(Some(0))));
    let copy = g.source.with_file_name("copy.jpg");
    fs::copy(&g.source, &copy).unwrap();
    fs::copy(
        &g.sidecar,
        copy.with_file_name(files::default_target(&copy)),
    )
    .unwrap();
    let observed = capture(
        &g.roots[&g.folder],
        &copy,
        &imaging::identity(&copy).unwrap(),
    );
    let id =
        g.c.upsert(
            g.folder,
            &copy,
            &imaging::identity(&copy).unwrap(),
            observed.rating().unwrap(),
        )
        .unwrap()
        .0;
    assert_ne!(id, g.id);
    assert_eq!(local(&g.c, &id).unwrap().rating, Some(0));
    let stem = g.source.with_extension("xmp");
    fs::write(&stem, packet(Some(2))).unwrap();
    assert!(
        capture(
            &g.roots[&g.folder],
            &g.source,
            &imaging::identity(&g.source).unwrap()
        )
        .error
        .is_some()
    );
}
#[cfg(unix)]
#[test]
fn sidecar_symlinks_and_replaced_parent_directories_cannot_escape_granted_root() {
    let mut f = Fixture::new(None);
    f.rate(Some(5));
    let outside = f.temp.path().join("outside.xmp");
    fs::write(&outside, packet(Some(3))).unwrap();
    std::os::unix::fs::symlink(&outside, &f.sidecar).unwrap();
    assert_eq!(f.sync().failed, 1);
    assert_eq!(fs::read_to_string(&outside).unwrap(), packet(Some(3)));
    let root = f.roots[&f.folder].clone();
    let old = f.temp.path().join("old");
    fs::rename(&root, &old).unwrap();
    std::os::unix::fs::symlink(&old, &root).unwrap();
    assert_eq!(f.sync().failed, 1);
}
