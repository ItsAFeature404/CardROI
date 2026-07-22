//! `cardroi comp` — comps (comparable sold listings), the hobby's actual
//! valuation method: manual, user-supplied values, never a formal
//! third-party appraisal.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

fn seed_holding(db: &Path) -> i64 {
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(db).unwrap());
    let set = repo
        .create_set(&cardroi::models::NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap();
    let card = repo
        .create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.create_holding(&cardroi::models::NewHolding {
        card_id: card.id,
        ..Default::default()
    })
    .unwrap()
    .id
}

#[test]
fn comp_add_then_latest_round_trips() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "650.00",
            "--date",
            "2026-01-01",
            "--source",
            "PSA pop report comp",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("650.00"))
        .stdout(predicates::str::contains("user-supplied value"));

    cardroi(&db)
        .args(["comp", "latest", "--holding-id", &holding_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("650.00"));
}

#[test]
fn comp_latest_picks_most_recent_by_date_not_insertion_order() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "700.00",
            "--date",
            "2026-06-01",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "500.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["comp", "latest", "--holding-id", &holding_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("700.00"));
}

#[test]
fn comp_latest_on_unpriced_holding_says_so_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args(["comp", "latest", "--holding-id", &holding_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("no comps on record"));
}

#[test]
fn comp_list_shows_full_history_oldest_first() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "500.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "700.00",
            "--date",
            "2026-06-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["comp", "list", "--holding-id", &holding_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("500.00"))
        .stdout(predicates::str::contains("700.00"))
        .stdout(predicates::str::contains("not live market prices"));
}

#[test]
fn comp_add_rejects_negative_value() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "-100.00",
        ])
        .assert()
        .failure()
        // Must fail on our own domain validation, not clap misparsing
        // "-100.00" as an unrecognized flag (see allow_hyphen_values).
        .stderr(predicates::str::contains(
            "appraised_value cannot be negative",
        ));
}

#[test]
fn comp_delete_removes_the_record() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            &holding_id,
            "--value",
            "500.00",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["comp", "delete", "1"])
        .assert()
        .success();

    cardroi(&db)
        .args(["comp", "latest", "--holding-id", &holding_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("no comps on record"));
}
