//! `cardroi twr` — time-weighted return, shown alongside IRR.

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
fn holding_twr_shows_twr_and_irr_side_by_side_with_explanation() {
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
            "2026-01-01",
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
            "1000.00",
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
            "1",
            "--value",
            "1200.00",
            "--date",
            "2026-04-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["twr", "--holding-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("TWR: 20.00"))
        .stdout(predicates::str::contains("IRR:"))
        .stdout(predicates::str::contains("diverge"));
}

#[test]
fn holding_twr_with_fewer_than_two_appraisals_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "1000.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["twr", "--holding-id", "1"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("at least 2"));
}

#[test]
fn portfolio_twr_with_no_scope_flag_reports_both_scopes_clearly_labeled() {
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
            "2026-01-01",
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
            "1000.00",
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
            "1",
            "--value",
            "1100.00",
            "--date",
            "2026-06-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .arg("twr")
        .assert()
        .success()
        .stdout(predicates::str::contains("currently-owned"))
        .stdout(predicates::str::contains("closed/sold positions only"));
}

#[test]
fn twr_annualize_flag_changes_the_reported_rate() {
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
            "2026-01-01",
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
            "1000.00",
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
            "1",
            "--value",
            "1440.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();

    // Same-day appraisals with a 2-year annualization: (1.44)^(1/2) - 1 = 20%.
    cardroi(&db)
        .args(["twr", "--holding-id", "1", "--annualize", "2"])
        .assert()
        .success()
        .stdout(predicates::str::contains("TWR: 20.00"));
}
