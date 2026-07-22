//! `cardroi set` — CRUD for card sets.

use anyhow::{Context, Result};
use clap::Subcommand;
use comfy_table::Table;

use cardroi::db::repository::Repository;
use cardroi::models::{NewSet, Set};

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// Create a new set
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "Basketball")]
        sport: String,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long)]
        brand: Option<String>,
        #[arg(long = "total-cards")]
        total_cards: Option<i32>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all sets
    List,
    /// Show a single set
    Show { id: i64 },
    /// Delete a set (fails if any cards still reference it)
    Delete { id: i64 },
}

pub fn run(repo: &Repository, cmd: SetCommand) -> Result<()> {
    match cmd {
        SetCommand::Add {
            name,
            sport,
            year,
            brand,
            total_cards,
            notes,
        } => {
            let set = repo
                .create_set(&NewSet {
                    name,
                    sport,
                    year,
                    brand,
                    total_cards,
                    notes,
                })
                .context("failed to create set")?;
            print_set(&set);
        }
        SetCommand::List => {
            let sets = repo.list_sets().context("failed to list sets")?;
            print_table(&sets);
        }
        SetCommand::Show { id } => {
            let set = repo
                .get_set(id)
                .with_context(|| format!("failed to fetch set {id}"))?;
            print_set(&set);
        }
        SetCommand::Delete { id } => {
            repo.delete_set(id).with_context(|| {
                format!("failed to delete set {id} — it still has cards referencing it")
            })?;
            println!("Deleted set #{id}");
        }
    }
    Ok(())
}

fn print_set(set: &Set) {
    println!("Set #{}: {}", set.id, set.name);
    println!("  Sport: {}", set.sport);
    if let Some(year) = set.year {
        println!("  Year: {year}");
    }
    if let Some(brand) = &set.brand {
        println!("  Brand: {brand}");
    }
    if let Some(total) = set.total_cards {
        println!("  Total cards: {total}");
    }
    if let Some(notes) = &set.notes {
        println!("  Notes: {notes}");
    }
}

fn print_table(sets: &[Set]) {
    let mut table = Table::new();
    table.set_header(vec!["ID", "Name", "Sport", "Year", "Brand", "Total Cards"]);
    for set in sets {
        table.add_row(vec![
            set.id.to_string(),
            set.name.clone(),
            set.sport.clone(),
            set.year.map(|y| y.to_string()).unwrap_or_default(),
            set.brand.clone().unwrap_or_default(),
            set.total_cards.map(|t| t.to_string()).unwrap_or_default(),
        ]);
    }
    println!("{table}");
}
