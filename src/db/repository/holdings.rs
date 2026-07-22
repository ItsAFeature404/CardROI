use rusqlite::{Connection, Row, params};

use crate::error::{CardRoiError, Result};
use crate::models::{Holding, HoldingEdit, HoldingStatus, NewHolding};

use super::{Repository, parse_date, parse_enum, parse_timestamp};

/// Inserts a holding row and returns its new id. Takes `&Connection` (not
/// `&Repository`) so it's usable both standalone and nested inside an
/// active `Transaction` (which derefs to `Connection`) — shared by
/// `create_holding`, `record_acquisition`, and `import_acquisitions`, which
/// previously each hand-wrote a copy of this same INSERT.
pub(super) fn insert_holding_row(conn: &Connection, new_holding: &NewHolding) -> Result<i64> {
    conn.execute(
        "INSERT INTO holdings (
            card_id, serial_number, grade, grading_company, cert_number,
            acquired_date, notes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            new_holding.card_id,
            new_holding.serial_number,
            new_holding.grade,
            new_holding.grading_company,
            new_holding.cert_number,
            new_holding.acquired_date.map(|d| d.to_string()),
            new_holding.notes,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

impl Repository {
    pub fn create_holding(&self, new_holding: &NewHolding) -> Result<Holding> {
        new_holding.validate()?;
        let id = insert_holding_row(&self.conn, new_holding)?;
        self.get_holding(id)
    }

    pub fn get_holding(&self, id: i64) -> Result<Holding> {
        self.conn
            .query_row(
                "SELECT * FROM holdings WHERE id = ?1",
                params![id],
                row_to_holding,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardRoiError::not_found("holding", id),
                other => other.into(),
            })
    }

    /// Lists holdings, optionally filtered by card and/or status.
    pub fn list_holdings(
        &self,
        card_id: Option<i64>,
        status: Option<HoldingStatus>,
    ) -> Result<Vec<Holding>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM holdings
             WHERE (?1 IS NULL OR card_id = ?1)
               AND (?2 IS NULL OR status = ?2)
             ORDER BY acquired_date ASC, id ASC",
        )?;
        let rows = stmt.query_map(
            params![card_id, status.map(HoldingStatus::as_str)],
            row_to_holding,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One page of holdings for the portfolio table, joined against
    /// `cards`/`sets` so it can filter by the same set/player/sport
    /// dimensions `analytics::portfolio`'s attribution already groups by.
    /// At most one of `set_id`/`player_name`/`sport` should be `Some` at
    /// a time - the table shows one grouping dimension drilled into at
    /// once, not an intersection of all three. Ordered most-recent-first
    /// (`acquired_date DESC`) rather than `list_holdings`'s ascending
    /// default, since a portfolio table reads better with recent activity
    /// on top.
    #[allow(clippy::too_many_arguments)]
    pub fn list_holdings_page(
        &self,
        status: Option<HoldingStatus>,
        set_id: Option<i64>,
        player_name: Option<&str>,
        sport: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Holding>> {
        // SQLite treats a negative `LIMIT` as "unlimited," not an error -
        // silently returning every row instead of rejecting a bad page
        // request, so this needs an explicit guard rather than trusting
        // the caller to only ever pass a sane value.
        if limit < 0 || offset < 0 {
            return Err(CardRoiError::validation(format!(
                "limit and offset must be non-negative, got limit={limit}, offset={offset}"
            )));
        }
        let mut stmt = self.conn.prepare(
            "SELECT h.* FROM holdings h
             JOIN cards c ON h.card_id = c.id
             JOIN sets s ON c.set_id = s.id
             WHERE (?1 IS NULL OR h.status = ?1)
               AND (?2 IS NULL OR s.id = ?2)
               AND (?3 IS NULL OR c.player_name = ?3)
               AND (?4 IS NULL OR s.sport = ?4)
             ORDER BY h.acquired_date DESC, h.id DESC
             LIMIT ?5 OFFSET ?6",
        )?;
        let rows = stmt.query_map(
            params![
                status.map(HoldingStatus::as_str),
                set_id,
                player_name,
                sport,
                limit,
                offset
            ],
            row_to_holding,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Total row count for the same filter `list_holdings_page` uses -
    /// lets the table show "page X of Y" / total-holdings-count without
    /// fetching every row just to count them.
    pub fn count_holdings_page(
        &self,
        status: Option<HoldingStatus>,
        set_id: Option<i64>,
        player_name: Option<&str>,
        sport: Option<&str>,
    ) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM holdings h
                 JOIN cards c ON h.card_id = c.id
                 JOIN sets s ON c.set_id = s.id
                 WHERE (?1 IS NULL OR h.status = ?1)
                   AND (?2 IS NULL OR s.id = ?2)
                   AND (?3 IS NULL OR c.player_name = ?3)
                   AND (?4 IS NULL OR s.sport = ?4)",
                params![
                    status.map(HoldingStatus::as_str),
                    set_id,
                    player_name,
                    sport
                ],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn delete_holding(&self, id: i64) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM holdings WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CardRoiError::not_found("holding", id));
        }
        Ok(())
    }

    /// Corrects a holding's own physical/grading attributes after the
    /// fact - see `HoldingEdit`'s doc comment for exactly what's in and
    /// out of scope (not status, not which card, not acquired_date).
    pub fn update_holding(&self, id: i64, edit: &HoldingEdit) -> Result<Holding> {
        edit.validate()?;
        let affected = self.conn.execute(
            "UPDATE holdings SET
                serial_number = ?2, grade = ?3, grading_company = ?4,
                cert_number = ?5, notes = ?6,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                id,
                edit.serial_number,
                edit.grade,
                edit.grading_company,
                edit.cert_number,
                edit.notes,
            ],
        )?;
        if affected == 0 {
            return Err(CardRoiError::not_found("holding", id));
        }
        self.get_holding(id)
    }

    /// Deletes a holding along with every transaction recorded against it,
    /// the explicit, deliberate override of `transactions.holding_id`'s
    /// normal `ON DELETE RESTRICT`, which exists specifically so an
    /// accidental or careless `DELETE FROM holdings` can't silently
    /// orphan/lose real ledger history by surprise. This method is the
    /// one sanctioned place that bypasses it, and only for a holding the
    /// caller has already gotten explicit, unambiguous confirmation to
    /// permanently remove (a mistaken test entry, a duplicate); unlike
    /// everything else in this ledger, this is real, permanent data loss,
    /// not just a status change. Appraisals and `holding_images` rows
    /// already cascade automatically (`ON DELETE CASCADE`); only
    /// transactions need clearing first. Atomic: both deletes happen in
    /// one SQL transaction, so a failure never leaves a holding with its
    /// transactions half-gone.
    pub fn delete_holding_cascade(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM transactions WHERE holding_id = ?1",
            params![id],
        )?;
        let affected = tx.execute("DELETE FROM holdings WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CardRoiError::not_found("holding", id));
        }
        tx.commit()?;
        Ok(())
    }
}

fn row_to_holding(row: &Row) -> rusqlite::Result<Holding> {
    Ok(Holding {
        id: row.get("id")?,
        card_id: row.get("card_id")?,
        serial_number: row.get("serial_number")?,
        grade: row.get("grade")?,
        grading_company: row.get("grading_company")?,
        cert_number: row.get("cert_number")?,
        status: parse_enum(row.get("status")?)?,
        acquired_date: row
            .get::<_, Option<String>>("acquired_date")?
            .map(parse_date)
            .transpose()?,
        disposed_date: row
            .get::<_, Option<String>>("disposed_date")?
            .map(parse_date)
            .transpose()?,
        notes: row.get("notes")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
        updated_at: parse_timestamp(row.get("updated_at")?)?,
    })
}
