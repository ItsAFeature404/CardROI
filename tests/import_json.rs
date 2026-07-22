//! `cardroi import --format json`.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_sample_json_and_roundtrips_through_roi() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("import_sample.json");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("2 row(s)"));

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(repo.list_sets().unwrap().len(), 1);
    assert_eq!(repo.list_cards(None).unwrap().len(), 2);

    // Row 1: 20 + 1 fee = 21.00; Row 2: 15.00. Total: 36.00.
    cardroi(&db)
        .arg("roi")
        .assert()
        .success()
        .stdout(predicates::str::contains("36.00"));
}

#[test]
fn format_is_inferred_from_json_extension() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("import_sample.json");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn malformed_json_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let bad_json = dir.path().join("bad.json");
    std::fs::write(&bad_json, "{ not valid json").unwrap();

    cardroi(&db)
        .args(["import", "--file", bad_json.to_str().unwrap()])
        .assert()
        .failure();

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert!(repo.list_sets().unwrap().is_empty());
}
