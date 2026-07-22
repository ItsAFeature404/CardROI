//! Atomic batch import of acquisitions. Written as its own transaction
//! rather than composing `create_set`/`create_card`/`record_acquisition`,
//! because rusqlite's `unchecked_transaction()` can't nest — those methods
//! each open their own. This module reuses the same row-insert helpers
//! those methods use (`insert_holding_row`/`insert_transaction_row`) so an
//! entire import is exactly one DB transaction: any bad row rolls back
//! everything already inserted from that call, not just that row.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;
use crate::models::{NewCard, NewHolding, NewSet, NewTransaction, TransactionType};

use super::Repository;
use super::holdings::insert_holding_row;
use super::transactions::insert_transaction_row;

/// One row's worth of data for [`Repository::import_acquisitions`].
/// `card.set_id`, `holding.card_id`, and `transaction.holding_id` are
/// ignored — they're resolved during import (find-or-create the set/card,
/// always create a new holding).
#[derive(Debug, Clone)]
pub struct AcquisitionImportRow {
    pub set: NewSet,
    pub card: NewCard,
    pub holding: NewHolding,
    pub transaction: NewTransaction,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub rows_imported: usize,
    pub sets_created: usize,
    pub cards_created: usize,
}

impl Repository {
    /// Atomically imports a batch of acquisitions. For each row: finds or
    /// creates the set (dedup by name+sport+year) and card (dedup by
    /// set+number+variant+parallel), then always creates a new holding and
    /// its founding acquisition transaction — importing the same file twice
    /// intentionally creates duplicate holdings (it means you bought it
    /// twice), even though the catalog entries themselves dedup.
    pub fn import_acquisitions(&self, rows: &[AcquisitionImportRow]) -> Result<ImportSummary> {
        for row in rows {
            row.set.validate()?;
            // card/holding/transaction carry placeholder FK ids (0) until
            // resolved below, so their FK-id checks can't run yet; only the
            // non-FK fields are worth validating this early.
        }

        let tx = self.conn.unchecked_transaction()?;
        let mut summary = ImportSummary::default();

        for row in rows {
            let set_id = find_or_insert_set(&tx, &row.set, &mut summary)?;

            let mut card = row.card.clone();
            card.set_id = set_id;
            card.validate()?;
            let card_id = find_or_insert_card(&tx, &card, &mut summary)?;

            let mut holding = row.holding.clone();
            holding.card_id = card_id;
            holding.validate()?;
            let holding_id = insert_holding_row(&tx, &holding)?;

            let mut txn = row.transaction.clone();
            txn.holding_id = holding_id;
            txn.transaction_type = TransactionType::Acquisition;
            txn.validate()?;
            insert_transaction_row(&tx, &txn)?;

            summary.rows_imported += 1;
        }

        tx.commit()?;
        Ok(summary)
    }
}

/// One row's worth of data for [`Repository::import_checklist`].
/// `card.set_id` is ignored — resolved during import, same as
/// [`AcquisitionImportRow`].
#[derive(Debug, Clone)]
pub struct ChecklistImportRow {
    pub set: NewSet,
    pub card: NewCard,
}

impl Repository {
    /// Atomically imports set/card catalog entries only — no holdings, no
    /// transactions. For pre-loading a checklist before owning any of it.
    /// Dedups sets/cards by the same natural keys as
    /// [`Repository::import_acquisitions`].
    pub fn import_checklist(&self, rows: &[ChecklistImportRow]) -> Result<ImportSummary> {
        for row in rows {
            row.set.validate()?;
        }

        let tx = self.conn.unchecked_transaction()?;
        let mut summary = ImportSummary::default();

        for row in rows {
            let set_id = find_or_insert_set(&tx, &row.set, &mut summary)?;

            let mut card = row.card.clone();
            card.set_id = set_id;
            card.validate()?;
            find_or_insert_card(&tx, &card, &mut summary)?;

            summary.rows_imported += 1;
        }

        tx.commit()?;
        Ok(summary)
    }
}

/// Finds a set by its natural key (`name`, `sport`, `year`) or inserts it.
/// Takes `&Connection` so it works both standalone and nested inside an
/// active `Transaction` (which derefs to `Connection`).
fn find_or_insert_set(
    conn: &Connection,
    new_set: &NewSet,
    summary: &mut ImportSummary,
) -> Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM sets WHERE name = ?1 AND sport = ?2 AND year IS ?3",
            params![new_set.name, new_set.sport, new_set.year],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO sets (name, sport, year, brand, total_cards, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new_set.name,
            new_set.sport,
            new_set.year,
            new_set.brand,
            new_set.total_cards,
            new_set.notes,
        ],
    )?;
    summary.sets_created += 1;
    Ok(conn.last_insert_rowid())
}

/// Finds a card by its natural key (`set_id`, `card_number`, `variant`,
/// `parallel_name`) or inserts it. `new_card.set_id` must already be
/// resolved by the caller.
fn find_or_insert_card(
    conn: &Connection,
    new_card: &NewCard,
    summary: &mut ImportSummary,
) -> Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM cards
             WHERE set_id = ?1 AND card_number = ?2
               AND variant IS ?3 AND parallel_name IS ?4",
            params![
                new_card.set_id,
                new_card.card_number,
                new_card.variant,
                new_card.parallel_name,
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO cards (
            set_id, card_number, player_name, variant, parallel_name,
            print_run, is_rookie, is_autograph, is_relic, notes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            new_card.set_id,
            new_card.card_number,
            new_card.player_name,
            new_card.variant,
            new_card.parallel_name,
            new_card.print_run,
            new_card.is_rookie,
            new_card.is_autograph,
            new_card.is_relic,
            new_card.notes,
        ],
    )?;
    summary.cards_created += 1;
    Ok(conn.last_insert_rowid())
}
