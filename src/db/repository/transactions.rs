use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{CardRoiError, Result};
use crate::models::{
    HoldingStatus, Money, NewHolding, NewTransaction, Transaction, TransactionEdit, TransactionType,
};

use super::holdings::insert_holding_row;
use super::{Repository, parse_date, parse_enum, parse_timestamp};

/// Inserts a transaction row (computing `total` internally) and returns its
/// new id. Takes `&Connection` for the same reason as
/// [`insert_holding_row`] — shared by `create_transaction`,
/// `record_acquisition`, `record_sale`, and `record_loss`.
pub(super) fn insert_transaction_row(conn: &Connection, new_txn: &NewTransaction) -> Result<i64> {
    let total = new_txn.total();
    conn.execute(
        "INSERT INTO transactions (
            holding_id, transaction_type, transaction_date, price_cents,
            fees_cents, shipping_cents, tax_cents, other_cost_cents,
            total_cents, currency, counterparty, platform, external_ref, notes,
            residual_value_cents, insurance_recovery_cents, loss_cause
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            new_txn.holding_id,
            new_txn.transaction_type.as_str(),
            new_txn.transaction_date.to_string(),
            new_txn.price,
            new_txn.fees,
            new_txn.shipping,
            new_txn.tax,
            new_txn.other_cost,
            total,
            new_txn.currency,
            new_txn.counterparty,
            new_txn.platform,
            new_txn.external_ref,
            new_txn.notes,
            new_txn.residual_value,
            new_txn.insurance_recovery,
            new_txn.loss_cause,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Atomically flips a still-owned holding to `new_status`, recording
/// `disposed_date` — shared by `record_sale` (-> `Sold`) and `record_loss`
/// (-> `Lost`/`Damaged`) so every terminal transition uses the exact same
/// guard: only valid from `Owned`, since a holding that's already
/// sold/lost/damaged has a real disposition transaction on record already
/// (overwriting its status a second time would silently drop that P&L from
/// every report).
fn flip_owned_holding(
    conn: &Connection,
    holding_id: i64,
    new_status: HoldingStatus,
    disposed_date: NaiveDate,
) -> Result<()> {
    let current_status: Option<String> = conn
        .query_row(
            "SELECT status FROM holdings WHERE id = ?1",
            params![holding_id],
            |row| row.get(0),
        )
        .optional()?;
    match current_status.as_deref() {
        None => return Err(CardRoiError::not_found("holding", holding_id)),
        Some(s) if s != HoldingStatus::Owned.as_str() => {
            return Err(CardRoiError::validation(format!(
                "holding {holding_id} is not owned (status: {s}); cannot change its status"
            )));
        }
        _ => {}
    }

    let affected = conn.execute(
        "UPDATE holdings
         SET status = ?2, disposed_date = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1 AND status = 'owned'",
        params![holding_id, new_status.as_str(), disposed_date.to_string()],
    )?;
    if affected == 0 {
        return Err(CardRoiError::not_found("holding", holding_id));
    }
    Ok(())
}

impl Repository {
    pub fn create_transaction(&self, new_txn: &NewTransaction) -> Result<Transaction> {
        new_txn.validate()?;
        let id = insert_transaction_row(&self.conn, new_txn)?;
        self.get_transaction(id)
    }

    /// Corrects an existing transaction's own fields after the fact (wrong
    /// price, wrong date, a typo) - see `TransactionEdit`'s doc comment for
    /// exactly what's in and out of scope (not type, not which holding,
    /// not loss-specific fields). `total` is recomputed from the existing
    /// transaction's own `transaction_type`, the same formula
    /// `NewTransaction::total()` uses, so the database's own
    /// total-matches-components CHECK constraint (`0003_transaction_total_
    /// check.sql`) stays satisfied - not just app-layer validation.
    pub fn update_transaction(&self, id: i64, edit: &TransactionEdit) -> Result<Transaction> {
        edit.validate()?;
        let existing = self.get_transaction(id)?;
        let total = edit.total(existing.transaction_type);
        let affected = self.conn.execute(
            "UPDATE transactions SET
                transaction_date = ?2, price_cents = ?3, fees_cents = ?4,
                shipping_cents = ?5, tax_cents = ?6, other_cost_cents = ?7,
                total_cents = ?8, currency = ?9, counterparty = ?10,
                platform = ?11, external_ref = ?12, notes = ?13
             WHERE id = ?1",
            params![
                id,
                edit.transaction_date.to_string(),
                edit.price,
                edit.fees,
                edit.shipping,
                edit.tax,
                edit.other_cost,
                total,
                edit.currency,
                edit.counterparty,
                edit.platform,
                edit.external_ref,
                edit.notes,
            ],
        )?;
        if affected == 0 {
            return Err(CardRoiError::not_found("transaction", id));
        }
        self.get_transaction(id)
    }

    pub fn get_transaction(&self, id: i64) -> Result<Transaction> {
        self.conn
            .query_row(
                "SELECT * FROM transactions WHERE id = ?1",
                params![id],
                row_to_transaction,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardRoiError::not_found("transaction", id),
                other => other.into(),
            })
    }

    pub fn list_transactions_for_holding(&self, holding_id: i64) -> Result<Vec<Transaction>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM transactions WHERE holding_id = ?1 ORDER BY transaction_date ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![holding_id], row_to_transaction)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Lists transactions across the whole ledger, optionally filtered by
    /// type and/or an inclusive date range. Used by reporting.
    pub fn list_transactions(
        &self,
        transaction_type: Option<TransactionType>,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<Vec<Transaction>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM transactions
             WHERE (?1 IS NULL OR transaction_type = ?1)
               AND (?2 IS NULL OR transaction_date >= ?2)
               AND (?3 IS NULL OR transaction_date <= ?3)
             ORDER BY transaction_date ASC, id ASC",
        )?;
        let rows = stmt.query_map(
            params![
                transaction_type.map(TransactionType::as_str),
                from.map(|d| d.to_string()),
                to.map(|d| d.to_string()),
            ],
            row_to_transaction,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One page of transactions across the whole ledger, optionally
    /// filtered by type and/or an inclusive date range - the paginated
    /// counterpart to `list_transactions` (which stays unpaginated,
    /// ascending, for its existing reporting callers). Ordered most-
    /// recent-first, matching this app's other paginated views' "default
    /// sort should reflect business logic" convention.
    pub fn list_transactions_page(
        &self,
        transaction_type: Option<TransactionType>,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Transaction>> {
        // Same guard as `list_holdings_page`: SQLite treats a negative
        // `LIMIT` as "unlimited" rather than an error.
        if limit < 0 || offset < 0 {
            return Err(CardRoiError::validation(format!(
                "limit and offset must be non-negative, got limit={limit}, offset={offset}"
            )));
        }
        let mut stmt = self.conn.prepare(
            "SELECT * FROM transactions
             WHERE (?1 IS NULL OR transaction_type = ?1)
               AND (?2 IS NULL OR transaction_date >= ?2)
               AND (?3 IS NULL OR transaction_date <= ?3)
             ORDER BY transaction_date DESC, id DESC
             LIMIT ?4 OFFSET ?5",
        )?;
        let rows = stmt.query_map(
            params![
                transaction_type.map(TransactionType::as_str),
                from.map(|d| d.to_string()),
                to.map(|d| d.to_string()),
                limit,
                offset,
            ],
            row_to_transaction,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Total row count for the same filter `list_transactions_page` uses.
    pub fn count_transactions_page(
        &self,
        transaction_type: Option<TransactionType>,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM transactions
                 WHERE (?1 IS NULL OR transaction_type = ?1)
                   AND (?2 IS NULL OR transaction_date >= ?2)
                   AND (?3 IS NULL OR transaction_date <= ?3)",
                params![
                    transaction_type.map(TransactionType::as_str),
                    from.map(|d| d.to_string()),
                    to.map(|d| d.to_string()),
                ],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Atomically creates a new holding and its founding acquisition
    /// transaction. This is the primary entry point for the `buy` command
    /// when recording a brand-new physical item.
    pub fn record_acquisition(
        &self,
        new_holding: &NewHolding,
        mut new_txn: NewTransaction,
    ) -> Result<(crate::models::Holding, Transaction)> {
        new_holding.validate()?;
        new_txn.transaction_type = TransactionType::Acquisition;

        let tx = self.conn.unchecked_transaction()?;

        let holding_id = insert_holding_row(&tx, new_holding)?;
        new_txn.holding_id = holding_id;
        new_txn.validate()?;
        let txn_id = insert_transaction_row(&tx, &new_txn)?;

        tx.commit()?;
        Ok((self.get_holding(holding_id)?, self.get_transaction(txn_id)?))
    }

    /// Atomically records a disposition transaction against an existing
    /// holding and flips its status to `sold`. This is the primary entry
    /// point for the `sell` command.
    pub fn record_sale(&self, mut new_txn: NewTransaction) -> Result<Transaction> {
        new_txn.transaction_type = TransactionType::Disposition;
        new_txn.validate()?;

        let tx = self.conn.unchecked_transaction()?;
        flip_owned_holding(
            &tx,
            new_txn.holding_id,
            HoldingStatus::Sold,
            new_txn.transaction_date,
        )?;
        let txn_id = insert_transaction_row(&tx, &new_txn)?;

        tx.commit()?;
        self.get_transaction(txn_id)
    }

    /// Atomically records a realized loss against a still-owned holding and
    /// flips its status to `Lost` or `Damaged`. Mirrors `record_sale`
    /// exactly — a loss is a disposition like any other, just with
    /// `residual_value` (salvage/market value retained, zero for a total
    /// loss) and `insurance_recovery` as its two proceeds components
    /// instead of a buyer's payment. This is what makes a lost/damaged
    /// holding's cost basis show up as a realized loss in every P&L
    /// rollup, rather than a bare status change that leaves it invisible.
    #[allow(clippy::too_many_arguments)]
    pub fn record_loss(
        &self,
        holding_id: i64,
        status: HoldingStatus,
        loss_date: NaiveDate,
        residual_value: Money,
        insurance_recovery: Money,
        loss_cause: Option<String>,
        notes: Option<String>,
    ) -> Result<Transaction> {
        if !matches!(status, HoldingStatus::Lost | HoldingStatus::Damaged) {
            return Err(CardRoiError::validation(
                "record_loss only accepts Lost or Damaged as the target status",
            ));
        }

        let new_txn = NewTransaction {
            holding_id,
            transaction_type: TransactionType::Disposition,
            transaction_date: loss_date,
            price: residual_value + insurance_recovery,
            residual_value: Some(residual_value),
            insurance_recovery: Some(insurance_recovery),
            loss_cause,
            notes,
            ..Default::default()
        };
        new_txn.validate()?;

        let tx = self.conn.unchecked_transaction()?;
        flip_owned_holding(&tx, holding_id, status, loss_date)?;
        let txn_id = insert_transaction_row(&tx, &new_txn)?;

        tx.commit()?;
        self.get_transaction(txn_id)
    }
}

fn row_to_transaction(row: &Row) -> rusqlite::Result<Transaction> {
    Ok(Transaction {
        id: row.get("id")?,
        holding_id: row.get("holding_id")?,
        transaction_type: parse_enum(row.get("transaction_type")?)?,
        transaction_date: parse_date(row.get("transaction_date")?)?,
        price: row.get("price_cents")?,
        fees: row.get("fees_cents")?,
        shipping: row.get("shipping_cents")?,
        tax: row.get("tax_cents")?,
        other_cost: row.get("other_cost_cents")?,
        total: row.get("total_cents")?,
        currency: row.get("currency")?,
        counterparty: row.get("counterparty")?,
        platform: row.get("platform")?,
        external_ref: row.get("external_ref")?,
        notes: row.get("notes")?,
        residual_value: row.get("residual_value_cents")?,
        insurance_recovery: row.get("insurance_recovery_cents")?,
        loss_cause: row.get("loss_cause")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
    })
}
