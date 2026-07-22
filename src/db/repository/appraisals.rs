use rusqlite::{OptionalExtension, Row, params};

use crate::error::{CardRoiError, Result};
use crate::models::{Appraisal, NewAppraisal};

use super::{Repository, parse_date, parse_timestamp};

impl Repository {
    pub fn create_appraisal(&self, new_appraisal: &NewAppraisal) -> Result<Appraisal> {
        new_appraisal.validate()?;
        self.conn.execute(
            "INSERT INTO appraisals (
                holding_id, appraised_value_cents, appraised_date, source, notes
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                new_appraisal.holding_id,
                new_appraisal.appraised_value,
                new_appraisal.appraised_date.to_string(),
                new_appraisal.source,
                new_appraisal.notes,
            ],
        )?;
        self.get_appraisal(self.conn.last_insert_rowid())
    }

    pub fn get_appraisal(&self, id: i64) -> Result<Appraisal> {
        self.conn
            .query_row(
                "SELECT * FROM appraisals WHERE id = ?1",
                params![id],
                row_to_appraisal,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardRoiError::not_found("comp", id),
                other => other.into(),
            })
    }

    /// Lists all appraisals recorded for a holding, oldest first — the full
    /// valuation history, not just the latest.
    pub fn list_appraisals_for_holding(&self, holding_id: i64) -> Result<Vec<Appraisal>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM appraisals WHERE holding_id = ?1
             ORDER BY appraised_date ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![holding_id], row_to_appraisal)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The most recent appraisal for a holding, by appraised_date (ties
    /// broken by insertion order). This is the value analytics uses as an
    /// open position's terminal value — always labeled as a user-supplied
    /// appraisal, never as live market value.
    pub fn latest_appraisal_for_holding(&self, holding_id: i64) -> Result<Option<Appraisal>> {
        self.conn
            .query_row(
                "SELECT * FROM appraisals WHERE holding_id = ?1
                 ORDER BY appraised_date DESC, id DESC LIMIT 1",
                params![holding_id],
                row_to_appraisal,
            )
            .optional()
            .map_err(CardRoiError::from)
    }

    pub fn delete_appraisal(&self, id: i64) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM appraisals WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CardRoiError::not_found("comp", id));
        }
        Ok(())
    }
}

fn row_to_appraisal(row: &Row) -> rusqlite::Result<Appraisal> {
    Ok(Appraisal {
        id: row.get("id")?,
        holding_id: row.get("holding_id")?,
        appraised_value: row.get("appraised_value_cents")?,
        appraised_date: parse_date(row.get("appraised_date")?)?,
        source: row.get("source")?,
        notes: row.get("notes")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
    })
}
