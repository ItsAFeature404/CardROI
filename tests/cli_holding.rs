//! `holding` CRUD + status command.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

/// Seeds a set + card directly via the library and returns the card id.
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
fn add_creates_and_prints_the_holding() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id, "--serial", "12/25"])
        .assert()
        .success()
        .stdout(predicates::str::contains("12/25"));
}

#[test]
fn add_accepts_no_optional_fields() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();
}

#[test]
fn add_rejects_unknown_card_id() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["holding", "add", "--card-id", "999"])
        .assert()
        .failure();
}

#[test]
fn add_rejects_grade_without_grading_company() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id, "--grade", "10"])
        .assert()
        .failure();
}

#[test]
fn list_filters_by_card_id_and_status() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();

    let output = cardroi(&db)
        .args(["holding", "list", "--card-id", &card_id])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains('1'));

    let output = cardroi(&db)
        .args(["holding", "list", "--status", "sold"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("owned"),
        "status filter should exclude non-matching holdings"
    );
}

#[test]
fn show_prints_a_single_holding() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id, "--serial", "12/25"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("12/25"));
}

#[test]
fn show_on_missing_id_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["holding", "show", "999"])
        .assert()
        .failure();
}

#[test]
fn delete_removes_a_holding_with_no_transactions() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "delete", "1"])
        .assert()
        .success();
    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .failure();
}

#[test]
fn delete_fails_clearly_when_holding_has_transactions() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    let holding_id = {
        let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
        let (holding, _) = repo
            .record_acquisition(
                &cardroi::models::NewHolding {
                    card_id,
                    ..Default::default()
                },
                cardroi::models::NewTransaction {
                    price: "10.00".parse().unwrap(),
                    ..Default::default()
                },
            )
            .unwrap();
        holding.id
    };

    cardroi(&db)
        .args(["holding", "delete", &holding_id.to_string()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("still has transactions"));
}

#[test]
fn delete_with_transactions_removes_a_holding_that_has_them() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "10.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "delete", "1", "--with-transactions"])
        .assert()
        .success()
        .stdout(predicates::str::contains("and all its transactions"));

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .failure();
}

#[test]
fn edit_changes_only_the_flags_passed_and_leaves_the_rest_alone() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args([
            "holding",
            "add",
            "--card-id",
            &card_id,
            "--serial",
            "12/25",
            "--notes",
            "original note",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args([
            "holding",
            "edit",
            "1",
            "--grade",
            "9.5",
            "--grading-company",
            "BGS",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("9.5"))
        .stdout(predicates::str::contains("BGS"));

    // Fields not touched by the edit must survive untouched.
    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("12/25"))
        .stdout(predicates::str::contains("original note"));
}

#[test]
fn edit_with_an_empty_string_clears_that_field() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id, "--serial", "12/25"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "edit", "1", "--serial", ""])
        .assert()
        .success();

    let output = cardroi(&db)
        .args(["holding", "show", "1"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("12/25"),
        "clearing --serial with an empty string should remove the old value"
    );
}

#[test]
fn edit_rejects_grade_without_grading_company_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "edit", "1", "--grade", "10"])
        .assert()
        .failure();
}

#[test]
fn mark_lost_updates_status_and_disposed_date() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "mark-lost", "1", "--date", "2026-01-15"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("lost"));
}

#[test]
fn mark_damaged_updates_status() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["holding", "add", "--card-id", &card_id])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "mark-damaged", "1"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("damaged"));
}

#[test]
fn mark_lost_on_a_sold_holding_is_rejected_and_leaves_it_sold() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "100.00"])
        .assert()
        .success();
    cardroi(&db)
        .args(["sell", "--holding-id", "1", "--price", "150.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["holding", "mark-lost", "1"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not owned"));

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("sold"));
}
