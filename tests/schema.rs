//! Integration tests for the migration pipeline itself
//! (`db::schema::migrate`/`PRAGMA user_version`), not any one entity's
//! repository behavior - those live in their own `tests/*.rs` files.
//! A real in-memory database, no mocks, matching this project's testing
//! habit.

use cardroi::db::open_in_memory;
use cardroi::db::repository::Repository;
use cardroi::models::{NewCard, NewHolding, NewSet};

#[test]
fn a_fresh_database_lands_on_the_latest_migration_version() {
    let conn = open_in_memory().expect("in-memory db should open and migrate");
    let user_version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        user_version, 7,
        "bump this alongside adding a new migration - it's a deliberate tripwire"
    );
}

/// A real, valid `holding_id` to insert `holding_images` rows against -
/// so the FK constraint (`foreign_keys = ON` for every connection) never
/// masks the CHECK constraint these tests actually exercise.
fn seed_holding(repo: &Repository) -> i64 {
    let set = repo
        .create_set(&NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap();
    let card = repo
        .create_card(&NewCard {
            set_id: set.id,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.create_holding(&NewHolding {
        card_id: card.id,
        ..Default::default()
    })
    .unwrap()
    .id
}

#[test]
fn holding_images_rejects_a_row_with_neither_file_path_nor_full_data() {
    let repo = Repository::new(open_in_memory().unwrap());
    let holding_id = seed_holding(&repo);

    let err = repo
        .connection()
        .execute(
            "INSERT INTO holding_images
                (holding_id, file_path, full_data, file_hash, mime_type,
                 width, height, file_size_bytes, thumbnail_data)
             VALUES (?1, NULL, NULL, 'hash', 'image/jpeg', 1, 1, 1, x'00')",
            [holding_id],
        )
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("constraint"));
}

#[test]
fn holding_images_rejects_a_row_with_both_file_path_and_full_data() {
    let repo = Repository::new(open_in_memory().unwrap());
    let holding_id = seed_holding(&repo);

    let err = repo
        .connection()
        .execute(
            "INSERT INTO holding_images
                (holding_id, file_path, full_data, file_hash, mime_type,
                 width, height, file_size_bytes, thumbnail_data)
             VALUES (?1, 'ab/cd/abcd.jpg', x'00', 'hash', 'image/jpeg', 1, 1, 1, x'00')",
            [holding_id],
        )
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("constraint"));
}
