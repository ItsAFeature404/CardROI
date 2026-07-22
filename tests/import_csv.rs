//! `cardroi import --format csv`.

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
fn imports_sample_csv_and_roundtrips_through_roi() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("import_sample.csv");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("2 row(s)"));

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(repo.list_sets().unwrap().len(), 1);
    assert_eq!(repo.list_cards(None).unwrap().len(), 2);
    assert_eq!(repo.list_holdings(None, None).unwrap().len(), 2);

    // Row 1: 500 + 25 fees + 10 shipping = 535.00
    // Row 2: 80 + 5 fees = 85.00
    // Total portfolio cost basis: 620.00
    cardroi(&db)
        .arg("roi")
        .assert()
        .success()
        .stdout(predicates::str::contains("620.00"));
}

#[test]
fn format_is_inferred_from_csv_extension() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("import_sample.csv");

    // No --format flag: inferred from the .csv extension.
    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn malformed_row_reports_row_number_and_commits_nothing() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let bad_csv = dir.path().join("bad.csv");
    std::fs::write(
        &bad_csv,
        "set_name,sport,card_number,player_name,price\n\
         2023 Topps Chrome,Basketball,123,LeBron James,not-a-price\n",
    )
    .unwrap();

    cardroi(&db)
        .args(["import", "--file", bad_csv.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("row 1"));

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert!(repo.list_sets().unwrap().is_empty());
}

#[test]
fn reimporting_the_same_file_dedups_catalog_but_adds_new_holdings() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let file = fixture("import_sample.csv");

    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success();
    cardroi(&db)
        .args(["import", "--file", file.to_str().unwrap()])
        .assert()
        .success();

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    assert_eq!(repo.list_sets().unwrap().len(), 1, "set catalog dedups");
    assert_eq!(
        repo.list_cards(None).unwrap().len(),
        2,
        "card catalog dedups"
    );
    assert_eq!(
        repo.list_holdings(None, None).unwrap().len(),
        4,
        "importing twice means buying twice - 2 holdings per import"
    );
}
