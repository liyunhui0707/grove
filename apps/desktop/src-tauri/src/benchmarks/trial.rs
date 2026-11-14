//! Explicit generated-fixture tool for the local candidate replacement test.
use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;

#[test]
#[ignore = "Prepare only an independently hashed, private local-trial generated batch"]
fn prepare_local_trial_fixture() {
    let root = PathBuf::from(std::env::var("MG_LOCAL_TRIAL_ROOT").unwrap());
    assert_eq!(root.canonicalize().unwrap(), root);
    let name = root.file_name().unwrap().to_str().unwrap();
    let suffix = name.strip_prefix("moment-grove-native-trial-").unwrap();
    let (date, nonce) = suffix.split_once('-').unwrap();
    assert!(date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()));
    assert!(nonce.len() == 8 && nonce.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(root.parent().unwrap().file_name().unwrap(), "runs");
    assert_eq!(fs::metadata(&root).unwrap().mode() & 0o777, 0o700);
    let raw = fs::read(root.join("fixtures.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&raw)),
        std::env::var("MG_LOCAL_TRIAL_FIXTURES_SHA256").unwrap()
    );
    let fixture: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(fixture["batch"], name);
    assert_eq!(fixture["files"].as_array().unwrap().len(), 8);
    let source = root.join("sources");
    let data = root.join("runtime");
    let output = root.join("exports-parent");
    assert_eq!(fs::read_dir(&source).unwrap().count(), 8);
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    let mut c = Catalog::open(&data.join("catalog.sqlite")).unwrap();
    let folder = c.folder(&source).unwrap();
    let roots = HashMap::from([(folder, source.clone())]);
    let mut baseline = Vec::new();
    let mut selected = None;
    for (n, file) in fixture["files"].as_array().unwrap().iter().enumerate() {
        let name = format!("0{}-generated.jpg", n + 1);
        assert_eq!(file["name"], name);
        let p = source.join(&name);
        let key = imaging::identity(&p).unwrap();
        assert_eq!(
            file["sha256"],
            format!("{:x}", Sha256::digest(fs::read(&p).unwrap()))
        );
        let id = c.upsert(folder, &p, &key, None).unwrap().0;
        sidecar::remember(&c, &id, &sidecar::capture(&source, &p, &key)).unwrap();
        let rating = [Some(0), Some(1), Some(4), Some(5), None, None, None, None][n];
        if rating.is_some() {
            c.rate(std::slice::from_ref(&id), rating).unwrap();
        }
        let revision = c.location(&id).unwrap().2;
        if n == 2 {
            selected = Some(moves::Target {
                id: id.clone(),
                revision,
            });
        }
        baseline.push(serde_json::json!({"id":id,"name":name,"identity":key,"rating":rating,"revision":revision}));
    }
    let snapshot = backups::create(&c, &data.join("backups")).unwrap();
    let no = AtomicBool::new(false);
    let plan = moves::prepare(
        &c,
        &roots,
        folder,
        &[selected.unwrap()],
        &output,
        &data,
        "move",
    )
    .unwrap();
    let done = moves::run(&mut c, plan, &data, &no, |_| {}, |_| Ok(())).unwrap();
    assert_eq!((done.status.as_str(), done.verified), ("completed", 1));
    let plan = moves::resume(&c, &data, &done.id, true).unwrap();
    let undone = moves::run(&mut c, plan, &data, &no, |_| {}, |_| Ok(())).unwrap();
    assert_eq!((undone.status.as_str(), undone.verified), ("restored", 1));
    let pending: i64 =
        c.db.query_row("SELECT count(*) FROM pending_xmp", [], |r| r.get(0))
            .unwrap();
    assert_eq!(pending, 4);
    let receipt = serde_json::json!({"status":"GENERATED_FIXTURE_READY_NOT_LAUNCHED","baseline":baseline,"snapshot":snapshot,"restoredMove":undone,"pending":pending,"schema":catalog::SCHEMA_VERSION});
    fs::write(
        root.join("trial-seed.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("LOCAL_TRIAL_SEED {}", receipt);
}
