//! `cardroi import` — bulk-load acquisitions from CSV or JSON.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Args;
use serde::Deserialize;

use cardroi::db::repository::{AcquisitionImportRow, ChecklistImportRow, Repository};
use cardroi::error::CardRoiError;
use cardroi::models::{Money, NewCard, NewHolding, NewSet, NewTransaction, TransactionType};

#[derive(Debug, Args)]
pub struct ImportArgs {
    #[arg(long)]
    file: PathBuf,
    /// csv | json; inferred from the file extension if omitted
    #[arg(long)]
    format: Option<String>,
    /// Only create set/card catalog entries; skip holdings/transactions.
    /// Useful for pre-loading a checklist before you own any of it - price
    /// columns are ignored (and may be absent) in this mode.
    #[arg(long)]
    checklist: bool,
}

/// One row of the import file. Every field beyond `set_name`/`card_number`/
/// `player_name`/`price` is optional so a minimal spreadsheet still works.
#[derive(Debug, Deserialize)]
struct ImportRow {
    set_name: String,
    #[serde(default = "default_sport")]
    sport: String,
    #[serde(default)]
    set_year: Option<i32>,
    #[serde(default)]
    set_brand: Option<String>,
    card_number: String,
    player_name: String,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    parallel_name: Option<String>,
    #[serde(default)]
    print_run: Option<i32>,
    #[serde(default)]
    is_rookie: Option<bool>,
    #[serde(default)]
    is_autograph: Option<bool>,
    #[serde(default)]
    is_relic: Option<bool>,
    #[serde(default)]
    serial_number: Option<String>,
    #[serde(default)]
    grade: Option<String>,
    #[serde(default)]
    grading_company: Option<String>,
    #[serde(default)]
    cert_number: Option<String>,
    #[serde(default)]
    acquired_date: Option<String>,
    /// Required for acquisition import; ignored (and may be absent) for
    /// `--checklist` import.
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    fees: Option<String>,
    #[serde(default)]
    shipping: Option<String>,
    #[serde(default)]
    tax: Option<String>,
    #[serde(default)]
    other_cost: Option<String>,
    #[serde(default)]
    counterparty: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    external_ref: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

fn default_sport() -> String {
    "Basketball".to_string()
}

pub fn run(repo: &Repository, args: ImportArgs) -> Result<()> {
    let format = resolve_format(&args)?;
    let raw = std::fs::read_to_string(&args.file)
        .with_context(|| format!("failed to read {}", args.file.display()))?;

    let rows: Vec<ImportRow> = match format.as_str() {
        "csv" => parse_csv(&raw)?,
        "json" => parse_json(&raw)?,
        other => bail!("unsupported import format: {other:?}, expected csv or json"),
    };

    let summary = if args.checklist {
        let converted = rows.iter().map(to_checklist_row).collect::<Vec<_>>();
        repo.import_checklist(&converted)
            .context("checklist import failed - no rows were committed")?
    } else {
        let converted = rows
            .iter()
            .enumerate()
            .map(|(idx, row)| to_acquisition_row(idx + 1, row))
            .collect::<Result<Vec<_>>>()?;
        repo.import_acquisitions(&converted)
            .context("import failed - no rows were committed")?
    };

    println!(
        "Imported {} row(s): {} new set(s), {} new card(s)",
        summary.rows_imported, summary.sets_created, summary.cards_created
    );

    Ok(())
}

fn resolve_format(args: &ImportArgs) -> Result<String> {
    if let Some(f) = &args.format {
        return Ok(f.clone());
    }
    match args.file.extension().and_then(|e| e.to_str()) {
        Some("csv") => Ok("csv".to_string()),
        Some("json") => Ok("json".to_string()),
        other => {
            bail!("cannot infer import format from extension {other:?}; pass --format csv|json")
        }
    }
}

fn parse_csv(raw: &str) -> Result<Vec<ImportRow>> {
    let mut reader = csv::Reader::from_reader(raw.as_bytes());
    let mut rows = Vec::new();
    for (idx, result) in reader.deserialize::<ImportRow>().enumerate() {
        let row = result.map_err(|e| import_err(idx + 1, format!("CSV parse error: {e}")))?;
        rows.push(row);
    }
    Ok(rows)
}

fn parse_json(raw: &str) -> Result<Vec<ImportRow>> {
    serde_json::from_str(raw).map_err(|e| import_err(0, format!("JSON parse error: {e}")))
}

fn to_acquisition_row(row_num: usize, row: &ImportRow) -> Result<AcquisitionImportRow> {
    let price_str = row
        .price
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            import_err(
                row_num,
                "price is required for acquisition import (use --checklist to import catalog entries only)",
            )
        })?;
    let price = parse_money(row_num, price_str, "price")?;
    let fees = parse_money_opt(row_num, row.fees.as_deref(), "fees")?;
    let shipping = parse_money_opt(row_num, row.shipping.as_deref(), "shipping")?;
    let tax = parse_money_opt(row_num, row.tax.as_deref(), "tax")?;
    let other_cost = parse_money_opt(row_num, row.other_cost.as_deref(), "other_cost")?;
    let acquired_date = row
        .acquired_date
        .as_deref()
        .map(|s| parse_date(row_num, s))
        .transpose()?;
    let transaction_date = acquired_date.unwrap_or_else(|| chrono::Utc::now().date_naive());

    Ok(AcquisitionImportRow {
        set: NewSet {
            name: row.set_name.clone(),
            sport: row.sport.clone(),
            year: row.set_year,
            brand: row.set_brand.clone(),
            total_cards: None,
            notes: None,
        },
        card: NewCard {
            set_id: 0, // resolved during import
            card_number: row.card_number.clone(),
            player_name: row.player_name.clone(),
            variant: row.variant.clone(),
            parallel_name: row.parallel_name.clone(),
            print_run: row.print_run,
            is_rookie: row.is_rookie.unwrap_or(false),
            is_autograph: row.is_autograph.unwrap_or(false),
            is_relic: row.is_relic.unwrap_or(false),
            notes: None,
        },
        holding: NewHolding {
            card_id: 0, // resolved during import
            serial_number: row.serial_number.clone(),
            grade: row.grade.clone(),
            grading_company: row.grading_company.clone(),
            cert_number: row.cert_number.clone(),
            acquired_date,
            notes: None,
        },
        transaction: NewTransaction {
            holding_id: 0, // resolved during import
            transaction_type: TransactionType::Acquisition,
            transaction_date,
            price,
            fees,
            shipping,
            tax,
            other_cost,
            currency: "USD".to_string(),
            counterparty: row.counterparty.clone(),
            platform: row.platform.clone(),
            external_ref: row.external_ref.clone(),
            notes: row.notes.clone(),
            ..Default::default()
        },
    })
}

fn to_checklist_row(row: &ImportRow) -> ChecklistImportRow {
    ChecklistImportRow {
        set: NewSet {
            name: row.set_name.clone(),
            sport: row.sport.clone(),
            year: row.set_year,
            brand: row.set_brand.clone(),
            total_cards: None,
            notes: None,
        },
        card: NewCard {
            set_id: 0, // resolved during import
            card_number: row.card_number.clone(),
            player_name: row.player_name.clone(),
            variant: row.variant.clone(),
            parallel_name: row.parallel_name.clone(),
            print_run: row.print_run,
            is_rookie: row.is_rookie.unwrap_or(false),
            is_autograph: row.is_autograph.unwrap_or(false),
            is_relic: row.is_relic.unwrap_or(false),
            notes: None,
        },
    }
}

fn import_err(row: usize, message: impl Into<String>) -> anyhow::Error {
    CardRoiError::Import {
        row,
        message: message.into(),
    }
    .into()
}

fn parse_money(row: usize, s: &str, field: &str) -> Result<Money> {
    Money::from_str(s).map_err(|e| import_err(row, format!("invalid {field} {s:?}: {e}")))
}

fn parse_money_opt(row: usize, s: Option<&str>, field: &str) -> Result<Money> {
    match s {
        Some(s) if !s.trim().is_empty() => parse_money(row, s, field),
        _ => Ok(Money::ZERO),
    }
}

fn parse_date(row: usize, s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s)
        .map_err(|_| import_err(row, format!("invalid date {s:?}, expected YYYY-MM-DD")))
}
