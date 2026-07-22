//! `set` CRUD command.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

#[test]
fn add_creates_and_prints_the_set() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");

    cardroi(&db)
        .args([
            "set",
            "add",
            "--name",
            "2023 Topps Chrome",
            "--sport",
            "Basketball",
            "--year",
            "2023",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("2023 Topps Chrome"));
}

#[test]
fn add_rejects_empty_name() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");

    cardroi(&db)
        .args(["set", "add", "--name", "", "--sport", "Basketball"])
        .assert()
        .failure();
}

#[test]
fn list_shows_sets_sorted_by_year_descending() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");

    cardroi(&db)
        .args([
            "set",
            "add",
            "--name",
            "Older Set",
            "--sport",
            "Basketball",
            "--year",
            "2010",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "set",
            "add",
            "--name",
            "Newer Set",
            "--sport",
            "Basketball",
            "--year",
            "2023",
        ])
        .assert()
        .success();

    let output = cardroi(&db).args(["set", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let newer_pos = stdout.find("Newer Set").expect("newer set listed");
    let older_pos = stdout.find("Older Set").expect("older set listed");
    assert!(
        newer_pos < older_pos,
        "newer set should be listed before older set (year desc)"
    );
}

#[test]
fn show_prints_a_single_set() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi(&db)
        .args([
            "set",
            "add",
            "--name",
            "2023 Topps Chrome",
            "--sport",
            "Basketball",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["set", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("2023 Topps Chrome"));
}

#[test]
fn show_on_missing_id_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi(&db).args(["set", "show", "999"]).assert().failure();
}

#[test]
fn delete_removes_a_set_with_no_cards() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi(&db)
        .args(["set", "add", "--name", "Deletable", "--sport", "Basketball"])
        .assert()
        .success();

    cardroi(&db).args(["set", "delete", "1"]).assert().success();
    cardroi(&db).args(["set", "show", "1"]).assert().failure();
}

#[test]
fn delete_fails_clearly_when_set_has_cards() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");

    // Seed directly via the library rather than the `card` CLI command,
    // to keep this test focused on `set delete`'s own guard.
    {
        let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
        let set = repo
            .create_set(&cardroi::models::NewSet {
                name: "Has Cards".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "1".to_string(),
            player_name: "Test Player".to_string(),
            ..Default::default()
        })
        .unwrap();
    }

    cardroi(&db)
        .args(["set", "delete", "1"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("still has cards"));
}
