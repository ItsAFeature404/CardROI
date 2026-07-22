use rusqlite::{Row, params};

use crate::error::{CardRoiError, Result};
use crate::models::{Card, NewCard};

use super::{Repository, parse_timestamp};

impl Repository {
    pub fn create_card(&self, new_card: &NewCard) -> Result<Card> {
        new_card.validate()?;
        self.conn.execute(
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
        self.get_card(self.conn.last_insert_rowid())
    }

    pub fn get_card(&self, id: i64) -> Result<Card> {
        self.conn
            .query_row(
                "SELECT * FROM cards WHERE id = ?1",
                params![id],
                row_to_card,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardRoiError::not_found("card", id),
                other => other.into(),
            })
    }

    /// Lists cards, optionally filtered to a single set.
    pub fn list_cards(&self, set_id: Option<i64>) -> Result<Vec<Card>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM cards
             WHERE (?1 IS NULL OR set_id = ?1)
             ORDER BY set_id ASC, card_number ASC",
        )?;
        let rows = stmt.query_map(params![set_id], row_to_card)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Finds a card by its natural key within a set, used by the CSV/JSON
    /// importer to avoid creating duplicate catalog entries.
    pub fn find_card(
        &self,
        set_id: i64,
        card_number: &str,
        variant: Option<&str>,
        parallel_name: Option<&str>,
    ) -> Result<Option<Card>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM cards
             WHERE set_id = ?1 AND card_number = ?2
               AND variant IS ?3 AND parallel_name IS ?4",
        )?;
        let mut rows = stmt.query_map(
            params![set_id, card_number, variant, parallel_name],
            row_to_card,
        )?;
        rows.next().transpose().map_err(Into::into)
    }

    pub fn delete_card(&self, id: i64) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM cards WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CardRoiError::not_found("card", id));
        }
        Ok(())
    }
}

fn row_to_card(row: &Row) -> rusqlite::Result<Card> {
    Ok(Card {
        id: row.get("id")?,
        set_id: row.get("set_id")?,
        card_number: row.get("card_number")?,
        player_name: row.get("player_name")?,
        variant: row.get("variant")?,
        parallel_name: row.get("parallel_name")?,
        print_run: row.get("print_run")?,
        is_rookie: row.get("is_rookie")?,
        is_autograph: row.get("is_autograph")?,
        is_relic: row.get("is_relic")?,
        notes: row.get("notes")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
        updated_at: parse_timestamp(row.get("updated_at")?)?,
    })
}
