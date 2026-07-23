//! `cardroi card` — CRUD for card catalog entries within a set.

use anyhow::{Context, Result};
use clap::Subcommand;
use comfy_table::Table;

use cardroi::db::repository::Repository;
use cardroi::models::{Card, CardEdit, NewCard};

use super::holding::apply_edit;

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
    /// Correct a card's own catalog identity after the fact (not which
    /// set it belongs to - that's a rarer, bigger move than fixing a
    /// typo). Affects every holding that references this card, since
    /// they're all the same catalog print. Omitting a flag leaves that
    /// field unchanged; passing an empty string clears it (does not
    /// apply to --rookie/--autograph/--relic, which can only be turned
    /// on this way in v1, not back off).
    Edit {
        id: i64,
        #[arg(long)]
        number: Option<String>,
        #[arg(long)]
        player: Option<String>,
        #[arg(long)]
        variant: Option<String>,
        #[arg(long)]
        parallel: Option<String>,
        #[arg(long = "print-run")]
        print_run: Option<String>,
        #[arg(long)]
        rookie: bool,
        #[arg(long)]
        autograph: bool,
        #[arg(long)]
        relic: bool,
        #[arg(long)]
        notes: Option<String>,
    },
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
        CardCommand::Edit {
            id,
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
            let current = repo
                .get_card(id)
                .with_context(|| format!("failed to fetch card {id}"))?;
            let print_run = apply_edit_print_run(print_run, current.print_run)
                .context("invalid --print-run")?;
            let edit = CardEdit {
                card_number: apply_edit(number, Some(current.card_number)).unwrap_or_default(),
                player_name: apply_edit(player, Some(current.player_name)).unwrap_or_default(),
                variant: apply_edit(variant, current.variant),
                parallel_name: apply_edit(parallel, current.parallel_name),
                print_run,
                is_rookie: rookie || current.is_rookie,
                is_autograph: autograph || current.is_autograph,
                is_relic: relic || current.is_relic,
                notes: apply_edit(notes, current.notes),
            };
            let updated = repo
                .update_card(id, &edit)
                .with_context(|| format!("failed to update card {id}"))?;
            println!("Updated card #{id}");
            print_card(&updated);
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

/// The `print_run`-shaped counterpart to `apply_edit` - same
/// omit-means-unchanged/empty-means-clear semantics, but parses the
/// remaining case as an integer rather than passing a string through.
fn apply_edit_print_run(flag: Option<String>, existing: Option<i32>) -> Result<Option<i32>> {
    match flag {
        None => Ok(existing),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => s
            .trim()
            .parse::<i32>()
            .map(Some)
            .map_err(|_| anyhow::anyhow!("expected a whole number, got {s:?}")),
    }
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
