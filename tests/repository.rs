//! Integration tests for the repository layer against a real in-memory
//! SQLite database (no mocks — see test-driven-development's "prefer real
//! implementations" guidance). Covers CRUD round-trips per entity and the
//! atomicity of the composite `record_acquisition`/`record_sale` operations.

use std::str::FromStr;

use cardroi::db::open_in_memory;
use cardroi::db::repository::Repository;
use cardroi::models::{
    HoldingStatus, Money, NewAppraisal, NewCard, NewHolding, NewSet, NewTransaction,
    TransactionType,
};

fn repo() -> Repository {
    Repository::new(open_in_memory().expect("in-memory db should open"))
}

fn seed_set(repo: &Repository) -> i64 {
    repo.create_set(&NewSet {
        name: "2023 Topps Chrome".to_string(),
        sport: "Basketball".to_string(),
        year: Some(2023),
        brand: Some("Topps".to_string()),
        total_cards: Some(220),
        notes: None,
    })
    .expect("set should be created")
    .id
}

fn seed_card(repo: &Repository, set_id: i64) -> i64 {
    repo.create_card(&NewCard {
        set_id,
        card_number: "123".to_string(),
        player_name: "LeBron James".to_string(),
        variant: Some("Refractor".to_string()),
        parallel_name: Some("Gold".to_string()),
        print_run: Some(25),
        is_rookie: false,
        is_autograph: false,
        is_relic: false,
        notes: None,
    })
    .expect("card should be created")
    .id
}

fn money(s: &str) -> Money {
    Money::from_str(s).unwrap()
}

fn seed_holding(repo: &Repository, card_id: i64) -> i64 {
    repo.create_holding(&NewHolding {
        card_id,
        ..Default::default()
    })
    .expect("holding should be created")
    .id
}

fn date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

// --- sets ---

#[test]
fn set_round_trips_through_create_get_list_delete() {
    let repo = repo();
    let id = seed_set(&repo);

    let fetched = repo.get_set(id).unwrap();
    assert_eq!(fetched.name, "2023 Topps Chrome");
    assert_eq!(repo.list_sets().unwrap().len(), 1);

    repo.delete_set(id).unwrap();
    assert!(repo.list_sets().unwrap().is_empty());
}

#[test]
fn create_set_rejects_invalid_input_without_writing() {
    let repo = repo();
    let result = repo.create_set(&NewSet {
        name: "".to_string(),
        sport: "Basketball".to_string(),
        ..Default::default()
    });
    assert!(result.is_err());
    assert!(repo.list_sets().unwrap().is_empty());
}

#[test]
fn get_set_on_missing_id_returns_not_found() {
    let repo = repo();
    assert!(repo.get_set(999).is_err());
}

// --- cards ---

#[test]
fn card_round_trips_through_create_get_list_delete() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);

    let fetched = repo.get_card(card_id).unwrap();
    assert_eq!(fetched.player_name, "LeBron James");
    assert_eq!(repo.list_cards(Some(set_id)).unwrap().len(), 1);
    assert_eq!(repo.list_cards(None).unwrap().len(), 1);

    repo.delete_card(card_id).unwrap();
    assert!(repo.list_cards(Some(set_id)).unwrap().is_empty());
}

#[test]
fn find_card_locates_existing_catalog_entry_by_natural_key() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);

    let found = repo
        .find_card(set_id, "123", Some("Refractor"), Some("Gold"))
        .unwrap();
    assert_eq!(found.map(|c| c.id), Some(card_id));
}

#[test]
fn find_card_returns_none_for_unknown_card() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let found = repo.find_card(set_id, "999", None, None).unwrap();
    assert!(found.is_none());
}

// --- holdings ---

#[test]
fn holding_round_trips_through_create_get_list_delete() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);

    let holding = repo
        .create_holding(&NewHolding {
            card_id,
            serial_number: Some("12/25".to_string()),
            grade: Some("10".to_string()),
            grading_company: Some("PSA".to_string()),
            cert_number: None,
            acquired_date: None,
            notes: None,
        })
        .unwrap();
    assert_eq!(holding.status, HoldingStatus::Owned);

    assert_eq!(repo.list_holdings(Some(card_id), None).unwrap().len(), 1);
    assert_eq!(
        repo.list_holdings(None, Some(HoldingStatus::Owned))
            .unwrap()
            .len(),
        1
    );
    assert!(
        repo.list_holdings(None, Some(HoldingStatus::Sold))
            .unwrap()
            .is_empty()
    );

    repo.delete_holding(holding.id).unwrap();
    assert!(repo.list_holdings(Some(card_id), None).unwrap().is_empty());
}

#[test]
fn list_holdings_page_orders_most_recent_first_and_respects_limit_offset() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let mut ids = Vec::new();
    for i in 1..=5 {
        let holding = repo
            .create_holding(&NewHolding {
                card_id,
                acquired_date: Some(
                    chrono::NaiveDate::from_ymd_opt(2026, 1, i).expect("valid date"),
                ),
                ..Default::default()
            })
            .unwrap();
        ids.push(holding.id);
    }

    let page = repo
        .list_holdings_page(None, None, None, None, 2, 0)
        .unwrap();
    // Most recently acquired (Jan 5) first, not insertion order.
    assert_eq!(
        page.iter().map(|h| h.id).collect::<Vec<_>>(),
        vec![ids[4], ids[3]]
    );

    let next_page = repo
        .list_holdings_page(None, None, None, None, 2, 2)
        .unwrap();
    assert_eq!(
        next_page.iter().map(|h| h.id).collect::<Vec<_>>(),
        vec![ids[2], ids[1]]
    );

    assert_eq!(repo.count_holdings_page(None, None, None, None).unwrap(), 5);
}

#[test]
fn list_holdings_page_rejects_a_negative_limit_or_offset_instead_of_returning_everything() {
    // SQLite treats a negative `LIMIT` as "unlimited," not an error -
    // silently returning every row instead of rejecting a bad page
    // request. Found by direct audit, not currently reachable through the
    // GUI (its page index is already clamped to zero-or-above).
    let repo = repo();
    let err = repo
        .list_holdings_page(None, None, None, None, -1, 0)
        .unwrap_err();
    assert!(err.to_string().contains("non-negative"));

    let err = repo
        .list_holdings_page(None, None, None, None, 10, -1)
        .unwrap_err();
    assert!(err.to_string().contains("non-negative"));
}

#[test]
fn list_holdings_page_filters_by_set_player_and_sport_exactly_like_attribution_grouping() {
    let repo = repo();
    let set_a = repo
        .create_set(&NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id;
    let set_b = repo
        .create_set(&NewSet {
            name: "2024 Bowman".to_string(),
            sport: "Baseball".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id;
    let lebron_a = repo
        .create_card(&NewCard {
            set_id: set_a,
            card_number: "1".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id;
    let curry_a = repo
        .create_card(&NewCard {
            set_id: set_a,
            card_number: "2".to_string(),
            player_name: "Stephen Curry".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id;
    let rookie_b = repo
        .create_card(&NewCard {
            set_id: set_b,
            card_number: "1".to_string(),
            player_name: "Rookie Player".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id;
    seed_holding(&repo, lebron_a);
    seed_holding(&repo, curry_a);
    seed_holding(&repo, rookie_b);

    assert_eq!(
        repo.count_holdings_page(None, Some(set_a), None, None)
            .unwrap(),
        2
    );
    assert_eq!(
        repo.count_holdings_page(None, None, Some("Stephen Curry"), None)
            .unwrap(),
        1
    );
    assert_eq!(
        repo.count_holdings_page(None, None, None, Some("Baseball"))
            .unwrap(),
        1
    );
}

#[test]
fn delete_holding_is_restricted_when_transactions_exist() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("10.00"),
                ..Default::default()
            },
        )
        .unwrap();

    let result = repo.delete_holding(holding.id);

    assert!(
        result.is_err(),
        "deleting a holding with transaction history must not silently erase the audit trail"
    );
    assert_eq!(repo.get_holding(holding.id).unwrap().id, holding.id);
}

#[test]
fn delete_holding_cascade_removes_the_holding_and_its_transactions() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, txn) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("10.00"),
                ..Default::default()
            },
        )
        .unwrap();

    repo.delete_holding_cascade(holding.id).unwrap();

    assert!(repo.get_holding(holding.id).is_err());
    assert!(repo.get_transaction(txn.id).is_err());
}

#[test]
fn delete_holding_cascade_also_removes_its_appraisals() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("10.00"),
                ..Default::default()
            },
        )
        .unwrap();
    let appraisal = repo
        .create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("15.00"),
            ..Default::default()
        })
        .unwrap();

    repo.delete_holding_cascade(holding.id).unwrap();

    assert!(repo.get_appraisal(appraisal.id).is_err());
}

#[test]
fn delete_holding_cascade_on_missing_id_returns_not_found_and_writes_nothing() {
    let repo = repo();
    assert!(repo.delete_holding_cascade(999).is_err());
}

#[test]
fn update_holding_changes_physical_attributes_but_not_status_or_card() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    let updated = repo
        .update_holding(
            holding_id,
            &cardroi::models::HoldingEdit {
                serial_number: Some("7/25".to_string()),
                grade: Some("9.5".to_string()),
                grading_company: Some("BGS".to_string()),
                cert_number: Some("999888".to_string()),
                notes: Some("re-graded".to_string()),
            },
        )
        .unwrap();

    assert_eq!(updated.serial_number.as_deref(), Some("7/25"));
    assert_eq!(updated.grade.as_deref(), Some("9.5"));
    assert_eq!(updated.grading_company.as_deref(), Some("BGS"));
    assert_eq!(updated.cert_number.as_deref(), Some("999888"));
    assert_eq!(updated.notes.as_deref(), Some("re-graded"));
    assert_eq!(updated.card_id, card_id);
    assert_eq!(updated.status, HoldingStatus::Owned);
}

#[test]
fn update_holding_rejects_grade_without_grading_company_and_writes_nothing() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    let result = repo.update_holding(
        holding_id,
        &cardroi::models::HoldingEdit {
            grade: Some("10".to_string()),
            grading_company: None,
            ..Default::default()
        },
    );

    assert!(result.is_err());
    assert_eq!(repo.get_holding(holding_id).unwrap().grade, None);
}

#[test]
fn update_holding_on_missing_id_returns_not_found() {
    let repo = repo();
    assert!(
        repo.update_holding(999, &cardroi::models::HoldingEdit::default())
            .is_err()
    );
}

#[test]
fn update_transaction_corrects_price_and_recomputes_total() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (_, txn) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                fees: money("5.00"),
                transaction_date: date("2026-01-01"),
                ..Default::default()
            },
        )
        .unwrap();

    let updated = repo
        .update_transaction(
            txn.id,
            &cardroi::models::TransactionEdit {
                transaction_date: date("2026-01-02"),
                price: money("120.00"),
                fees: money("5.00"),
                shipping: Money::ZERO,
                tax: Money::ZERO,
                other_cost: Money::ZERO,
                currency: "USD".to_string(),
                counterparty: None,
                platform: None,
                external_ref: None,
                notes: Some("fixed a typo".to_string()),
            },
        )
        .unwrap();

    // Corrected price 120.00 + fees 5.00 = 125.00 total, matching
    // NewTransaction::total()'s exact formula for an acquisition.
    assert_eq!(updated.price, money("120.00"));
    assert_eq!(updated.total, money("125.00"));
    assert_eq!(updated.transaction_date, date("2026-01-02"));
    assert_eq!(updated.notes.as_deref(), Some("fixed a typo"));
    // type/holding_id are not part of the edit surface - must survive
    // untouched.
    assert_eq!(updated.transaction_type, TransactionType::Acquisition);
    assert_eq!(updated.holding_id, txn.holding_id);
}

#[test]
fn update_transaction_rejects_negative_price_and_writes_nothing() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (_, txn) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();

    let result = repo.update_transaction(
        txn.id,
        &cardroi::models::TransactionEdit {
            transaction_date: txn.transaction_date,
            price: -money("1.00"),
            fees: Money::ZERO,
            shipping: Money::ZERO,
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            currency: "USD".to_string(),
            counterparty: None,
            platform: None,
            external_ref: None,
            notes: None,
        },
    );

    assert!(result.is_err());
    assert_eq!(repo.get_transaction(txn.id).unwrap().price, money("100.00"));
}

#[test]
fn update_transaction_on_missing_id_returns_not_found() {
    let repo = repo();
    assert!(
        repo.update_transaction(
            999,
            &cardroi::models::TransactionEdit {
                transaction_date: date("2026-01-01"),
                price: Money::ZERO,
                fees: Money::ZERO,
                shipping: Money::ZERO,
                tax: Money::ZERO,
                other_cost: Money::ZERO,
                currency: "USD".to_string(),
                counterparty: None,
                platform: None,
                external_ref: None,
                notes: None,
            }
        )
        .is_err()
    );
}

#[test]
fn record_loss_updates_status_disposed_date_and_creates_a_disposition() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();

    let disposed = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
    let txn = repo
        .record_loss(
            holding.id,
            HoldingStatus::Lost,
            disposed,
            Money::ZERO,
            Money::ZERO,
            Some("stolen".to_string()),
            None,
        )
        .unwrap();

    let updated = repo.get_holding(holding.id).unwrap();
    assert_eq!(updated.status, HoldingStatus::Lost);
    assert_eq!(updated.disposed_date, Some(disposed));
    assert_eq!(
        txn.transaction_type,
        TransactionType::Disposition,
        "a loss must be a real ledger transaction, not just a status flip"
    );
    assert_eq!(txn.total, Money::ZERO);
    assert_eq!(txn.loss_cause.as_deref(), Some("stolen"));
}

#[test]
fn record_loss_captures_residual_value_and_insurance_recovery_separately() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();

    let txn = repo
        .record_loss(
            holding.id,
            HoldingStatus::Damaged,
            chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            money("20.00"),
            money("30.00"),
            Some("water damage".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(txn.residual_value, Some(money("20.00")));
    assert_eq!(txn.insurance_recovery, Some(money("30.00")));
    // total (proceeds) is the sum of the two, not either alone.
    assert_eq!(txn.total, money("50.00"));
}

#[test]
fn record_loss_is_rejected_on_an_already_sold_holding() {
    // A sold holding has a real disposition transaction on record - silently
    // overwriting its status to lost/damaged would make holding_pnl's
    // realized_pnl vanish from every report even though the money already
    // changed hands. Mirrors record_sale's own "AND status = 'owned'" guard.
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_sale(NewTransaction {
        holding_id: holding.id,
        price: money("150.00"),
        ..Default::default()
    })
    .unwrap();

    let result = repo.record_loss(
        holding.id,
        HoldingStatus::Lost,
        chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        Money::ZERO,
        Money::ZERO,
        None,
        None,
    );

    assert!(result.is_err());
    let unchanged = repo.get_holding(holding.id).unwrap();
    assert_eq!(
        unchanged.status,
        HoldingStatus::Sold,
        "a sold holding's status must not be overwritable"
    );
}

#[test]
fn record_loss_is_rejected_when_already_lost_or_damaged() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_loss(
        holding.id,
        HoldingStatus::Lost,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        Money::ZERO,
        Money::ZERO,
        None,
        None,
    )
    .unwrap();

    let result = repo.record_loss(
        holding.id,
        HoldingStatus::Damaged,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
        Money::ZERO,
        Money::ZERO,
        None,
        None,
    );

    assert!(result.is_err());
    let unchanged = repo.get_holding(holding.id).unwrap();
    assert_eq!(unchanged.status, HoldingStatus::Lost);
}

#[test]
fn record_loss_rejects_a_target_status_other_than_lost_or_damaged() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding = repo
        .create_holding(&NewHolding {
            card_id,
            ..Default::default()
        })
        .unwrap();

    let result = repo.record_loss(
        holding.id,
        HoldingStatus::Owned,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        Money::ZERO,
        Money::ZERO,
        None,
        None,
    );

    assert!(result.is_err());
}

// --- transactions ---

#[test]
fn transaction_round_trips_and_computes_total() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding = repo
        .create_holding(&NewHolding {
            card_id,
            ..Default::default()
        })
        .unwrap();

    let txn = repo
        .create_transaction(&NewTransaction {
            holding_id: holding.id,
            transaction_type: TransactionType::Acquisition,
            price: money("100.00"),
            fees: money("5.00"),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(txn.total, money("105.00"));
    assert_eq!(
        repo.list_transactions_for_holding(holding.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn list_transactions_page_orders_most_recent_first_and_respects_limit_offset() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding = repo
        .create_holding(&NewHolding {
            card_id,
            ..Default::default()
        })
        .unwrap();
    let mut ids = Vec::new();
    for i in 1..=5 {
        let txn = repo
            .create_transaction(&NewTransaction {
                holding_id: holding.id,
                transaction_type: TransactionType::Acquisition,
                transaction_date: chrono::NaiveDate::from_ymd_opt(2026, 1, i).expect("valid date"),
                price: money("10.00"),
                ..Default::default()
            })
            .unwrap();
        ids.push(txn.id);
    }

    let page = repo.list_transactions_page(None, None, None, 2, 0).unwrap();
    // Most recent (Jan 5) first, not insertion order.
    assert_eq!(
        page.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![ids[4], ids[3]]
    );

    let next_page = repo.list_transactions_page(None, None, None, 2, 2).unwrap();
    assert_eq!(
        next_page.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![ids[2], ids[1]]
    );

    assert_eq!(repo.count_transactions_page(None, None, None).unwrap(), 5);
}

#[test]
fn list_transactions_page_rejects_a_negative_limit_or_offset_instead_of_returning_everything() {
    let repo = repo();
    let err = repo
        .list_transactions_page(None, None, None, -1, 0)
        .unwrap_err();
    assert!(err.to_string().contains("non-negative"));

    let err = repo
        .list_transactions_page(None, None, None, 10, -1)
        .unwrap_err();
    assert!(err.to_string().contains("non-negative"));
}

#[test]
fn list_transactions_page_filters_by_type() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_sale(NewTransaction {
        holding_id: holding.id,
        price: money("150.00"),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(
        repo.count_transactions_page(Some(TransactionType::Acquisition), None, None)
            .unwrap(),
        1
    );
    assert_eq!(
        repo.count_transactions_page(Some(TransactionType::Disposition), None, None)
            .unwrap(),
        1
    );
    assert_eq!(repo.count_transactions_page(None, None, None).unwrap(), 2);
}

// --- composite operations ---

#[test]
fn record_acquisition_creates_holding_and_transaction_together() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);

    let (holding, txn) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("50.00"),
                fees: money("2.50"),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(holding.status, HoldingStatus::Owned);
    assert_eq!(txn.transaction_type, TransactionType::Acquisition);
    assert_eq!(txn.total, money("52.50"));
    assert_eq!(txn.holding_id, holding.id);
}

#[test]
fn record_acquisition_rolls_back_holding_when_transaction_is_invalid() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);

    let result = repo.record_acquisition(
        &NewHolding {
            card_id,
            ..Default::default()
        },
        NewTransaction {
            price: -money("10.00"), // invalid: negative price
            ..Default::default()
        },
    );

    assert!(result.is_err());
    assert!(
        repo.list_holdings(Some(card_id), None).unwrap().is_empty(),
        "the holding insert must not survive a failed transaction insert"
    );
}

#[test]
fn record_sale_flips_holding_to_sold_and_inserts_disposition() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();

    let sale = repo
        .record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("150.00"),
            fees: money("10.00"),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(sale.transaction_type, TransactionType::Disposition);
    assert_eq!(sale.total, money("140.00"));

    let updated_holding = repo.get_holding(holding.id).unwrap();
    assert_eq!(updated_holding.status, HoldingStatus::Sold);
    assert!(updated_holding.disposed_date.is_some());
}

#[test]
fn record_sale_on_an_already_sold_holding_is_rejected() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let (holding, _) = repo
        .record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("100.00"),
                ..Default::default()
            },
        )
        .unwrap();
    repo.record_sale(NewTransaction {
        holding_id: holding.id,
        price: money("150.00"),
        ..Default::default()
    })
    .unwrap();

    let result = repo.record_sale(NewTransaction {
        holding_id: holding.id,
        price: money("200.00"),
        ..Default::default()
    });

    assert!(result.is_err(), "selling an already-sold holding must fail");
    assert_eq!(
        repo.list_transactions_for_holding(holding.id)
            .unwrap()
            .len(),
        2,
        "the rejected second sale must not insert a transaction"
    );
}

#[test]
fn record_sale_on_nonexistent_holding_writes_nothing() {
    let repo = repo();

    let result = repo.record_sale(NewTransaction {
        holding_id: 999,
        price: money("10.00"),
        ..Default::default()
    });

    assert!(result.is_err());
    assert!(
        repo.list_transactions(None, None, None).unwrap().is_empty(),
        "no transaction should be written when the target holding doesn't exist"
    );
}

// --- batch import ---

use cardroi::db::repository::AcquisitionImportRow;

fn import_row(set_name: &str, card_number: &str, price: &str) -> AcquisitionImportRow {
    AcquisitionImportRow {
        set: NewSet {
            name: set_name.to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        },
        card: NewCard {
            card_number: card_number.to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        },
        holding: NewHolding::default(),
        transaction: NewTransaction {
            price: money(price),
            ..Default::default()
        },
    }
}

#[test]
fn import_acquisitions_creates_set_card_holding_and_transaction() {
    let repo = repo();
    let summary = repo
        .import_acquisitions(&[import_row("2023 Topps Chrome", "123", "100.00")])
        .unwrap();

    assert_eq!(summary.rows_imported, 1);
    assert_eq!(summary.sets_created, 1);
    assert_eq!(summary.cards_created, 1);
    assert_eq!(repo.list_sets().unwrap().len(), 1);
    assert_eq!(repo.list_cards(None).unwrap().len(), 1);
    assert_eq!(repo.list_holdings(None, None).unwrap().len(), 1);
}

#[test]
fn import_acquisitions_dedups_set_and_card_across_rows() {
    let repo = repo();
    let rows = vec![
        import_row("2023 Topps Chrome", "123", "100.00"),
        import_row("2023 Topps Chrome", "123", "110.00"),
    ];

    let summary = repo.import_acquisitions(&rows).unwrap();

    assert_eq!(summary.rows_imported, 2);
    assert_eq!(summary.sets_created, 1, "same set across rows dedups");
    assert_eq!(summary.cards_created, 1, "same card across rows dedups");
    assert_eq!(
        repo.list_holdings(None, None).unwrap().len(),
        2,
        "each row still creates its own holding - importing twice means bought twice"
    );
}

#[test]
fn import_acquisitions_is_all_or_nothing_on_a_bad_row() {
    let repo = repo();
    let mut bad_row = import_row("2024 Bowman", "1", "50.00");
    bad_row.transaction.price = -money("1.00"); // invalid: negative price
    let rows = vec![import_row("2023 Topps Chrome", "123", "100.00"), bad_row];

    let result = repo.import_acquisitions(&rows);

    assert!(result.is_err());
    assert!(
        repo.list_sets().unwrap().is_empty(),
        "the first, otherwise-valid row must not survive a later row's failure"
    );
    assert!(repo.list_holdings(None, None).unwrap().is_empty());
}

// --- appraisals ---

#[test]
fn appraisal_round_trips_through_create_get_list_delete() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    let appraisal = repo
        .create_appraisal(&NewAppraisal {
            holding_id,
            appraised_value: money("650.00"),
            appraised_date: date("2026-01-01"),
            source: Some("PSA pop report comp".to_string()),
            notes: None,
        })
        .unwrap();

    let fetched = repo.get_appraisal(appraisal.id).unwrap();
    assert_eq!(fetched.appraised_value, money("650.00"));
    assert_eq!(
        repo.list_appraisals_for_holding(holding_id).unwrap().len(),
        1
    );

    repo.delete_appraisal(appraisal.id).unwrap();
    assert!(
        repo.list_appraisals_for_holding(holding_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn create_appraisal_rejects_invalid_input_without_writing() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    let result = repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value: -money("1.00"),
        ..Default::default()
    });

    assert!(result.is_err());
    assert!(
        repo.list_appraisals_for_holding(holding_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn get_appraisal_on_missing_id_returns_not_found() {
    let repo = repo();
    assert!(repo.get_appraisal(999).is_err());
}

#[test]
fn latest_appraisal_for_holding_returns_none_when_unappraised() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    assert!(
        repo.latest_appraisal_for_holding(holding_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn latest_appraisal_for_holding_returns_most_recent_by_date() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value: money("500.00"),
        appraised_date: date("2026-01-01"),
        ..Default::default()
    })
    .unwrap();
    repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value: money("700.00"),
        appraised_date: date("2026-06-01"),
        ..Default::default()
    })
    .unwrap();
    // Inserted out of date order to prove we sort by appraised_date, not id.
    repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value: money("600.00"),
        appraised_date: date("2026-03-01"),
        ..Default::default()
    })
    .unwrap();

    let latest = repo
        .latest_appraisal_for_holding(holding_id)
        .unwrap()
        .expect("should have an appraisal");
    assert_eq!(latest.appraised_value, money("700.00"));
    assert_eq!(latest.appraised_date, date("2026-06-01"));
}

#[test]
fn list_appraisals_for_holding_is_scoped_to_that_holding() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_a = seed_holding(&repo, card_id);
    let holding_b = seed_holding(&repo, card_id);

    repo.create_appraisal(&NewAppraisal {
        holding_id: holding_a,
        appraised_value: money("500.00"),
        appraised_date: date("2026-01-01"),
        ..Default::default()
    })
    .unwrap();
    repo.create_appraisal(&NewAppraisal {
        holding_id: holding_b,
        appraised_value: money("900.00"),
        appraised_date: date("2026-01-01"),
        ..Default::default()
    })
    .unwrap();

    let for_a = repo.list_appraisals_for_holding(holding_a).unwrap();
    assert_eq!(for_a.len(), 1);
    assert_eq!(for_a[0].appraised_value, money("500.00"));
}

#[test]
fn deleting_a_holding_cascades_its_appraisals() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value: money("500.00"),
        appraised_date: date("2026-01-01"),
        ..Default::default()
    })
    .unwrap();

    repo.delete_holding(holding_id).unwrap();

    assert!(
        repo.list_appraisals_for_holding(holding_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn schema_rejects_a_transaction_whose_total_disagrees_with_its_components() {
    let repo = repo();
    let set_id = seed_set(&repo);
    let card_id = seed_card(&repo, set_id);
    let holding_id = seed_holding(&repo, card_id);

    // A correct acquisition total (price + fees + shipping + tax + other_cost)
    // must still succeed after adding the CHECK constraint.
    repo.connection()
        .execute(
            "INSERT INTO transactions
                (holding_id, transaction_type, transaction_date,
                 price_cents, fees_cents, shipping_cents, tax_cents, other_cost_cents,
                 total_cents, currency)
             VALUES (?1, 'acquisition', '2026-01-01', 10000, 500, 0, 0, 0, 10500, 'USD')",
            [holding_id],
        )
        .expect("a total consistent with its components must be accepted");

    // A total that disagrees with price+fees+shipping+tax+other_cost must be
    // rejected at the database level, not just by the app-layer NewTransaction::total().
    let err = repo
        .connection()
        .execute(
            "INSERT INTO transactions
                (holding_id, transaction_type, transaction_date,
                 price_cents, fees_cents, shipping_cents, tax_cents, other_cost_cents,
                 total_cents, currency)
             VALUES (?1, 'acquisition', '2026-01-01', 10000, 500, 0, 0, 0, 999999, 'USD')",
            [holding_id],
        )
        .expect_err(
            "a total inconsistent with its components must be rejected by the CHECK constraint",
        );

    assert!(
        err.to_string().contains("CHECK constraint failed"),
        "expected a CHECK constraint failure, got: {err}"
    );
}
