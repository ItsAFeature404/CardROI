//! `cardroi irr` (closed positions).

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
fn holding_irr_matches_exact_ten_percent() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    // Exactly 365 days apart (2023 is not a leap year), 1000 -> 1100:
    // hand-computable exact 10.00% XIRR.
    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "1000.00",
            "--date",
            "2023-01-01",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "sell",
            "--holding-id",
            "1",
            "--price",
            "1100.00",
            "--date",
            "2024-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["irr", "--holding-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("10.00"));
}

#[test]
fn irr_on_still_owned_holding_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "1000.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["irr", "--holding-id", "1"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not yet sold"));
}

#[test]
fn portfolio_irr_with_no_scope_flag_uses_closed_positions() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "1000.00",
            "--date",
            "2023-01-01",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "sell",
            "--holding-id",
            "1",
            "--price",
            "1100.00",
            "--date",
            "2024-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .arg("irr")
        .assert()
        .success()
        .stdout(predicates::str::contains("10.00"));
}

#[test]
fn holding_irr_on_open_position_uses_appraisal_and_labels_it() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "1000.00",
            "--date",
            "2023-01-01",
        ])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            "1",
            "--value",
            "1100.00",
            "--date",
            "2024-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["irr", "--holding-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("10.00"))
        .stdout(predicates::str::contains("still owned"))
        .stdout(predicates::str::contains("2024-01-01"))
        .stdout(predicates::str::contains("not a live market value"));
}

#[test]
fn portfolio_irr_with_no_sold_holdings_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .arg("irr")
        .assert()
        .failure()
        .stderr(predicates::str::contains("no closed"));
}
