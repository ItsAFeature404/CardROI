//! `cardroi transaction` — corrections to an existing ledger entry.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

fn seed_card(db: &Path) -> i64 {
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(db).unwrap());
    let set = repo
        .create_set(&cardroi::models::NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.create_card(&cardroi::models::NewCard {
        set_id: set.id,
        card_number: "123".to_string(),
        player_name: "LeBron James".to_string(),
        ..Default::default()
    })
    .unwrap()
    .id
}

#[test]
fn show_prints_a_single_transaction() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "100.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["transaction", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("100.00"))
        .stdout(predicates::str::contains("acquisition"));
}

#[test]
fn edit_corrects_price_and_recomputes_total_leaving_type_and_holding_alone() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "100.00",
            "--fees",
            "5.00",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["transaction", "edit", "1", "--price", "120.00"])
        .assert()
        .success()
        .stdout(predicates::str::contains("125.00")); // 120.00 + fees 5.00

    cardroi(&db)
        .args(["transaction", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("acquisition"));
}

#[test]
fn edit_only_touches_flags_passed_leaving_the_rest_alone() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "100.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["transaction", "edit", "1", "--notes", "fixed a typo"])
        .assert()
        .success();

    cardroi(&db)
        .args(["transaction", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("100.00"))
        .stdout(predicates::str::contains("2026-01-01"))
        .stdout(predicates::str::contains("fixed a typo"));
}

#[test]
fn edit_rejects_negative_price_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "100.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["transaction", "edit", "1", "--price", "-1.00"])
        .assert()
        .failure();

    cardroi(&db)
        .args(["transaction", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("100.00"));
}

#[test]
fn show_on_missing_id_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["transaction", "show", "999"])
        .assert()
        .failure();
}
