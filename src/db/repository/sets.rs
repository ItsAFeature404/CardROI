use rusqlite::{Row, params};

use crate::error::{CardRoiError, Result};
use crate::models::{NewSet, Set};

use super::{Repository, parse_timestamp};

impl Repository {
    pub fn create_set(&self, new_set: &NewSet) -> Result<Set> {
        new_set.validate()?;
        self.conn.execute(
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
        self.get_set(self.conn.last_insert_rowid())
    }

    pub fn get_set(&self, id: i64) -> Result<Set> {
        self.conn
            .query_row("SELECT * FROM sets WHERE id = ?1", params![id], row_to_set)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardRoiError::not_found("set", id),
                other => other.into(),
            })
    }

    pub fn list_sets(&self) -> Result<Vec<Set>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM sets ORDER BY year DESC, name ASC")?;
        let rows = stmt.query_map([], row_to_set)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_set(&self, id: i64) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM sets WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CardRoiError::not_found("set", id));
        }
        Ok(())
    }
}

fn row_to_set(row: &Row) -> rusqlite::Result<Set> {
    Ok(Set {
        id: row.get("id")?,
        name: row.get("name")?,
        sport: row.get("sport")?,
        year: row.get("year")?,
        brand: row.get("brand")?,
        total_cards: row.get("total_cards")?,
        notes: row.get("notes")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
        updated_at: parse_timestamp(row.get("updated_at")?)?,
    })
}
