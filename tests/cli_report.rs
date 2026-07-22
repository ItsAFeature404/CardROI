//! `cardroi report`.

use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn cardroi(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cardroi").unwrap();
    cmd.arg("--db").arg(db);
    cmd
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Card 1 (LeBron): bought 100.00, sold 150.00 -> 50.00 realized profit.
/// Card 2 (Curry): bought 40.00, still owned.
fn seed_report_fixture_data(db: &Path) {
    let repo = cardroi::db::repository::Repository::new(cardroi::db::open(db).unwrap());
    let set = repo
        .create_set(&cardroi::models::NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap();
    let card_a = repo
        .create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap();
    let (holding_a, _) = repo
        .record_acquisition(
            &cardroi::models::NewHolding {
                card_id: card_a.id,
                ..Default::default()
            },
            cardroi::models::NewTransaction {
                price: "100.00".parse().unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_sale(cardroi::models::NewTransaction {
        holding_id: holding_a.id,
        price: "150.00".parse().unwrap(),
        ..Default::default()
    })
    .unwrap();

    let card_b = repo
        .create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "45".to_string(),
            player_name: "Stephen Curry".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.record_acquisition(
        &cardroi::models::NewHolding {
            card_id: card_b.id,
            ..Default::default()
        },
        cardroi::models::NewTransaction {
            price: "40.00".parse().unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn csv_report_matches_golden_fixture_exactly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_report_fixture_data(&db);

    let output = cardroi(&db)
        .args(["report", "--format", "csv"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(stdout, fixture("report_expected.csv"));
}

#[test]
fn json_report_matches_golden_fixture_exactly() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_report_fixture_data(&db);

    let output = cardroi(&db)
        .args(["report", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(
        stdout.trim_end(),
        fixture("report_expected.json").trim_end()
    );
}

#[test]
fn table_report_shows_portfolio_summary_and_per_card_breakdown() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_report_fixture_data(&db);

    cardroi(&db)
        .arg("report")
        .assert()
        .success()
        .stdout(predicates::str::contains("LeBron James"))
        .stdout(predicates::str::contains("Curry"))
        .stdout(predicates::str::contains("50.00")); // portfolio realized P&L
}

#[test]
fn output_flag_writes_to_a_file_instead_of_stdout() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_report_fixture_data(&db);
    let out_path = dir.path().join("report.csv");

    cardroi(&db)
        .args([
            "report",
            "--format",
            "csv",
            "--output",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::is_empty());

    let written = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(written, fixture("report_expected.csv"));
}

#[test]
fn table_report_shows_allocation_concentration_and_attribution() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    seed_report_fixture_data(&db);

    // Card 1 (LeBron) was sold, so it's excluded from current allocation;
    // Card 2 (Curry) is the only owned holding, so it's 100% of the
    // tracked portfolio and the concentration is maximal (a single
    // position). Attribution, unlike allocation, is all-time and includes
    // both players.
    cardroi(&db)
        .arg("report")
        .assert()
        .success()
        .stdout(predicates::str::contains("Allocation by card"))
        .stdout(predicates::str::contains("Stephen Curry"))
        .stdout(predicates::str::contains("Allocation by set"))
        .stdout(predicates::str::contains("2023 Topps Chrome"))
        .stdout(predicates::str::contains("Concentration risk"))
        .stdout(predicates::str::contains("Effective positions: 1.00"))
        .stdout(predicates::str::contains("Attribution by player"))
        .stdout(predicates::str::contains("LeBron James"))
        .stdout(predicates::str::contains("Attribution by sport"))
        .stdout(predicates::str::contains("Basketball"));
}

#[test]
fn effective_positions_rounds_instead_of_truncating() {
    // A 90/10 cost-basis split gives HHI = 0.9^2 + 0.1^2 = 0.82 exactly,
    // so effective positions = 1/0.82 = 1.219512...19 - correct rounding
    // gives 1.22 (third decimal is 9), truncating gives the wrong 1.21.
    // Same latent defect as commands::roi::as_percent's fix, caught here
    // too while cross-checking the desktop Risk/Allocation screen.
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
    let card_a = repo
        .create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "1".to_string(),
            player_name: "Player A".to_string(),
            ..Default::default()
        })
        .unwrap();
    let card_b = repo
        .create_card(&cardroi::models::NewCard {
            set_id: set.id,
            card_number: "2".to_string(),
            player_name: "Player B".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.record_acquisition(
        &cardroi::models::NewHolding {
            card_id: card_a.id,
            ..Default::default()
        },
        cardroi::models::NewTransaction {
            price: cardroi::models::Money::from_cents(90000),
            ..Default::default()
        },
    )
    .unwrap();
    repo.record_acquisition(
        &cardroi::models::NewHolding {
            card_id: card_b.id,
            ..Default::default()
        },
        cardroi::models::NewTransaction {
            price: cardroi::models::Money::from_cents(10000),
            ..Default::default()
        },
    )
    .unwrap();

    cardroi(&db)
        .arg("report")
        .assert()
        .success()
        .stdout(predicates::str::contains("Effective positions: 1.22"));
}
