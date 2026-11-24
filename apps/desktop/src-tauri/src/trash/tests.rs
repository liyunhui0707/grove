use super::*;
use crate::{catalog::Catalog, imaging, sidecar};
use std::os::unix::fs::symlink;
struct Scene {
    _temp: tempfile::TempDir,
    c: Catalog,
    root: PathBuf,
    data: PathBuf,
    roots: HashMap<i64, PathBuf>,
    folder: i64,
    targets: Vec<Target>,
}
fn scene(ratings: &[Option<i32>], xmp: bool) -> Scene {
    scene_in(tempfile::tempdir().unwrap(), ratings, xmp)
}
fn scene_in(temp: tempfile::TempDir, ratings: &[Option<i32>], xmp: bool) -> Scene {
    let base = temp.path().canonicalize().unwrap();
    let root = base.join("generated");
    let data = base.join("runtime");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&data).unwrap();
    let mut c = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&root).unwrap();
    let mut targets = Vec::new();
    for (n, rating) in ratings.iter().enumerate() {
        let p = root.join(format!("generated-{n}.jpg"));
        image::RgbImage::new(24, 16).save(&p).unwrap();
        if xmp {
            fs::write(p.with_extension("xmp"), "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'/></x:xmpmeta>").unwrap();
        }
        let key = imaging::identity(&p).unwrap();
        let id = c.upsert(folder, &p, &key, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&root, &p, &key)).unwrap();
        let revision = if rating.is_some() {
            c.rate(std::slice::from_ref(&id), *rating).unwrap();
            1
        } else {
            0
        };
        targets.push(Target { id, revision });
    }
    Scene {
        _temp: temp,
        c,
        root: root.clone(),
        data,
        roots: HashMap::from([(folder, root)]),
        folder,
        targets,
    }
}
fn prepare_scene(s: &Scene) -> Result<Plan> {
    prepare(
        &s.c,
        &s.roots,
        s.folder,
        &s.targets,
        &AtomicBool::new(false),
    )
}
fn no_trash(_: &Path, _: &FileCheck) -> Result<Option<String>> {
    panic!("must never request Trash")
}
#[test]
fn only_explicit_unique_zero_ratings_and_current_revision_are_eligible() {
    for rating in [None, Some(1), Some(5)] {
        assert!(prepare_scene(&scene(&[rating], false)).is_err());
    }
    let mut s = scene(&[Some(0)], false);
    assert!(prepare_scene(&s).is_ok());
    s.targets.push(s.targets[0].clone());
    assert!(prepare_scene(&s).is_err());
    s.targets.pop();
    s.targets[0].revision = 0;
    assert!(prepare_scene(&s).is_err());
    s.targets.clear();
    assert!(prepare_scene(&s).is_err());
}
#[test]
fn changed_rating_content_identity_or_authorization_prevents_every_request() {
    for mode in ["rating", "content", "identity", "root"] {
        let mut s = scene(&[Some(0), Some(0)], false);
        let p = prepare_scene(&s).unwrap();
        let path = s.root.join("generated-1.jpg");
        match mode {
            "rating" => {
                s.c.rate(&[s.targets[1].id.clone()], Some(4)).unwrap();
            }
            "content" => {
                fs::write(&path, b"changed").unwrap();
            }
            "identity" => {
                fs::rename(&path, s.root.join("retained.jpg")).unwrap();
                image::RgbImage::new(24, 16).save(&path).unwrap();
            }
            _ => {
                s.roots.clear();
            }
        }
        assert!(
            execute(
                &s.c,
                &s.roots,
                p,
                &s.data,
                &AtomicBool::new(false),
                |_| {},
                no_trash
            )
            .is_err()
        );
        assert!(s.root.join("generated-0.jpg").exists());
    }
}
#[test]
fn refuses_symlinks_hardlinks_companion_video_and_changed_xmp() {
    for mode in ["symlink", "hardlink", "video", "xmp"] {
        let s = scene(&[Some(0)], false);
        let p = prepare_scene(&s).unwrap();
        let path = s.root.join("generated-0.jpg");
        match mode {
            "symlink" => {
                fs::rename(&path, s.root.join("retained.jpg")).unwrap();
                symlink(s.root.join("retained.jpg"), &path).unwrap();
            }
            "hardlink" => fs::hard_link(&path, s.root.join("hardlink.jpg")).unwrap(),
            "video" => fs::write(s.root.join("generated-0.MOV"), b"companion").unwrap(),
            _ => fs::write(path.with_extension("xmp"), b"new xmp").unwrap(),
        }
        assert!(
            execute(
                &s.c,
                &s.roots,
                p,
                &s.data,
                &AtomicBool::new(false),
                |_| {},
                no_trash
            )
            .is_err()
        );
    }
}
#[test]
fn canceled_review_never_requests_trash() {
    let s = scene(&[Some(0)], false);
    let p = prepare_scene(&s).unwrap();
    assert!(
        execute(
            &s.c,
            &s.roots,
            p,
            &s.data,
            &AtomicBool::new(true),
            |_| {},
            no_trash
        )
        .is_err()
    );
}
#[test]
fn completed_groups_count_photos_not_sidecars_and_preserve_rating_rows() {
    let s = scene(&[Some(0), Some(0)], true);
    let p = prepare_scene(&s).unwrap();
    let out = s.data.join("simulated-trash");
    fs::create_dir(&out).unwrap();
    let r = execute(
        &s.c,
        &s.roots,
        p,
        &s.data,
        &AtomicBool::new(false),
        |_| {},
        |path, _| {
            let dest = out.join(path.file_name().unwrap());
            fs::rename(path, &dest).unwrap();
            Ok(Some(dest.to_string_lossy().into_owned()))
        },
    )
    .unwrap();
    assert_eq!((r.status.as_str(), r.completed), ("completed", 2));
    assert_eq!(r.items[0].resources.len(), 2);
    assert_eq!(history(&s.data).unwrap()[0].completed, 2);
    for t in &s.targets {
        assert_eq!(s.c.location(&t.id).unwrap().2, 1);
    }
    assert!(
        r.items
            .iter()
            .all(|r| r.resources.iter().all(|f| f.status == "trashed"))
    );
}
#[test]
fn partial_sidecar_failure_is_visible_and_does_not_continue_other_photos() {
    let s = scene(&[Some(0), Some(0)], true);
    let p = prepare_scene(&s).unwrap();
    let mut requests = 0;
    let r = execute(
        &s.c,
        &s.roots,
        p,
        &s.data,
        &AtomicBool::new(false),
        |_| {},
        |_, _| {
            requests += 1;
            if requests == 2 {
                Err("permission denied".into())
            } else {
                Ok(Some("simulated-result".into()))
            }
        },
    )
    .unwrap();
    assert_eq!(
        (r.status.as_str(), r.completed, requests),
        ("stopped", 0, 2)
    );
    assert_eq!(r.items[0].resources[0].status, "trashed");
    assert_eq!(r.items[0].resources[1].status, "requesting");
    assert_eq!(r.items[1].status, "pending");
}
#[test]
fn stop_finishes_photo_xmp_group_and_retains_pending_items() {
    let s = scene(&[Some(0), Some(0)], true);
    let p = prepare_scene(&s).unwrap();
    let cancel = AtomicBool::new(false);
    let mut requests = 0;
    let r = execute(
        &s.c,
        &s.roots,
        p,
        &s.data,
        &cancel,
        |_| {},
        |_, _| {
            requests += 1;
            cancel.store(true, Ordering::SeqCst);
            Ok(None)
        },
    )
    .unwrap();
    assert_eq!(
        (r.status.as_str(), r.completed, requests),
        ("stopped", 1, 2)
    );
    assert_eq!(r.items[1].status, "pending");
}
#[test]
fn interrupted_request_history_does_not_claim_success_or_retry() {
    let s = scene(&[Some(0)], false);
    let p = prepare_scene(&s).unwrap();
    let mut r = review(&p).report;
    r.status = "running".into();
    r.items[0].status = "attention".into();
    r.items[0].resources[0].status = "requesting".into();
    let d = journal(&s.data).unwrap().create(&r.id).unwrap();
    record(&d, &mut 0, &r).unwrap();
    let h = history(&s.data).unwrap();
    assert_eq!(h[0].status, "interrupted");
    assert_eq!(h[0].completed, 0);
    assert_eq!(h[0].items[0].resources[0].status, "requesting");
}
#[test]
fn stable_reference_follows_checked_occurrence_instead_of_path_replacement() {
    let s = scene(&[Some(0)], false);
    let p = prepare_scene(&s).unwrap();
    let path = s.root.join("generated-0.jpg");
    autoreleasepool(|_| {
        let reference = TrashFile::open(&path, &p.items[0].files[0]).unwrap();
        let retained = s.root.join("retained.jpg");
        fs::rename(&path, &retained).unwrap();
        image::RgbImage::new(24, 16).save(&path).unwrap();
        assert_eq!(reference.path().unwrap(), retained);
        assert_ne!(
            imaging::identity(&path).unwrap(),
            p.items[0].files[0].identity
        );
    });
}
#[test]
#[ignore = "Explicit generated-media native Trash QA; restores only returned owned files"]
fn native_generated_photo_and_xmp_go_to_trash_and_restore_without_overwrite() {
    assert_eq!(std::env::var("MG_RUN_TRASH_QA").as_deref(), Ok("1"));
    let s = scene(&[Some(0), Some(0)], true);
    let p = prepare_scene(&s).unwrap();
    let checks: Vec<_> = p
        .items
        .iter()
        .flat_map(|i| i.files.iter().map(|f| (f.name.clone(), f.hash.clone())))
        .collect();
    let r = run(&s.c, &s.roots, p, &s.data, &AtomicBool::new(false), |_| {}).unwrap();
    let proof = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.local/trash-native-qa");
    fs::create_dir_all(&proof).unwrap();
    fs::write(
        proof.join(format!("{}.json", r.id)),
        serde_json::to_vec_pretty(&r).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (r.status.as_str(), r.completed),
        ("completed", 2),
        "{:?}",
        r.message
    );
    for row in r.items {
        for resource in row.resources {
            let dest = PathBuf::from(resource.destination.unwrap());
            assert!(!s.root.join(&resource.name).exists());
            let f = std::fs::File::open(&dest).unwrap();
            assert_eq!(
                files::digest(&f, &AtomicBool::new(false)).unwrap().0,
                checks.iter().find(|c| c.0 == resource.name).unwrap().1
            );
            let to = Dir::open(&s.root).unwrap();
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                &dest,
                &to.file,
                &resource.name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .unwrap();
        }
    }
    for t in &s.targets {
        let (_, path, _, _) = s.c.location(&t.id).unwrap();
        assert!(Path::new(&path).exists());
    }
}

#[test]
#[ignore = "Create only a named, empty generated native-GUI fixture directory"]
fn prepare_grove_native_fixture() {
    use crate::{safety_tests::state, workspace};
    let root = PathBuf::from(std::env::var("GROVE_NATIVE_QA_ROOT").unwrap());
    assert!(root.starts_with("/private/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("grove-native-qa-")
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let source = root.join("generated-photos");
    fs::create_dir(&source).unwrap();
    for n in 0..4 {
        image::RgbImage::from_fn(160, 120, |x, y| {
            image::Rgb([((x + n * 40) % 255) as u8, ((y + 80) % 255) as u8, 140])
        })
        .save(source.join(format!("generated-{n}.png")))
        .unwrap();
    }
    fs::write(source.join("generated-0.xmp"),"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'/></x:xmpmeta>").unwrap();
    let s = state(&root.join("runtime"));
    let folder = crate::grant(&s, source.clone()).unwrap();
    let mut hashes = Vec::new();
    for (n, rating) in [Some(0), Some(0), Some(4), None].into_iter().enumerate() {
        let p = source.join(format!("generated-{n}.png"));
        let key = imaging::identity(&p).unwrap();
        let mut c = s.catalog.lock().unwrap();
        let id = c.upsert(folder.id, &p, &key, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&source, &p, &key)).unwrap();
        if rating.is_some() {
            c.rate(&[id], rating).unwrap();
        }
        hashes.push(serde_json::json!({"name":p.file_name().unwrap().to_str().unwrap(),"identity":key,"sha256":files::hash_bytes(&fs::read(&p).unwrap())}));
    }
    workspace::save(
        &s,
        Some(folder.id),
        workspace::View {
            mode: "grid".into(),
            active: "".into(),
            filter: "reject".into(),
            sort: "name".into(),
            search: "".into(),
            size: 270,
            auto_advance: true,
            theme: "light".into(),
            offset: 0.0,
        },
    )
    .unwrap();
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"source":source,"data":root.join("runtime"),"files":hashes}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Run only on the operator-created disposable GroveRecoveryQA24 volume with Finder"]
fn finder_restores_generated_pair_and_catalog_occurrence() {
    let volume = Path::new("/Volumes/GroveRecoveryQA24");
    assert_eq!(std::env::var("GROVE_FINDER_QA").as_deref(), Ok("1"));
    let s = scene_in(tempfile::tempdir_in(volume).unwrap(), &[Some(0)], true);
    let plan = prepare_scene(&s).unwrap();
    let checks = plan.items[0].files.clone();
    let result = run(
        &s.c,
        &s.roots,
        plan,
        &s.data,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(result.completed, 1);
    for (res, check) in result.items[0].resources.iter().zip(checks) {
        let path = s.root.join(&res.name);
        let dest = Path::new(res.destination.as_deref().unwrap());
        assert!(dest.starts_with(volume.join(".Trashes")));
        assert!(!path.exists());
        let script = r#"on run argv
            set sourceFile to POSIX file (item 1 of argv) as alias
            set targetFolder to POSIX file (item 2 of argv) as alias
            tell application "Finder" to move sourceFile to targetFolder without replacing
        end run"#;
        assert!(
            std::process::Command::new("osascript")
                .args(["-e", script])
                .arg(dest)
                .arg(&s.root)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(files::hash_bytes(&fs::read(&path).unwrap()), check.hash);
        if res.name.ends_with(".jpg") {
            assert!(!path.with_extension("xmp").exists());
            let id =
                s.c.upsert(s.folder, &path, &imaging::identity(&path).unwrap(), None)
                    .unwrap()
                    .0;
            assert_eq!(id, s.targets[0].id);
            let rating: i32 =
                s.c.db
                    .query_row("SELECT rating FROM assets WHERE id=?1", [&id], |r| r.get(0))
                    .unwrap();
            assert_eq!(rating, 0);
        }
    }
}
