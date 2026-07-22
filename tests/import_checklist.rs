//! `cardroi import --checklist`.

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
fn checklist_import_creates_catalog_with_no_price_column() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("checklist_sample.csv");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap(), "--checklist"])
        .assert()
        .success()
        .stdout(predicates::str::contains("2 row(s)"));

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(repo.list_sets().unwrap().len(), 1);
    assert_eq!(repo.list_cards(None).unwrap().len(), 2);
    assert!(
        repo.list_holdings(None, None).unwrap().is_empty(),
        "checklist import must not create any holdings"
    );
}

#[test]
fn checklist_import_ignores_price_columns_when_present() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    // Reuses the acquisition fixture, which *does* have price columns -
    // --checklist should ignore them entirely and still succeed.
    let file = fixture("import_sample.csv");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap(), "--checklist"])
        .assert()
        .success();

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(repo.list_cards(None).unwrap().len(), 2);
    assert!(repo.list_holdings(None, None).unwrap().is_empty());
}

#[test]
fn checklist_import_dedups_across_rows_and_reimports() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("checklist_sample.csv");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap(), "--checklist"])
        .assert()
        .success();
    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap(), "--checklist"])
        .assert()
        .success();

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(
        repo.list_sets().unwrap().len(),
        1,
        "re-importing dedups the set"
    );
    assert_eq!(
        repo.list_cards(None).unwrap().len(),
        2,
        "re-importing dedups the cards too - checklist rows have no holding to distinguish them"
    );
}

#[test]
fn non_checklist_import_still_requires_price() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("checklist_sample.csv"); // has no price column

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--checklist"));
}
