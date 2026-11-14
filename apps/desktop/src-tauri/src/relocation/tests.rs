use super::*;
use crate::{backups, xmp};
struct Fixture {
    temp: tempfile::TempDir,
    c: Catalog,
    old: PathBuf,
    new: PathBuf,
    folder: i64,
    ids: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let old = root.join("old");
        let new = root.join("新位置");
        fs::create_dir_all(old.join("nested")).unwrap();
        let mut c = Catalog::open(&root.join("catalog.sqlite")).unwrap();
        let folder = c.folder(&old).unwrap();
        let mut ids = Vec::new();
        for (i, rating) in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
            .into_iter()
            .enumerate()
        {
            let path = old.join(format!("nested/{i}.jpg"));
            fs::write(&path, b"generated identical test bytes").unwrap();
            let id = c
                .upsert(folder, &path, &imaging::identity(&path).unwrap(), None)
                .unwrap()
                .0;
            sidecar::remember(
                &c,
                &id,
                &sidecar::capture(&old, &path, &imaging::identity(&path).unwrap()),
            )
            .unwrap();
            c.rate(std::slice::from_ref(&id), rating).unwrap();
            ids.push(id);
        }
        Self {
            temp,
            c,
            old,
            new,
            folder,
            ids,
        }
    }
    fn moved(&self) {
        fs::rename(&self.old, &self.new).unwrap();
    }
    fn apply(&mut self, plan: &Plan) -> Result<(i64, backups::Snapshot)> {
        apply(&mut self.c, plan, &self.temp.path().join("backups"))
    }
}
#[test]
fn another_catalog_connection_cannot_silently_change_a_reviewed_plan() {
    let mut f = Fixture::new();
    f.moved();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    let mut other = Catalog::open(&f.temp.path().join("catalog.sqlite")).unwrap();
    other
        .rate(std::slice::from_ref(&f.ids[0]), Some(4))
        .unwrap();
    drop(other);
    assert!(f.apply(&plan).is_err());
    assert!(
        f.c.location(&f.ids[0])
            .unwrap()
            .1
            .starts_with(f.old.to_str().unwrap())
    );
    let rating: Option<i32> =
        f.c.db
            .query_row("SELECT rating FROM assets WHERE id=?1", [&f.ids[0]], |r| {
                r.get(0)
            })
            .unwrap();
    assert_eq!(rating, Some(4));
}
#[test]
fn a_prepared_sidecar_job_can_resume_only_at_the_relinked_occurrence() {
    let mut f = Fixture::new();
    let id = f.ids[1].clone();
    let token = Uuid::new_v4().to_string();
    let staged = format!(".moment-grove-{token}.staged.xmp");
    let previous = format!(".moment-grove-{token}.previous.xmp");
    let desired = xmp::merge(None, Some(0)).unwrap();
    f.c.db.execute("INSERT INTO xmp_jobs(token,asset_id,revision,source,target,staged,previous,expected,desired,status) VALUES (?1,?2,1,?3,'1.jpg.xmp',?4,?5,NULL,?6,'prepared')",params![token,id,f.old.join("nested/1.jpg").to_str(),staged,previous,desired]).unwrap();
    fs::write(f.old.join("nested").join(&staged), &desired).unwrap();
    f.moved();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    f.apply(&plan).unwrap();
    let roots = std::collections::HashMap::from([(f.folder, f.new.clone())]);
    let outcome = sidecar::sync(
        &mut f.c,
        &roots,
        f.folder,
        &[sidecar::Target {
            id: id.clone(),
            revision: 1,
        }],
    )
    .unwrap();
    assert_eq!((outcome.completed, outcome.failed), (1, 0));
    assert_eq!(
        xmp::read(&fs::read_to_string(f.new.join("nested/1.jpg.xmp")).unwrap()).unwrap(),
        Some(0)
    );
    assert_eq!(
        f.c.location(&id).unwrap().1,
        f.new.join("nested/1.jpg").to_str().unwrap()
    );
}
#[test]
fn whole_directory_move_preserves_seven_states_pending_undo_and_restart() {
    let mut f = Fixture::new();
    f.moved();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    assert_eq!((plan.review.photos, plan.review.pending), (7, 7));
    let (id, snapshot) = f.apply(&plan).unwrap();
    assert_eq!(id, f.folder);
    assert_eq!(snapshot.photos, 7);
    f.c = Catalog::open(&f.temp.path().join("catalog.sqlite")).unwrap();
    let ratings: Vec<Option<i32>> = f
        .ids
        .iter()
        .map(|id| {
            f.c.db
                .query_row("SELECT rating FROM assets WHERE id=?1", [id], |r| r.get(0))
                .unwrap()
        })
        .collect();
    assert_eq!(
        ratings,
        vec![None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
    );
    let roots = std::collections::HashMap::from([(f.folder, f.new.clone())]);
    let targets = f
        .ids
        .iter()
        .map(|id| sidecar::Target {
            id: id.clone(),
            revision: 1,
        })
        .collect::<Vec<_>>();
    let result = sidecar::sync(&mut f.c, &roots, f.folder, &targets).unwrap();
    assert_eq!((result.completed, result.failed), (7, 0));
    for (i, rating) in ratings.iter().enumerate() {
        assert_eq!(
            xmp::read(&fs::read_to_string(f.new.join(format!("nested/{i}.jpg.xmp"))).unwrap())
                .unwrap(),
            *rating
        );
        assert_eq!(
            fs::read(f.new.join(format!("nested/{i}.jpg"))).unwrap(),
            b"generated identical test bytes"
        );
    }
    let token: String =
        f.c.db
            .query_row("SELECT token FROM history WHERE id=?1", [&f.ids[6]], |r| {
                r.get(0)
            })
            .unwrap();
    assert_eq!(f.c.undo(&token).unwrap().changes[0].after, None);
    let safety = backups::restore(&mut f.c, &f.temp.path().join("backups"), &snapshot.id).unwrap();
    assert_eq!(safety.restored.photos, 7);
    assert!(
        f.c.location(&f.ids[0])
            .unwrap()
            .1
            .starts_with(f.old.to_str().unwrap())
    );
}
#[test]
fn independent_copies_missing_files_and_existing_destinations_are_not_merged() {
    let mut f = Fixture::new();
    fs::create_dir_all(f.new.join("nested")).unwrap();
    for i in 0..7 {
        fs::copy(
            f.old.join(format!("nested/{i}.jpg")),
            f.new.join(format!("nested/{i}.jpg")),
        )
        .unwrap();
    }
    assert!(prepare(&f.c, f.folder, &f.new).is_err());
    fs::rename(&f.old, f.temp.path().join("archived")).unwrap();
    assert!(prepare(&f.c, f.folder, &f.new).is_err()); // Same bytes and relative names are not identity proof.
    fs::rename(&f.new, f.temp.path().join("copies")).unwrap();
    fs::rename(f.temp.path().join("archived"), &f.new).unwrap();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    fs::rename(f.new.join("nested/6.jpg"), f.new.join("nested/missing.jpg")).unwrap();
    assert!(f.apply(&plan).is_err());
    assert!(
        f.c.location(&f.ids[0])
            .unwrap()
            .1
            .starts_with(f.old.to_str().unwrap())
    );
    f.c.folder(&f.new).unwrap();
    assert!(prepare(&f.c, f.folder, &f.new).is_err());
}
#[test]
fn rating_changes_after_review_and_failed_safety_backup_leave_bindings_untouched() {
    let mut f = Fixture::new();
    f.moved();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    f.c.rate(&[f.ids[0].clone()], Some(4)).unwrap();
    assert!(f.apply(&plan).is_err());
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    fs::write(f.temp.path().join("backups"), b"not a directory").unwrap();
    assert!(f.apply(&plan).is_err());
    assert!(
        f.c.location(&f.ids[0])
            .unwrap()
            .1
            .starts_with(f.old.to_str().unwrap())
    );
}
#[cfg(unix)]
#[test]
fn substituted_directories_and_linked_parents_are_rejected() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    f.moved();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    let retained = f.temp.path().join("retained");
    fs::rename(&f.new, &retained).unwrap();
    fs::create_dir(&f.new).unwrap();
    assert!(f.apply(&plan).is_err());
    fs::rename(&f.new, f.temp.path().join("empty")).unwrap();
    fs::rename(&retained, &f.new).unwrap();
    let plan = prepare(&f.c, f.folder, &f.new).unwrap();
    fs::rename(f.new.join("nested"), f.new.join("retained-nested")).unwrap();
    symlink(f.new.join("retained-nested"), f.new.join("nested")).unwrap();
    assert!(f.apply(&plan).is_err());
}
