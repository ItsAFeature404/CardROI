//! `buy` command, including --quantity fan-out.

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

fn count_holdings(db: &Path, card_id: i64) -> usize {
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(db).unwrap());
    repo.list_holdings(Some(card_id), None).unwrap().len()
}

#[test]
fn single_buy_creates_one_holding_and_transaction_with_correct_total() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "100.00",
            "--fees",
            "5.00",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("105.00"));

    assert_eq!(count_holdings(&db, card_id), 1);
}

#[test]
fn quantity_fans_out_into_multiple_holdings() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "10.00",
            "--quantity",
            "3",
        ])
        .assert()
        .success();

    assert_eq!(count_holdings(&db, card_id), 3);

    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(&db).unwrap());
    let total_transactions: usize = repo
        .list_holdings(Some(card_id), None)
        .unwrap()
        .iter()
        .map(|h| repo.list_transactions_for_holding(h.id).unwrap().len())
        .sum();
    assert_eq!(total_transactions, 3);
}

#[test]
fn zero_quantity_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "10.00",
            "--quantity",
            "0",
        ])
        .assert()
        .failure();

    assert_eq!(count_holdings(&db, card_id), 0);
}

#[test]
fn negative_price_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "-10.00",
        ])
        .assert()
        .failure()
        // Must fail on our own domain validation, not clap misparsing
        // "-10.00" as an unrecognized flag (see allow_hyphen_values).
        .stderr(predicates::str::contains("price cannot be negative"));
}

#[test]
fn european_style_comma_decimal_price_is_rejected_as_ambiguous() {
    // "10,00" is how many locales write ten dollars. Silently treating the
    // comma as a thousands separator would turn it into 1000.00 - a 100x
    // error with zero warning. Must fail loudly, not guess.
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args(["buy", "--card-id", &card_id.to_string(), "--price", "10,00"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ambiguous comma placement"));
}

#[test]
fn thousands_grouped_price_still_parses_correctly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "1,234.50",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("1234.50"));
}

#[test]
fn serial_number_rejected_when_quantity_greater_than_one() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db);

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id.to_string(),
            "--price",
            "10.00",
            "--quantity",
            "2",
            "--serial",
            "12/25",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--quantity"));

    assert_eq!(count_holdings(&db, card_id), 0);
}

#[test]
fn unknown_card_id_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    cardroi::db::open(&db).unwrap();

    cardroi(&db)
        .args(["buy", "--card-id", "999", "--price", "10.00"])
        .assert()
        .failure();
}
