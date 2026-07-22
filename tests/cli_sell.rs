//! `sell` command.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

/// Seeds a set + card + owned holding directly via the library and returns
/// the holding id.
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
    holding.id
}

#[test]
fn sell_flips_status_and_prints_net_proceeds() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args([
            "sell",
            "--holding-id",
            &holding_id,
            "--price",
            "150.00",
            "--fees",
            "10.00",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("140.00"));

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    let holding = repo.get_holding(holding_id.parse().unwrap()).unwrap();
    assert_eq!(holding.status, cardroi::models::HoldingStatus::Sold);
    assert!(holding.disposed_date.is_some());
}

#[test]
fn selling_an_already_sold_holding_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args(["sell", "--holding-id", &holding_id, "--price", "150.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["sell", "--holding-id", &holding_id, "--price", "200.00"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not owned"));
}

#[test]
fn selling_an_unknown_holding_fails() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["sell", "--holding-id", "999", "--price", "10.00"])
        .assert()
        .failure();
}

#[test]
fn negative_price_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let holding_id = seed_holding(&db).to_string();

    cardroi(&db)
        .args(["sell", "--holding-id", &holding_id, "--price", "-10.00"])
        .assert()
        .failure()
        // Must fail on our own domain validation, not clap misparsing
        // "-10.00" as an unrecognized flag (see allow_hyphen_values).
        .stderr(predicates::str::contains("price cannot be negative"));
}
