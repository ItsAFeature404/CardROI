//! `card` CRUD command.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

/// Seeds a set directly via the library and returns its id, so card tests
/// don't depend on parsing `set add` stdout.
fn seed_set(db: &Path) -> i64 {
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(db).unwrap());
    repo.create_set(&cardroi::models::NewSet {
        name: "2023 Topps Chrome".to_string(),
        sport: "Basketball".to_string(),
        ..Default::default()
    })
    .unwrap()
    .id
}

#[test]
fn add_creates_and_prints_the_card() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db).to_string();

    cardroi(&db)
        .args([
            "card",
            "add",
            "--set-id",
            &set_id,
            "--number",
            "123",
            "--player",
            "LeBron James",
            "--variant",
            "Refractor",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("LeBron James"));
}

#[test]
fn add_rejects_empty_player_name() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db).to_string();

    cardroi(&db)
        .args([
            "card", "add", "--set-id", &set_id, "--number", "123", "--player", "",
        ])
        .assert()
        .failure();
}

#[test]
fn add_rejects_unknown_set_id() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    // Opens/migrates the db but seeds no set.
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args([
            "card", "add", "--set-id", "999", "--number", "1", "--player", "Nobody",
        ])
        .assert()
        .failure();
}

#[test]
fn list_filters_by_set_id() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db).to_string();

    cardroi(&db)
        .args([
            "card",
            "add",
            "--set-id",
            &set_id,
            "--number",
            "1",
            "--player",
            "Player One",
        ])
        .assert()
        .success();

    let output = cardroi(&db)
        .args(["card", "list", "--set-id", &set_id])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("Player One"));

    // A different, nonexistent set id filters everything out.
    let output = cardroi(&db)
        .args(["card", "list", "--set-id", "999"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Player One"));
}

#[test]
fn show_prints_a_single_card() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db).to_string();
    cardroi(&db)
        .args([
            "card",
            "add",
            "--set-id",
            &set_id,
            "--number",
            "1",
            "--player",
            "Player One",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["card", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Player One"));
}

#[test]
fn show_on_missing_id_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["card", "show", "999"])
        .assert()
        .failure();
}

#[test]
fn delete_removes_a_card_with_no_holdings() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db).to_string();
    cardroi(&db)
        .args([
            "card",
            "add",
            "--set-id",
            &set_id,
            "--number",
            "1",
            "--player",
            "Player One",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["card", "delete", "1"])
        .assert()
        .success();
    cardroi(&db).args(["card", "show", "1"]).assert().failure();
}

#[test]
fn delete_fails_clearly_when_card_has_holdings() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let set_id = seed_set(&db);

    let card_id = {
        let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
        let card = repo
            .create_card(&cardroi::models::NewCard {
                set_id,
                card_number: "1".to_string(),
                player_name: "Player One".to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_holding(&cardroi::models::NewHolding {
            card_id: card.id,
            ..Default::default()
        })
        .unwrap();
        card.id
    };

    cardroi(&db)
        .args(["card", "delete", &card_id.to_string()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("still has holdings"));
}
