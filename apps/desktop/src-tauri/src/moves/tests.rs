use super::*;
use crate::{imaging, sidecar, xmp};
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};
fn fixture(base: &Path) -> (Catalog, HashMap<i64, PathBuf>, i64, Vec<Target>) {
    let root = base.join("sources");
    fs::create_dir(&root).unwrap();
    fs::create_dir(base.join("destination")).unwrap();
    fs::create_dir(base.join("data")).unwrap();
    let mut c = Catalog::open(&base.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&root).unwrap();
    let mut targets = Vec::new();
    for (n, rating) in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
        .into_iter()
        .enumerate()
    {
        let photo = root.join(format!("generated-{n}.jpg"));
        fs::write(&photo, format!("Generated source {n}")).unwrap();
        if n % 2 == 0 {
            fs::write(root.join(format!("generated-{n}.jpg.xmp")), xmp::merge(Some("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description><custom:Note xmlns:custom=\"urn:generated-test\">Keep this field</custom:Note></rdf:Description></rdf:RDF></x:xmpmeta>"),None).unwrap()).unwrap();
        }
        let key = imaging::identity(&photo).unwrap();
        let id = c.upsert(folder, &photo, &key, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&root, &photo, &key)).unwrap();
        c.rate(std::slice::from_ref(&id), rating).unwrap();
        targets.push(Target { id, revision: 1 });
    }
    (c, HashMap::from([(folder, root)]), folder, targets)
}
fn execute(c: &mut Catalog, p: Plan, b: &Path) -> Report {
    run(
        c,
        p,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        |_| Ok(()),
    )
    .unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn legacy_move_continues_and_withdraws_after_online_identity_upgrade() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let data = b.join("data");
    let legacy = |p: &Path| imaging::identity_from_metadata(&fs::metadata(p).unwrap()).unwrap();
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets,
        &b.join("destination"),
        &data,
        "move",
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let first = run(
        &mut c,
        plan,
        &data,
        &cancel,
        |r| {
            if r.verified == 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!((first.status.as_str(), first.verified), ("paused", 1));
    // Recreate legacy directory/file keys without changing any occurrence or bytes.
    let journal = data.join("moves").join(&first.id);
    let plan_path = journal.join("plan.json");
    let mut job: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    job["root_key"] = legacy(&roots[&f]).into();
    job["parent_key"] = legacy(&b.join("destination")).into();
    for item in job["items"].as_array_mut().unwrap() {
        let original = PathBuf::from(item["path"].as_str().unwrap());
        item["parent_key"] = legacy(original.parent().unwrap()).into();
        for resource in item["resources"].as_array_mut().unwrap() {
            let name = resource["name"].as_str().unwrap();
            let source = original.parent().unwrap().join(name);
            let actual = if source.exists() {
                source
            } else {
                Path::new(&first.destination).join(name)
            };
            resource["identity"] = legacy(&actual).into();
        }
        c.db.execute(
            "UPDATE assets SET identity=?1 WHERE id=?2",
            rusqlite::params![
                item["resources"][0]["identity"].as_str().unwrap(),
                item["id"].as_str().unwrap()
            ],
        )
        .unwrap();
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
        let (folder, path, _, _) = c.location(&target.id).unwrap();
        let path = Path::new(&path);
        assert_eq!(
            c.upsert(folder, path, &imaging::identity(path).unwrap(), None)
                .unwrap()
                .0,
            target.id
        );
    }
    // The next resource was claimed before a process interruption; the catalog
    // still points at its original name. Legacy recovery must recognize staging.
    let pending = &job["items"][1];
    let original = Path::new(pending["path"].as_str().unwrap());
    let staged = original
        .parent()
        .unwrap()
        .join(pending["resources"][0]["stage"].as_str().unwrap());
    fs::rename(original, staged).unwrap();
    let next = resume(&c, &data, &first.id, false).unwrap();
    let moved = execute(&mut c, next, &b);
    assert_eq!((moved.status.as_str(), moved.verified), ("completed", 7));
    let back = resume(&c, &data, &first.id, true).unwrap();
    let restored = execute(&mut c, back, &b);
    assert_eq!(
        (restored.status.as_str(), restored.verified),
        ("restored", 7)
    );
    for target in &targets {
        let (folder, path, _, key) = c.location(&target.id).unwrap();
        assert_eq!(folder, f);
        assert!(Path::new(&path).starts_with(&roots[&f]));
        assert!(key.starts_with("volume:"));
        assert_eq!(imaging::identity(Path::new(&path)).unwrap(), key);
    }
}

#[test]
fn stale_review_changes_and_snapshot_failure_leave_all_files_in_place() {
    for case in ["rating", "photo", "xmp", "snapshot"] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (mut c, roots, f, targets) = fixture(&b);
        let p = prepare(
            &c,
            &roots,
            f,
            &targets[..1],
            &b.join("destination"),
            &b.join("data"),
            "move",
        )
        .unwrap();
        match case {
            "rating" => {
                c.rate(&[targets[0].id.clone()], Some(5)).unwrap();
            }
            "photo" => {
                fs::write(roots[&f].join("generated-0.jpg"), b"External replacement").unwrap()
            }
            "xmp" => {
                fs::write(roots[&f].join("generated-0.jpg.xmp"), b"External XMP edit").unwrap()
            }
            _ => fs::write(b.join("data/backups"), b"Not a directory").unwrap(),
        }
        assert!(
            run(
                &mut c,
                p,
                &b.join("data"),
                &AtomicBool::new(false),
                |_| {},
                |_| Ok(())
            )
            .is_err(),
            "{case}"
        );
        assert!(roots[&f].join("generated-0.jpg").exists());
        assert_eq!(fs::read_dir(b.join("destination")).unwrap().count(), 0);
    }
}

#[test]
fn paired_linked_hardlinked_and_active_xmp_resources_are_rejected() {
    use std::os::unix::fs::symlink;
    for case in [
        "pair",
        "pef",
        "srw",
        "symlink",
        "hardlink",
        "active",
        "destination-link",
    ] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (c, roots, f, targets) = fixture(&b);
        let source = roots[&f].join("generated-0.jpg");
        let mut parent = b.join("destination");
        match case {
            "pair" | "pef" | "srw" => fs::write(
                roots[&f].join(format!(
                    "generated-0.{}",
                    if case == "pair" { "MOV" } else { case }
                )),
                b"Generated paired resource",
            )
            .unwrap(),
            "symlink" => {
                fs::rename(&source, b.join("retained-generated.jpg")).unwrap();
                symlink(b.join("retained-generated.jpg"), &source).unwrap();
            }
            "hardlink" => fs::hard_link(&source, b.join("generated-hardlink.jpg")).unwrap(),
            "active" => {
                c.db.execute("INSERT INTO xmp_jobs(token,asset_id,revision,source,target,staged,previous,desired,status) VALUES ('generated',?1,1,?2,'a.xmp','stage','previous','xml','prepared')",rusqlite::params![targets[0].id,source.to_str()]).unwrap();
            }
            _ => {
                symlink(&parent, b.join("destination-link")).unwrap();
                parent = b.join("destination-link");
            }
        }
        assert!(
            prepare(
                &c,
                &roots,
                f,
                &targets[..1],
                &parent,
                &b.join("data"),
                "move"
            )
            .is_err(),
            "{case}"
        );
        assert!(source.exists());
    }
}

#[test]
fn duplicate_names_are_rejected_but_identical_occurrences_stay_distinct() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, mut targets) = fixture(&b);
    let nested = roots[&f].join("nested");
    fs::create_dir(&nested).unwrap();
    let p = nested.join("generated-0.jpg");
    fs::write(&p, b"Another photo").unwrap();
    let key = imaging::identity(&p).unwrap();
    let (id, _, revision) = c.upsert(f, &p, &key, None).unwrap();
    sidecar::remember(&c, &id, &sidecar::capture(&roots[&f], &p, &key)).unwrap();
    targets.push(Target { id, revision });
    assert!(
        prepare(
            &c,
            &roots,
            f,
            &[targets[0].clone(), targets[7].clone()],
            &b.join("destination"),
            &b.join("data"),
            "move"
        )
        .is_err()
    );
    let mut copies = Vec::new();
    for name in ["copy-a.jpg", "copy-b.jpg"] {
        let p = roots[&f].join(name);
        fs::write(&p, b"Identical generated bytes").unwrap();
        let key = imaging::identity(&p).unwrap();
        let (id, _, revision) = c.upsert(f, &p, &key, None).unwrap();
        sidecar::remember(&c, &id, &sidecar::capture(&roots[&f], &p, &key)).unwrap();
        copies.push(Target { id, revision });
    }
    assert_ne!(copies[0].id, copies[1].id);
    let p = prepare(
        &c,
        &roots,
        f,
        &copies[..1],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let r = execute(&mut c, p, &b);
    assert_eq!(r.status, "completed");
    assert!(roots[&f].join("copy-b.jpg").exists());
    assert_eq!(c.location(&copies[1].id).unwrap().0, f);
}

#[test]
fn different_extensions_with_same_stem_are_rejected_before_legacy_xmp_can_be_split() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (c, roots, f, targets) = fixture(&b);
    let photo = roots[&f].join("generated-0.jpg");
    fs::rename(
        roots[&f].join("generated-0.jpg.xmp"),
        roots[&f].join("generated-0.xmp"),
    )
    .unwrap();
    let key = imaging::identity(&photo).unwrap();
    sidecar::remember(
        &c,
        &targets[0].id,
        &sidecar::capture(&roots[&f], &photo, &key),
    )
    .unwrap();
    let nested = roots[&f].join("nested");
    fs::create_dir(&nested).unwrap();
    let other = nested.join("generated-0.png");
    fs::write(&other, b"Separate generated occurrence").unwrap();
    let key = imaging::identity(&other).unwrap();
    let (id, _, revision) = c.upsert(f, &other, &key, None).unwrap();
    sidecar::remember(&c, &id, &sidecar::capture(&roots[&f], &other, &key)).unwrap();
    let result = prepare(
        &c,
        &roots,
        f,
        &[targets[0].clone(), Target { id, revision }],
        &b.join("destination"),
        &b.join("data"),
        "move",
    );
    assert!(result.err().unwrap().contains("同名主干"));
    assert!(photo.exists() && other.exists() && roots[&f].join("generated-0.xmp").exists());
    assert_eq!(fs::read_dir(b.join("destination")).unwrap().count(), 0);
}

#[test]
fn changed_derivatives_and_new_sidecars_block_withdrawal_without_losing_either_file() {
    for case in ["photo", "sidecar", "new-sidecar"] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (mut c, roots, f, targets) = fixture(&b);
        let selected = if case == "new-sidecar" {
            &targets[1..2]
        } else {
            &targets[..1]
        };
        let p = prepare(
            &c,
            &roots,
            f,
            selected,
            &b.join("destination"),
            &b.join("data"),
            "move",
        )
        .unwrap();
        let r = execute(&mut c, p, &b);
        let photo = Path::new(&r.destination).join(&r.items[0].output);
        let altered = if case == "photo" {
            photo.clone()
        } else {
            photo.with_file_name(format!("{}.xmp", r.items[0].output))
        };
        fs::write(&altered, b"External edited content").unwrap();
        assert!(resume(&c, &b.join("data"), &r.id, true).is_err());
        assert_eq!(fs::read(&altered).unwrap(), b"External edited content");
        assert!(photo.exists());
        assert!(access(&b.join("data"), false).is_ok());
    }
}

#[test]
fn operation_locks_and_catalog_restore_guard_cover_pending_and_completed_moves() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let guard = access(&b.join("data"), false).unwrap();
    assert!(access(&b.join("data"), true).is_err());
    drop(guard);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[..1],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let r = execute(&mut c, p, &b);
    assert!(access(&b.join("data"), false).is_ok());
    assert!(restore_catalog_allowed(&b.join("data")).is_err());
    let p = resume(&c, &b.join("data"), &r.id, true).unwrap();
    execute(&mut c, p, &b);
    assert!(restore_catalog_allowed(&b.join("data")).is_ok());
}

#[test]
fn withdrawn_move_rejects_snapshot_with_old_destination_bindings_but_accepts_original_bindings() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[..2],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let moved = execute(&mut c, p, &b);
    let p = resume(&c, &b.join("data"), &moved.id, true).unwrap();
    let restored = execute(&mut c, p, &b);
    let current = c.location(&targets[0].id).unwrap();
    assert_eq!(current.0, f);
    let incompatible = restored.safety_backup.unwrap();
    let bindings = catalog_restore_bindings(&c, &b.join("data")).unwrap();
    assert!(
        crate::backups::restore_with_locations(
            &mut c,
            &b.join("data/backups"),
            &incompatible,
            &bindings
        )
        .is_err()
    );
    assert_eq!(c.location(&targets[0].id).unwrap(), current);
    assert!(roots[&f].join("generated-0.jpg").exists());
    let compatible = moved.safety_backup.unwrap();
    crate::backups::restore_with_locations(&mut c, &b.join("data/backups"), &compatible, &bindings)
        .unwrap();
    assert_eq!(c.location(&targets[0].id).unwrap(), current);
}

#[test]
#[ignore = "Invoked by the generated-data crash recovery test only"]
fn crash_worker() {
    let b = PathBuf::from(std::env::var("MG_MOVE_TEST_ROOT").expect("Parent supplies a test root"));
    let phase = std::env::var("MG_MOVE_TEST_PHASE").unwrap();
    let restore = std::env::var("MG_MOVE_TEST_RESTORE").is_ok();
    let mut c = Catalog::open(&b.join("catalog.sqlite")).unwrap();
    let p = if restore {
        let id = history(&b.join("data")).unwrap()[0].id.clone();
        resume(&c, &b.join("data"), &id, true).unwrap()
    } else {
        let root = b.join("sources");
        let f = c.folder(&root).unwrap();
        let targets =
            c.db.prepare("SELECT id,revision FROM assets WHERE folder_id=?1 ORDER BY path LIMIT 2")
                .unwrap()
                .query_map([f], |r| {
                    Ok(Target {
                        id: r.get(0)?,
                        revision: r.get(1)?,
                    })
                })
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
        prepare(
            &c,
            &HashMap::from([(f, root)]),
            f,
            &targets,
            &b.join("destination"),
            &b.join("data"),
            "move",
        )
        .unwrap()
    };
    run(
        &mut c,
        p,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        |point| {
            if point == phase {
                std::process::exit(91)
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("Expected crash point was not reached");
}
fn crash(b: &Path, phase: &str, restore: bool) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "moves::tests::crash_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("MG_MOVE_TEST_ROOT", b)
        .env("MG_MOVE_TEST_PHASE", phase);
    if restore {
        command.env("MG_MOVE_TEST_RESTORE", "yes");
    }
    assert_eq!(command.status().unwrap().code(), Some(91));
}

#[test]
fn replacement_between_verify_and_claim_is_retained_without_publishing_wrong_bytes() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[..1],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let source = roots[&f].join("generated-0.jpg");
    let retained = b.join("retained-generated.jpg");
    let mut replaced = false;
    let r = run(
        &mut c,
        p,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        |phase| {
            if phase == "before_claim" && !replaced {
                fs::rename(&source, &retained).unwrap();
                fs::write(&source, b"New external occurrence").unwrap();
                replaced = true;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(r.status, "partial");
    assert_eq!(fs::read(&retained).unwrap(), b"Generated source 0");
    assert!(!Path::new(&r.destination).join("generated-0.jpg").exists());
    let stage = fs::read_dir(&roots[&f])
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".moment-grove-move-")
        })
        .unwrap();
    assert_eq!(fs::read(&stage).unwrap(), b"New external occurrence");
    assert!(resume(&c, &b.join("data"), &r.id, true).is_err());
    // Simulate explicit manual reconciliation of only these generated test resources; remove nothing.
    fs::rename(stage, b.join("retained-external.jpg")).unwrap();
    fs::rename(retained, &source).unwrap();
    let p = resume(&c, &b.join("data"), &r.id, true).unwrap();
    assert_eq!(execute(&mut c, p, &b).status, "restored");
}

#[test]
fn sqlite_write_transaction_prevents_rating_changes_during_filesystem_mutation() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[..1],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let other = rusqlite::Connection::open(b.join("catalog.sqlite")).unwrap();
    other.busy_timeout(std::time::Duration::ZERO).unwrap();
    let mut observed = false;
    let r = run(
        &mut c,
        p,
        &b.join("data"),
        &AtomicBool::new(false),
        |_| {},
        |phase| {
            if phase == "before_claim" {
                assert!(
                    other
                        .execute(
                            "UPDATE assets SET rating=5,revision=revision+1 WHERE id=?1",
                            [&targets[0].id]
                        )
                        .is_err()
                );
                observed = true;
            }
            Ok(())
        },
    )
    .unwrap();
    assert!(observed);
    assert_eq!(r.status, "completed");
    assert_eq!(c.location(&targets[0].id).unwrap().2, 1);
}

#[test]
fn process_exit_recovers_claim_publish_database_commit_and_unknown_created_directory() {
    for phase in [
        "prepared",
        "directory_created",
        "claimed",
        "published",
        "catalog_committed",
    ] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (c, _, _, _) = fixture(&b);
        drop(c);
        crash(&b, phase, false);
        let mut c = Catalog::open(&b.join("catalog.sqlite")).unwrap();
        let r = history(&b.join("data")).unwrap().remove(0);
        assert!(access(&b.join("data"), false).is_err());
        let restore = phase == "directory_created" || phase == "published";
        let p = resume(&c, &b.join("data"), &r.id, restore).unwrap();
        let r = execute(&mut c, p, &b);
        assert_eq!(
            r.status,
            if restore { "restored" } else { "completed" },
            "{phase}"
        );
        if !restore {
            let p = resume(&c, &b.join("data"), &r.id, true).unwrap();
            execute(&mut c, p, &b);
        }
        for n in 0..7 {
            assert_eq!(
                fs::read(b.join(format!("sources/generated-{n}.jpg"))).unwrap(),
                format!("Generated source {n}").as_bytes()
            );
        }
        assert!(access(&b.join("data"), false).is_ok());
    }
}

#[test]
fn withdrawal_exit_resumes_in_reverse_direction_and_preserves_new_local_rating() {
    for phase in ["claimed", "catalog_committed"] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (mut c, roots, f, targets) = fixture(&b);
        let p = prepare(
            &c,
            &roots,
            f,
            &targets[..2],
            &b.join("destination"),
            &b.join("data"),
            "move",
        )
        .unwrap();
        let r = execute(&mut c, p, &b);
        c.rate(&[targets[0].id.clone()], Some(5)).unwrap();
        drop(c);
        crash(&b, phase, true);
        let mut c = Catalog::open(&b.join("catalog.sqlite")).unwrap();
        let p = resume(&c, &b.join("data"), &r.id, false).unwrap();
        assert_eq!(review(&p).direction, "restore");
        let r = execute(&mut c, p, &b);
        assert_eq!(r.status, "restored");
        assert_eq!(
            c.db.query_row(
                "SELECT rating FROM assets WHERE id=?1",
                [&targets[0].id],
                |r| r.get::<_, Option<i32>>(0)
            )
            .unwrap(),
            Some(5)
        );
        assert_eq!(c.location(&targets[0].id).unwrap().0, f);
    }
}
#[test]
fn seven_states_move_and_withdraw_preserve_identity_bytes_history_and_pending() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let before: Vec<_> = targets.iter().map(|t| c.location(&t.id).unwrap()).collect();
    let original_xmp: Vec<_> = (0..7)
        .filter(|n| n % 2 == 0)
        .map(|n| {
            let name = format!("generated-{n}.jpg.xmp");
            let bytes = fs::read(roots[&f].join(&name)).unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains("Keep this field"));
            (name, bytes)
        })
        .collect();
    let p = prepare(
        &c,
        &roots,
        f,
        &targets,
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    assert!(roots[&f].join("generated-0.jpg").exists());
    assert_eq!(fs::read_dir(b.join("destination")).unwrap().count(), 0);
    let report = execute(&mut c, p, &b);
    assert_eq!(report.status, "completed");
    assert_eq!(report.verified, 7);
    for (name, bytes) in &original_xmp {
        assert_eq!(
            &fs::read(Path::new(&report.destination).join(name)).unwrap(),
            bytes
        );
    }
    for (n, t) in targets.iter().enumerate() {
        let (_, p, rating, key) = c.location(&t.id).unwrap();
        assert_eq!(rating, before[n].2);
        assert_eq!(key, before[n].3);
        assert_eq!(
            fs::read(&p).unwrap(),
            format!("Generated source {n}").as_bytes()
        );
        assert!(!roots[&f].join(format!("generated-{n}.jpg")).exists());
    }
    c.rate(std::slice::from_ref(&targets[6].id), Some(4))
        .unwrap();
    let p = resume(&c, &b.join("data"), &report.id, true).unwrap();
    let restored = execute(&mut c, p, &b);
    assert_eq!(restored.status, "restored");
    for (name, bytes) in &original_xmp {
        assert_eq!(&fs::read(roots[&f].join(name)).unwrap(), bytes);
    }
    for (n, t) in targets.iter().enumerate() {
        let (folder, p, rating, key) = c.location(&t.id).unwrap();
        assert_eq!(folder, f);
        assert_eq!(p, before[n].1);
        assert_eq!(key, before[n].3);
        assert_eq!(rating, if n == 6 { 2 } else { before[n].2 });
        assert_eq!(
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&t.id], |r| r
                .get::<_, Option<
                i32,
            >>(
                0
            ))
            .unwrap(),
            if n == 0 {
                None
            } else if n == 6 {
                Some(4)
            } else {
                Some(n as i32 - 1)
            }
        );
        assert!(Path::new(&p).exists());
    }
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM history", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
}
#[test]
fn quarantine_accepts_only_explicit_zero_or_one_and_keeps_original_names() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    assert!(
        prepare(
            &c,
            &roots,
            f,
            &targets[..3],
            &b.join("destination"),
            &b.join("data"),
            "quarantine"
        )
        .is_err()
    );
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[1..3],
        &b.join("destination"),
        &b.join("data"),
        "quarantine",
    )
    .unwrap();
    let r = execute(&mut c, p, &b);
    assert_eq!(r.status, "completed");
    assert_eq!(r.verified, 2);
    assert!(r.destination.contains("Quarantine"));
    assert!(roots[&f].join("generated-0.jpg").exists());
    assert!(Path::new(&r.destination).join("generated-1.jpg").exists());
}
#[test]
fn cancel_resume_and_occupied_original_never_overwrite_external_files() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets[1..3],
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let stop = AtomicBool::new(false);
    let r = run(
        &mut c,
        p,
        &b.join("data"),
        &stop,
        |r| {
            if r.verified == 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(r.status, "paused");
    assert_eq!(r.verified, 1);
    assert!(access(&b.join("data"), false).is_err());
    let p = resume(&c, &b.join("data"), &r.id, false).unwrap();
    let r = execute(&mut c, p, &b);
    assert_eq!(r.status, "completed");
    let occupied = roots[&f].join("generated-1.jpg");
    fs::write(&occupied, b"New external file").unwrap();
    let error = resume(&c, &b.join("data"), &r.id, true).err().unwrap();
    assert!(error.contains("generated-1.jpg"), "{error}");
    assert_eq!(fs::read(&occupied).unwrap(), b"New external file");
    assert!(Path::new(&r.destination).join("generated-1.jpg").exists());
    // Explicit reconciliation preserves the external file, then requires a fresh review.
    let kept = b.join("external-file-kept.txt");
    fs::rename(&occupied, &kept).unwrap();
    let p = resume(&c, &b.join("data"), &r.id, true).unwrap();
    let restored = execute(&mut c, p, &b);
    assert_eq!(restored.status, "restored");
    assert_eq!(restored.verified, 2);
    assert_eq!(fs::read(kept).unwrap(), b"New external file");
    for target in &targets[1..3] {
        let (folder, path, _, key) = c.location(&target.id).unwrap();
        let path = Path::new(&path);
        assert_eq!(folder, f);
        assert_eq!(imaging::identity(path).unwrap(), key);
        assert!(path.exists());
    }
}

/// Creates a real paused checkpoint for manual native recovery QA, without timing a GUI click.
/// Only an explicitly named, fresh generated QA batch is accepted. Never run on a media library.
#[test]
#[ignore = "Explicit generated native recovery preparation only; requires pinned fixture manifest"]
fn prepare_native_recovery_fixture() {
    use sha2::{Digest, Sha256};
    let base = PathBuf::from(std::env::var("MG_NATIVE_RECOVERY_ROOT").unwrap());
    assert_eq!(base.canonicalize().unwrap(), base);
    let name = base.file_name().unwrap().to_str().unwrap();
    let suffix = name
        .strip_prefix("moment-grove-native-3b-recovery-")
        .unwrap();
    let (date, nonce) = suffix.split_once('-').unwrap();
    assert!(date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()));
    assert!(nonce.len() == 8 && nonce.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(base.parent().unwrap().file_name().unwrap(), "runs");
    let raw = fs::read(base.join("fixtures.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&raw)),
        std::env::var("MG_NATIVE_RECOVERY_FIXTURES_SHA256").unwrap()
    );
    let fixtures: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(fixtures["batch"], name);
    assert_eq!(fixtures["files"].as_array().unwrap().len(), 8);
    assert_eq!(fixtures["sidecars"].as_array().unwrap().len(), 2);
    let source = base.join("sources");
    assert_eq!(fs::read_dir(&source).unwrap().count(), 10);
    for (kind, count) in [("files", 8), ("sidecars", 2)] {
        for n in 1..=count {
            let name = format!(
                "0{n}-generated.jpg{}",
                if kind == "sidecars" { ".xmp" } else { "" }
            );
            let entry = &fixtures[kind][n - 1];
            assert_eq!(entry["name"], name);
            let p = source.join(name);
            let m = fs::symlink_metadata(&p).unwrap();
            assert!(m.is_file() && !m.file_type().is_symlink());
            assert_eq!(
                entry["sha256"],
                format!("{:x}", Sha256::digest(fs::read(p).unwrap()))
            );
        }
    }
    let data = base.join("runtime");
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(base.join("exports-parent")).unwrap().count(),
        0
    );
    assert!(!base.join("recovery-prepared.json").exists());
    let mut c = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&source).unwrap();
    let mut targets = Vec::new();
    let mut baseline = Vec::new();
    for n in 1..=8 {
        let name = format!("0{n}-generated.jpg");
        let photo = source.join(&name);
        let identity = imaging::identity(&photo).unwrap();
        let id = c.upsert(folder, &photo, &identity, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&source, &photo, &identity)).unwrap();
        let rating = match n {
            1 => Some(0),
            2 => Some(1),
            _ => None,
        };
        let revision = if rating.is_some() {
            c.rate(std::slice::from_ref(&id), rating).unwrap();
            1
        } else {
            0
        };
        baseline.push(serde_json::json!({"id":id,"name":name,"identity":identity,"rating":rating,"revision":revision}));
        targets.push(Target { id, revision });
    }
    let roots = HashMap::from([(folder, source.clone())]);
    let plan = prepare(
        &c,
        &roots,
        folder,
        &targets[..2],
        &base.join("exports-parent"),
        &data,
        "move",
    )
    .unwrap();
    let stop = AtomicBool::new(false);
    let report = run(
        &mut c,
        plan,
        &data,
        &stop,
        |r| {
            if r.verified == 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(report.status, "paused");
    assert_eq!(report.verified, 1);
    assert!(!source.join("01-generated.jpg").exists());
    assert!(source.join("02-generated.jpg").exists());
    assert!(access(&data, false).is_err());
    drop(c);
    let receipt = serde_json::json!({"scope":"Actual engine cancellation after one verified resource group; not a native Stop-button click", "job":report,"baseline":baseline});
    fs::write(
        base.join("recovery-prepared.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn review_cancel_in_each_pass_leaves_sources_and_catalog_unchanged() {
    use crate::preparation::Review;
    for phase in ["pre-cancelled", "checking", "rechecking", "complete"] {
        let t = tempfile::tempdir().unwrap();
        let b = t.path().canonicalize().unwrap();
        let (c, roots, f, targets) = fixture(&b);
        let original: Vec<_> = (0..7)
            .map(|n| fs::read(roots[&f].join(format!("generated-{n}.jpg"))).unwrap())
            .collect();
        let cancel = AtomicBool::new(phase == "pre-cancelled");
        let mut events = Vec::new();
        let p = prepare_with_progress(
            &c,
            &roots,
            f,
            &targets,
            &b.join("destination"),
            &b.join("data"),
            "move",
            &mut Review::new(&cancel, |p| {
                if p.phase == phase && p.checked == 1 {
                    cancel.store(true, Ordering::SeqCst);
                }
                events.push(p);
            }),
        );
        if phase == "complete" {
            assert_eq!(review(&p.unwrap()).items.len(), 7);
            assert_eq!(events.last().unwrap().checked, 7);
        } else {
            assert!(
                p.is_err(),
                "Cancelled review must not return a confirmation"
            );
        }
        assert_eq!(fs::read_dir(b.join("destination")).unwrap().count(), 0);
        assert!(history(&b.join("data")).unwrap().is_empty());
        assert!(!b.join("data/backups").exists());
        for (n, t) in targets.iter().enumerate() {
            let (folder, p, revision, key) = c.location(&t.id).unwrap();
            assert_eq!((folder, revision), (f, 1));
            assert_eq!(imaging::identity(Path::new(&p)).unwrap(), key);
            assert_eq!(fs::read(p).unwrap(), original[n]);
        }
    }
}
#[test]
fn cancelled_resume_review_retains_paused_checkpoint_and_can_be_reviewed_again() {
    use crate::preparation::Review;
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let p = prepare(
        &c,
        &roots,
        f,
        &targets,
        &b.join("destination"),
        &b.join("data"),
        "move",
    )
    .unwrap();
    let stop = AtomicBool::new(false);
    let r = run(
        &mut c,
        p,
        &b.join("data"),
        &stop,
        |r| {
            if r.verified == 1 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(r.status, "paused");
    let checkpoint = serde_json::to_string(&history(&b.join("data")).unwrap()).unwrap();
    let cancelled = AtomicBool::new(true);
    assert!(
        resume_with_progress(
            &c,
            &b.join("data"),
            &r.id,
            false,
            &mut Review::new(&cancelled, |_| {})
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_string(&history(&b.join("data")).unwrap()).unwrap(),
        checkpoint
    );
    let p = resume_with_progress(
        &c,
        &b.join("data"),
        &r.id,
        false,
        &mut Review::new(&AtomicBool::new(false), |_| {}),
    )
    .unwrap();
    assert_eq!(execute(&mut c, p, &b).status, "completed");
}

#[test]
fn grove_withdraws_legacy_named_batches_without_changing_their_record_protocol() {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().canonicalize().unwrap();
    let (mut c, roots, f, targets) = fixture(&b);
    let data = b.join("data");
    let plan = prepare(
        &c,
        &roots,
        f,
        &targets[..2],
        &b.join("destination"),
        &data,
        "move",
    )
    .unwrap();
    let done = execute(&mut c, plan, &b);
    assert_eq!(done.status, "completed");
    let output = PathBuf::from(&done.destination);
    let legacy = output.with_file_name(output.file_name().unwrap().to_str().unwrap().replacen(
        "Grove-",
        "Moment-Grove-",
        1,
    ));
    fs::rename(&output, &legacy).unwrap();
    c.db.execute(
        "UPDATE assets SET path=replace(path,?1,?2) WHERE path LIKE ?3",
        rusqlite::params![
            output.to_str(),
            legacy.to_str(),
            format!("{}%", output.display())
        ],
    )
    .unwrap();
    c.db.execute(
        "UPDATE folders SET path=?2 WHERE path=?1",
        rusqlite::params![output.to_str(), legacy.to_str()],
    )
    .unwrap();
    let record = data.join("moves").join(&done.id).join("plan.json");
    let mut job: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    job["directory"] = legacy.file_name().unwrap().to_str().unwrap().into();
    fs::write(record, serde_json::to_vec(&job).unwrap()).unwrap();
    let plan = resume(&c, &data, &done.id, true).unwrap();
    let restored = execute(&mut c, plan, &b);
    assert_eq!(
        (restored.status.as_str(), restored.verified),
        ("restored", 2)
    );
    assert!(b.join("sources/generated-0.jpg").is_file());
    assert!(b.join("sources/generated-1.jpg").is_file());
}
