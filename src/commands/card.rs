//! `cardroi card` — CRUD for card catalog entries within a set.

use anyhow::{Context, Result};
use clap::Subcommand;
use comfy_table::Table;

use cardroi::db::repository::Repository;
use cardroi::models::{Card, NewCard};

#[derive(Debug, Subcommand)]
pub enum CardCommand {
    /// Create a new card catalog entry
    Add {
        #[arg(long = "set-id")]
        set_id: i64,
        #[arg(long)]
        number: String,
        #[arg(long)]
        player: String,
        #[arg(long)]
        variant: Option<String>,
        #[arg(long)]
        parallel: Option<String>,
        #[arg(long = "print-run")]
        print_run: Option<i32>,
        #[arg(long)]
        rookie: bool,
        #[arg(long)]
        autograph: bool,
        #[arg(long)]
        relic: bool,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List cards, optionally filtered to one set
    List {
        #[arg(long = "set-id")]
        set_id: Option<i64>,
    },
    /// Show a single card
    Show { id: i64 },
    /// Delete a card (fails if any holdings still reference it)
    Delete { id: i64 },
}

pub fn run(repo: &Repository, cmd: CardCommand) -> Result<()> {
    match cmd {
        CardCommand::Add {
            set_id,
            number,
            player,
            variant,
            parallel,
            print_run,
            rookie,
            autograph,
            relic,
            notes,
        } => {
            let card = repo
                .create_card(&NewCard {
                    set_id,
                    card_number: number,
                    player_name: player,
                    variant,
                    parallel_name: parallel,
                    print_run,
                    is_rookie: rookie,
                    is_autograph: autograph,
                    is_relic: relic,
                    notes,
                })
                .context("failed to create card")?;
            print_card(&card);
        }
        CardCommand::List { set_id } => {
            let cards = repo.list_cards(set_id).context("failed to list cards")?;
            print_table(&cards);
        }
        CardCommand::Show { id } => {
            let card = repo
                .get_card(id)
                .with_context(|| format!("failed to fetch card {id}"))?;
            print_card(&card);
        }
        CardCommand::Delete { id } => {
            repo.delete_card(id).with_context(|| {
                format!("failed to delete card {id} — it still has holdings referencing it")
            })?;
            println!("Deleted card #{id}");
        }
    }
    Ok(())
}

fn print_card(card: &Card) {
    println!("Card #{}: {}", card.id, card.display_name());
    println!("  Set: {}", card.set_id);
    if card.is_rookie {
        println!("  Rookie: yes");
    }
    if card.is_autograph {
        println!("  Autograph: yes");
    }
    if card.is_relic {
        println!("  Relic: yes");
    }
    if let Some(notes) = &card.notes {
        println!("  Notes: {notes}");
    }
}

fn print_table(cards: &[Card]) {
    let mut table = Table::new();
    table.set_header(vec!["ID", "Set", "Card"]);
    for card in cards {
        table.add_row(vec![
            card.id.to_string(),
            card.set_id.to_string(),
            card.display_name(),
        ]);
    }
    println!("{table}");
}
