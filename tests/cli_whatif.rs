//! `cardroi whatif` — hypothetical disposition, never persisted.

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
fn whatif_with_price_shows_hypothetical_pnl_clearly_labeled() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "500.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args([
            "whatif",
            "--holding-id",
            "1",
            "--price",
            "800.00",
            "--date",
            "2026-06-01",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("HYPOTHETICAL"))
        .stdout(predicates::str::contains("NOT sold"))
        .stdout(predicates::str::contains("300.00"))
        .stdout(predicates::str::contains(
            "user-supplied hypothetical price",
        ));
}

#[test]
fn whatif_at_comp_uses_latest_comp_as_price() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();
    cardroi(&db)
        .args([
            "comp",
            "add",
            "--holding-id",
            "1",
            "--value",
            "900.00",
            "--date",
            "2026-03-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args(["whatif", "--holding-id", "1", "--at-comp"])
        .assert()
        .success()
        .stdout(predicates::str::contains("900.00"))
        .stdout(predicates::str::contains("400.00"))
        .stdout(predicates::str::contains("2026-03-01"))
        .stdout(predicates::str::contains("not a live market value"));
}

#[test]
fn whatif_at_comp_with_no_comp_fails_clearly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();

    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["whatif", "--holding-id", "1", "--at-comp"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no comp on record"));
}

#[test]
fn whatif_requires_exactly_one_of_price_or_at_comp() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["whatif", "--holding-id", "1"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "exactly one of --price or --at-comp",
        ));

    cardroi(&db)
        .args([
            "whatif",
            "--holding-id",
            "1",
            "--price",
            "800.00",
            "--at-comp",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "exactly one of --price or --at-comp",
        ));
}

#[test]
fn whatif_on_a_sold_holding_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();
    cardroi(&db)
        .args(["sell", "--holding-id", "1", "--price", "800.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["whatif", "--holding-id", "1", "--price", "999.00"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not currently owned"));
}

#[test]
fn whatif_never_writes_anything_to_the_database() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();

    // Run it twice, then confirm the real roi output is still that of an
    // untouched, unsold holding - no new transaction, no status change.
    cardroi(&db)
        .args(["whatif", "--holding-id", "1", "--price", "800.00"])
        .assert()
        .success();
    cardroi(&db)
        .args(["whatif", "--holding-id", "1", "--price", "1200.00"])
        .assert()
        .success();

    cardroi(&db)
        .args(["roi", "--holding-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Status: owned"))
        .stdout(predicates::str::contains("not yet realized"));

    cardroi(&db)
        .args(["holding", "show", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("owned"));
}

#[test]
fn whatif_json_output_is_structurally_distinct_from_roi_json() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args(["buy", "--card-id", &card_id, "--price", "500.00"])
        .assert()
        .success();

    let output = cardroi(&db)
        .args([
            "whatif",
            "--holding-id",
            "1",
            "--price",
            "800.00",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(
        json.get("hypothetical").and_then(|v| v.as_bool()),
        Some(true)
    );
    // Field names must differ from roi's HoldingPnl JSON shape - no
    // "realized_pnl"/"roi_pct" keys that a script might confuse for real.
    assert!(json.get("realized_pnl").is_none());
    assert!(json.get("hypothetical_realized_pnl").is_some());
}

#[test]
fn same_day_whatif_reports_pnl_with_no_defined_irr() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let card_id = seed_card(&db).to_string();
    cardroi(&db)
        .args([
            "buy",
            "--card-id",
            &card_id,
            "--price",
            "500.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();

    cardroi(&db)
        .args([
            "whatif",
            "--holding-id",
            "1",
            "--price",
            "800.00",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("300.00"))
        .stdout(predicates::str::contains("Hypothetical IRR: n/a"));
}
