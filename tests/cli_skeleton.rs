//! CLI skeleton. Tests the global `--db`/`CARDROI_DB` resolution and DB
//! auto-create-and-migrate behavior via the real `cardroi` binary
//! (assert_cmd), not by calling internal functions directly — this is what
//! actually proves the wiring works end-to-end.

use assert_cmd::Command;
use rusqlite::Connection;
use tempfile::tempdir;

fn cardroi() -> Command {
    Command::cargo_bin("cardroi").expect("cardroi binary should build")
}

#[test]
fn help_runs_and_documents_the_db_flag() {
    cardroi()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--db"));
}

#[test]
fn missing_db_path_is_created_and_migrated_on_first_run() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");
    assert!(!db_path.exists());

    cardroi().arg("--db").arg(&db_path).assert().success();

    assert!(db_path.exists(), "db file should be created");
    let conn = Connection::open(&db_path).unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 7, "all migrations should have run");
}

#[test]
fn db_flag_takes_precedence_over_env_var() {
    let dir = tempdir().unwrap();
    let flag_path = dir.path().join("flag.db");
    let env_path = dir.path().join("env.db");

    cardroi()
        .env("CARDROI_DB", &env_path)
        .arg("--db")
        .arg(&flag_path)
        .assert()
        .success();

    assert!(flag_path.exists(), "the --db flag path should be used");
    assert!(
        !env_path.exists(),
        "the env var path should be ignored when --db is set"
    );
}

#[test]
fn env_var_is_used_when_flag_is_absent() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join("env-only.db");

    cardroi().env("CARDROI_DB", &env_path).assert().success();

    assert!(env_path.exists());
}
