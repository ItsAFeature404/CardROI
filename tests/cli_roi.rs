//! `roi` command.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

/// Seeds a set + card, buys one holding for 100.00, sells it for 150.00
/// (50.00 realized profit), and returns (card_id, holding_id).
fn seed_round_trip(db: &Path) -> (i64, i64) {
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
    let (holding, _) = repo
        .record_acquisition(
            &cardroi::models::NewHolding {
                card_id: card.id,
                ..Default::default()
            },
            cardroi::models::NewTransaction {
                price: "100.00".parse().unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_sale(cardroi::models::NewTransaction {
        holding_id: holding.id,
        price: "150.00".parse().unwrap(),
        ..Default::default()
    })
    .unwrap();
    (card.id, holding.id)
}

#[test]
fn roi_with_no_scope_shows_portfolio_rollup() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_round_trip(&db);

    cardroi(&db)
        .arg("roi")
        .assert()
        .success()
        .stdout(predicates::str::contains("50.00"));
}

#[test]
fn roi_card_id_shows_card_rollup() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let (card_id, _) = seed_round_trip(&db);

    cardroi(&db)
        .args(["roi", "--card-id", &card_id.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains("50.00"));
}

#[test]
fn roi_holding_id_shows_holding_detail_with_percentage() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let (_, holding_id) = seed_round_trip(&db);

    // 50.00 realized on 100.00 cost basis = 50% ROI.
    cardroi(&db)
        .args(["roi", "--holding-id", &holding_id.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains("50.00"))
        .stdout(predicates::str::contains('%'));
}

#[test]
fn roi_json_format_is_valid_and_contains_expected_fields() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_round_trip(&db);

    let output = cardroi(&db)
        .args(["roi", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.get("realized_pnl").is_some());
    assert!(json.get("holding_count").is_some());
}

#[test]
fn roi_rejects_multiple_scope_flags() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let (card_id, holding_id) = seed_round_trip(&db);

    cardroi(&db)
        .args([
            "roi",
            "--card-id",
            &card_id.to_string(),
            "--holding-id",
            &holding_id.to_string(),
        ])
        .assert()
        .failure();
}

#[test]
fn roi_on_unsold_holding_reports_no_realized_pnl() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
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
            card_number: "1".to_string(),
            player_name: "Player One".to_string(),
            ..Default::default()
        })
        .unwrap();
    let (holding, _) = repo
        .record_acquisition(
            &cardroi::models::NewHolding {
                card_id: card.id,
                ..Default::default()
            },
            cardroi::models::NewTransaction {
                price: "10.00".parse().unwrap(),
                ..Default::default()
            },
        )
        .unwrap();

    cardroi(&db)
        .args(["roi", "--holding-id", &holding.id.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains("not yet realized"));
}

#[test]
fn roi_on_appraised_open_holding_shows_labeled_unrealized_pnl() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
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
            card_number: "1".to_string(),
            player_name: "Player One".to_string(),
            ..Default::default()
        })
        .unwrap();
    let (holding, _) = repo
        .record_acquisition(
            &cardroi::models::NewHolding {
                card_id: card.id,
                ..Default::default()
            },
            cardroi::models::NewTransaction {
                price: "100.00".parse().unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
    repo.create_appraisal(&cardroi::models::NewAppraisal {
        holding_id: holding.id,
        appraised_value: "160.00".parse().unwrap(),
        appraised_date: chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        ..Default::default()
    })
    .unwrap();

    cardroi(&db)
        .args(["roi", "--holding-id", &holding.id.to_string()])
        .assert()
        .success()
        .stdout(predicates::str::contains("60.00"))
        .stdout(predicates::str::contains("2026-06-01"))
        .stdout(predicates::str::contains("user-supplied comp"));
}

#[test]
fn roi_on_unappraised_open_holding_shows_no_unrealized_claim() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
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
            card_number: "1".to_string(),
            player_name: "Player One".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.record_acquisition(
        &cardroi::models::NewHolding {
            card_id: card.id,
            ..Default::default()
        },
        cardroi::models::NewTransaction {
            price: "100.00".parse().unwrap(),
            ..Default::default()
        },
    )
    .unwrap();

    cardroi(&db)
        .args(["roi", "--holding-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Unrealized P&L: n/a (no comp on record)",
        ));
}
