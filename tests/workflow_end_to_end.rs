//! Phase 1 checkpoint: the full pipeline end-to-end through the real
//! binary — import a fixture, sell one holding, check `roi`, check
//! `report`. Nothing here calls the library directly; every step goes
//! through the CLI exactly as a user would run it, since that's the actual
//! contract Phase 1 promises.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn import_then_sell_then_roi_then_report_is_consistent_end_to_end() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("workflow.db");

    // 1. Import the two-row acquisition fixture: LeBron (500+25+10 = 535.00
    //    cost basis) and Curry (80+5 = 85.00 cost basis).
    cardroi(&db)
        .args([
            "import",
            "--file",
            fixture("import_sample.csv").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("2 row(s)"));

    // 2. Sell the LeBron holding (holding #1, first row imported) for a
    //    known profit: 700.00 - 20.00 fees = 680.00 net proceeds.
    cardroi(&db)
        .args([
            "sell",
            "--holding-id",
            "1",
            "--price",
            "700.00",
            "--fees",
            "20.00",
        ])
        .assert()
        .success();

    // 3. roi --card-id 1 (LeBron) should show the realized profit:
    //    680.00 - 535.00 = 145.00.
    cardroi(&db)
        .args(["roi", "--card-id", "1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("145.00"));

    // 4. Portfolio roi: realized P&L is still just the 145.00 from the one
    //    sale; Curry's 85.00 cost basis is still open capital, not P&L.
    cardroi(&db)
        .arg("roi")
        .assert()
        .success()
        .stdout(predicates::str::contains("145.00"))
        .stdout(predicates::str::contains("85.00"));

    // 5. report --format json: cross-check the same numbers appear in the
    //    structured export, not just the human-readable roi output.
    let output = cardroi(&db)
        .args(["report", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(report["portfolio"]["realized_pnl"], "145.00");
    assert_eq!(report["portfolio"]["open_cost_basis"], "85.00");
    assert_eq!(report["portfolio"]["closed_count"], 1);
    assert_eq!(report["portfolio"]["holding_count"], 2);

    let lebron = report["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["card_name"].as_str().unwrap().contains("LeBron"))
        .expect("LeBron row present in report");
    assert_eq!(lebron["realized_pnl"], "145.00");
    assert_eq!(lebron["cost_basis"], "535.00");
    assert_eq!(lebron["proceeds"], "680.00");
}
