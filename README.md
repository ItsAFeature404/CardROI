# CardROI

[![CI](https://github.com/ItsAFeature404/CardROI/actions/workflows/ci.yml/badge.svg)](https://github.com/ItsAFeature404/CardROI/actions/workflows/ci.yml)

A local-first investment portfolio manager for trading card collectors and
investors, usable from a phone or computer browser with no install, plus a
CLI for scripting. Every acquisition cost, sale proceed, fee, shipping
expense, tax, and comp (a comparable sold listing's price) gets tracked
down to the cent, so ROI, IRR, TWR, and P&L come out exact — not
eyeballed off a spreadsheet.

**Not a scanner or market-price app.** CardROI never fetches, scrapes, or
estimates a card's market value, and never phones home. Every number in a
report is either what you actually paid or received (a real transaction),
or a value you typed in yourself after pricing a card against comps
(comparable sold listings) — timestamped, sourced, and audit-tracked like
everything else. A comp is always labeled as a user-supplied value as of
a specific date, never presented as a live price.

**Local-first, genuinely.** The web app runs the real database and
analytics engine client-side, in your own browser — your collection lives
in that browser's own storage, not on a server anywhere. The CLI stores
everything in one portable SQLite file you control directly. Either way,
your data never leaves your device unless you explicitly export it.

## Table of contents

- [Features](#features)
- [Precision, trust & the audit trail](#precision-trust--the-audit-trail)
- [The web app](#the-web-app)
- [Install / Build the CLI](#install--build-the-cli)
- [Configuration](#configuration)
- [Quick start](#quick-start)
- [Command reference](#command-reference)
- [Development](#development)
- [Contributing](#contributing)
- [Security](#security)
- [Status](#status)
- [License](#license)

## Features

**Catalog & ledger**
- `Set → Card → Holding → Transaction` data model: cards are catalog
  entries, holdings are specific physical copies (serialized/graded cards
  are never fungible — five copies of the same card are five separate
  holdings), transactions are the ledger (acquisitions, dispositions,
  cost-basis adjustments)
- Full CRUD on sets, cards, and holdings, with referential-integrity
  guards (e.g. a card can't be deleted while holdings still reference it)
- `buy`/`sell` with price, fees, shipping, tax, and other costs tracked
  separately; `--quantity N` fans one `buy` out into N independently
  sellable holdings
- Correct a data-entry mistake after the fact: `holding edit`/
  `transaction edit` fix a typo (serial, grade, price, date, ...) without
  disturbing anything else; `holding delete --with-transactions`
  permanently removes a mistaken or test holding and its whole history —
  a deliberate, explicit override of the normal safeguard that otherwise
  keeps ledger history from ever being silently lost
- Grading support (grade, grading company, cert number), serial numbers,
  print runs, parallels, variants, rookie/autograph/relic flags
- Loss/damage tracking (`mark-lost`/`mark-damaged`) records a real
  realized loss — optional residual/salvage value and insurance recovery,
  tracked separately — not just a status flip; guarded so a sold holding's
  status and realized P&L can never be silently overwritten
- Bulk import from CSV or JSON, with a `--checklist` mode for
  catalog-only pre-loading before you own anything
- Every mutation lives in the transaction ledger — a holding's full
  history is reconstructable from it alone

**Comps**
- User-supplied values, timestamped and optionally sourced/noted — the
  hobby's actual pricing method (comparable sold listings), not a formal
  third-party appraisal
- Full comp history per holding, or just the latest

**Analytics**
- Realized P&L at holding, card, set, or portfolio scope
- Unrealized P&L for currently-owned holdings with a comp on record —
  always shown next to the comp date it came from
- IRR (XIRR, Newton-Raphson with a bisection fallback for cash-flow
  patterns that don't converge) for closed positions using the real sale
  price, or open positions using the latest comp as terminal value — the
  standard GIPS/private-equity convention for illiquid assets. A short
  holding period can legitimately annualize to an enormous-looking rate
  (real math, not a bug) — flagged with a plain-language note past 500%
  rather than left to look broken.
- Time-weighted return, chaining sub-period returns between consecutive
  comps, with optional annualization — shown side by side with IRR
  and a note on when/why the two diverge
- Portfolio analytics: allocation by card and by set (currently-owned
  holdings, weighted by comp value where available else cost basis),
  HHI concentration risk with effective-position count, and P&L
  attribution by player and by sport (all-time)

**What-if scenario modeling**
- Simulates a hypothetical sale (a fixed price, or the latest comp)
  against a currently-owned holding's real cost basis — read-only, never
  writes anything, and only ever operates on holdings you still own
- Every assumption used (price, source, date, fees/shipping/tax/other
  cost) is always printed; `--format json` output uses field names
  distinct from `roi`'s real P&L so it can never be mistaken for a real
  number, whether read by a human or a script

**Reporting**
- `table`, `csv`, and `json` output; write to a file or stdout

**Web app**
- Runs entirely in your browser ([Dioxus](https://dioxuslabs.com/), web/
  WASM target) — the exact same `Repository`/`analytics` engine the CLI
  uses, genuinely client-side via real SQLite compiled to WebAssembly,
  persisted to your browser's own storage. No server, no account, no
  cloud — see [The web app](#the-web-app) below for exactly how that
  works and what it means for your data.
- Responsive from the ground up: a sidebar on desktop, a bottom nav plus
  a floating quick-action button on phone — not a desktop layout
  squeezed down.
- Dashboard, a paginated/groupable portfolio table (by set, player, or
  sport), per-holding drill-down (full transaction and comp history,
  What-If sale simulation, Mark Lost/Damaged, inline edit, and a
  danger-zone delete), Buy/Sell/Comp entry forms, an advanced performance
  view (IRR/TWR behind a "Show advanced" toggle) and a risk/allocation
  view (diversification score, concentration bar, allocation donut).
- See [Status](#status) for exactly what's built versus still in
  progress, and [The web app](#the-web-app) for how to run it yourself.

**Precision & platform**
- Money is exact integer cents end to end — never a float — with
  overflow checks on; the only narrow exception is the IRR/TWR rate
  itself, which is inherently the root of an equation found by numerical
  approximation (documented at the conversion boundary in the code)
- The CLI stores everything in one portable `.db` file; the web app
  stores everything in your browser's own local storage. Neither needs
  an account or a cloud service to function.
- CLI is cross-platform (Linux, macOS, Windows) — CI-verified on all
  three for every commit, not just assumed

## Precision, trust & the audit trail

Four things this project will never compromise on:

- **Money is never a float.** Every amount is parsed to exact integer
  cents and rejects anything with more than 2 decimal places rather than
  silently rounding. Accepts a plain number (`500.00`), an optional
  leading `$`, and optional standard thousands grouping (`1,234.56`); a
  comma is **never** interpreted as a decimal point (`10,00` is rejected
  as ambiguous, not silently read as `1000.00`, since that's how many
  locales write ten). Dates are always `YYYY-MM-DD`.
- **Every mutation lives in the transaction ledger.** A holding's full
  cost basis, disposition, and loss history is reconstructable from its
  transactions alone — nothing about a holding's financial history is
  ever stored only as a derived, overwritable field. The one deliberate
  exception is `holding delete --with-transactions`, an explicit,
  irreversible action you have to opt into by name — it exists to undo a
  genuine mistake, not to quietly erase history by accident.
- **Nothing is ever estimated as if it were real.** A comp is
  always labeled as a user-supplied value as of a specific date. A
  what-if result is always prefixed `HYPOTHETICAL` and uses field names
  distinct from real P&L, in both table and JSON output, so a script
  can't accidentally treat one as the other.
- **JSON percentage fields are raw ratios, not pre-scaled percentages.**
  `roi_pct`, `hypothetical_roi_pct`, and similar `*_pct` fields in
  `--format json` output serialize as e.g. `"0.50"` for 50%, matching
  what the table view shows as `50.00%`. JSON is for further computation,
  not display — if you're scripting against it, expect ratios.

**This is not financial, tax, or legal advice**, and CardROI does not
provide any — it's a ledger and calculator for numbers you supply
yourself. See the [MIT license](LICENSE) for the full "as-is, no
warranty" terms.

## The web app

`crates/cardroi-web` is the primary way most people should use CardROI —
open it in a phone or desktop browser, no install. It depends on the root
`cardroi` crate as a library, so there's no second data or validation
layer to keep in sync with the CLI: the same `Repository`/`analytics`
code runs in both places.

**Where your data actually lives:** the app installs a real SQLite
database, compiled to WebAssembly, running inside your browser tab. Its
storage is backed by your browser's IndexedDB — genuinely local to that
one browser on that one device. Nothing is sent to a server, because
there is no server; the app's code itself has to be *fetched* from
somewhere (see the deploy note below), but once loaded, everything it
does with your data happens entirely on your device. Confirmed to survive
a tab reload, a full browser close/reopen, and — on a phone with the page
added to the home screen — closing and relaunching it like a native app.

Two practical consequences worth knowing:
- Your phone and your computer, or two different browsers, each hold
  their own separate copy of your data — there's no sync between them
  (not yet built; see [Status](#status)).
- Clearing that browser's site data for this page deletes your CardROI
  data completely and permanently, the same as deleting a file — there's
  no server copy to recover it from.

**Running it yourself:**

```bash
cd crates/cardroi-web
dx serve --platform web --addr 0.0.0.0
```

`--addr 0.0.0.0` serves on your LAN, not just `localhost` — useful for
testing on a real phone over the same WiFi network. Requires the
[Dioxus CLI](https://dioxuslabs.com/) (`cargo install dioxus-cli`) and a
wasm-capable `clang` (needed to compile SQLite's C source to WebAssembly;
already present on most Linux/macOS toolchains).

Tailwind is **not** compiled automatically by `dx` — after changing any
utility class in a `.rs` file, recompile by hand from `crates/cardroi-web/`:

```bash
./node_modules/.bin/tailwindcss -i assets/tailwind.css -o assets/tailwind.generated.css
```

## Install / Build the CLI

Requires Rust 1.88+ (2024 edition). On Windows, a C toolchain (MSVC Build
Tools or MinGW-w64) is needed to compile the bundled SQLite.

```bash
git clone https://github.com/ItsAFeature404/CardROI.git
cd CardROI
cargo build --release
# binary at target/release/cardroi
```

To run `cardroi` from anywhere as a normal command (not just inside this
repo), install it to `~/.cargo/bin` (already on `PATH` if you have Rust set
up via rustup):

```bash
cargo install --path .
```

This installs a snapshot — it does **not** auto-update as the code changes.
Re-run `cargo install --path .` whenever you want your global `cardroi` to
pick up new commits.

## Configuration

CardROI's CLI stores everything in one SQLite file, `cardroi.db` by
default in the current directory. Override the path with the global
`--db <path>` flag or the `CARDROI_DB` environment variable (`--db` wins
if both are set). There is no other configuration. The web app has no
equivalent file path at all — see [The web app](#the-web-app) for where
its data actually lives.

## Quick start

```bash
# 1. Catalog: define the set and the card
cardroi set add --name "2023 Topps Chrome" --sport Basketball --year 2023
cardroi card add --set-id 1 --number 123 --player "LeBron James" \
  --variant Refractor --parallel Gold --print-run 25

# 2. Buy it - creates holding #1 and its founding transaction
cardroi buy --card-id 1 --price 500.00 --fees 25.00 --serial "12/25" --date 2026-01-01

# 3. (Optional) record what you believe it's worth, whenever you like -
#    IRR/TWR need this to be a different date than the purchase
cardroi comp add --holding-id 1 --value 900.00 --date 2026-06-01 \
  --source "PSA pop report comp"

# 4. See where you stand
cardroi roi --holding-id 1
cardroi irr --holding-id 1

# 5. Eventually, sell it
cardroi sell --holding-id 1 --price 800.00 --fees 40.00 --date 2026-09-01
cardroi roi --holding-id 1     # now shows realized P&L instead
```

Run `cardroi <command> --help` or `cardroi <command> <subcommand> --help`
for the full, always-current flag reference — everything below is a
summary, not a substitute for it.

## Command reference

### Catalog: sets and cards

```bash
cardroi set add --name "2023 Topps Chrome" --sport Basketball --year 2023 \
  --brand Topps --total-cards 220 --notes "..."
cardroi set list
cardroi set show <id>
cardroi set delete <id>        # fails if any cards still reference it

cardroi card add --set-id 1 --number 123 --player "LeBron James" \
  --variant Refractor --parallel Gold --print-run 25 \
  --rookie --autograph --relic --notes "..."
cardroi card list [--set-id <id>]
cardroi card show <id>
cardroi card delete <id>       # fails if any holdings still reference it
```

A set is unique on `(name, sport, year)`; a card is unique within a set on
`(number, variant, parallel)`.

### Holdings, buy, sell, edit, delete

```bash
# buy/sell cover the common path; holding add/delete are for direct control
cardroi buy --card-id 1 --price 500.00 --fees 25.00 --shipping 0.00 \
  --tax 0.00 --other-cost 0.00 --serial "12/25" --grade 10 \
  --grading-company PSA --cert 123456 --date 2026-01-01 \
  --counterparty "John Doe" --platform eBay --notes "..."
cardroi buy --card-id 1 --price 500.00 --quantity 5   # 5 independent holdings

cardroi sell --holding-id 1 --price 800.00 --fees 40.00 --date 2026-06-01

cardroi holding add --card-id 1 --serial "12/25" --grade 10 --grading-company PSA
cardroi holding list [--card-id <id>] [--status owned|sold|lost|damaged]
cardroi holding show <id>
cardroi holding edit <id> [--serial "..."] [--grade "..."] \
  [--grading-company "..."] [--cert "..."] [--notes "..."]
cardroi holding delete <id>                     # fails if any transactions reference it
cardroi holding delete <id> --with-transactions # also deletes its transactions - permanent
cardroi holding mark-lost <id> [--date YYYY-MM-DD] [--residual-value 0.00] \
  [--insurance-recovery 0.00] [--cause "stolen"] [--notes "..."]
cardroi holding mark-damaged <id> [--date YYYY-MM-DD] [--residual-value 100.00] \
  [--insurance-recovery 50.00] [--cause "water damage"] [--notes "..."]

cardroi transaction show <id>
cardroi transaction edit <id> [--date YYYY-MM-DD] [--price 0.00] [--fees 0.00] \
  [--shipping 0.00] [--tax 0.00] [--other-cost 0.00] [--counterparty "..."] \
  [--platform "..."] [--external-ref "..."] [--notes "..."]
```

A grade requires a `--grading-company`. `--quantity N > 1` is incompatible
with `--serial`/`--cert`, which identify one physical item. `mark-lost`/
`mark-damaged` only work on a currently-owned holding — a sold holding's
status can't be overwritten. Selling an already-sold holding is rejected.

`mark-lost`/`mark-damaged` record a real realized loss (a `Disposition`
transaction), not just a status change. `--residual-value` is any
salvage/market value the card retains (defaults to 0.00, a total loss);
`--insurance-recovery` is reimbursement received, tracked separately since
the two are legally distinct for tax purposes (both are subtracted from
cost basis, but from different sources — mirroring how the IRS and
collectibles insurers actually treat a damaged/lost item). Neither is ever
estimated for you — damage discount varies enormously by rarity and damage
type, so you provide the number.

`holding edit`/`transaction edit` only change fields you actually pass a
flag for — omit a flag to leave that field as-is, or pass an empty string
to clear an optional text field. Neither can change a holding's card,
status, or a transaction's type — those stay governed by `sell`/
`mark-lost`/`mark-damaged`, so an edit can never desync a status from its
real disposition. `holding delete --with-transactions` is the one
sanctioned way to fully remove a holding that already has transactions on
it (every holding does, the moment it's bought) — a real, permanent loss
of history, meant for a genuine mistake or test entry, not routine use.

### Comps

```bash
cardroi comp add --holding-id 1 --value 900.00 --date 2026-06-01 \
  --source "PSA pop report comp" --notes "..."
cardroi comp list --holding-id 1     # full history, oldest first
cardroi comp latest --holding-id 1
cardroi comp delete <id>
```

Comps (comparable sold listings) are the hobby's actual pricing method —
not a formal, third-party appraisal. Multiple comps per holding are
expected — log a new one whenever you re-price it. Everything downstream
(`roi`, `irr`, `twr`, `report`) uses the most recent one by date and
always labels it as user-supplied.

### Analytics

```bash
cardroi roi                            # whole portfolio
cardroi roi --card-id 1
cardroi roi --set-id 1
cardroi roi --holding-id 1
cardroi roi --format json

cardroi irr --holding-id 1             # closed (sold/lost/damaged): real disposition; open: latest comp
cardroi irr                            # whole portfolio, closed positions only

cardroi twr --holding-id 1             # needs >= 2 comps on the holding
cardroi twr --holding-id 1 --annualize 1.5
cardroi twr                            # portfolio, currently-owned holdings
```

`roi`/`report` never claim an unrealized gain/loss for a holding with no
comp on record — cost basis only, exactly as if no comps existed.
`--format json`'s `roi_pct` is a raw ratio (`"0.50"`), not a pre-scaled
percentage — see [Precision, trust & the audit trail](#precision-trust--the-audit-trail).

### What-if

```bash
cardroi whatif --holding-id 1 --price 800.00 --date 2026-06-01
cardroi whatif --holding-id 1 --price 800.00 --fees 40.00 --shipping 10.00
cardroi whatif --holding-id 1 --at-comp    # price = latest comp
cardroi whatif --holding-id 1 --price 800.00 --format json
```

Exactly one of `--price`/`--at-comp` is required. Only valid on a
currently-owned holding (an already-sold one has a real answer via `roi`).
Nothing is ever written — every assumption used (price, its source, date,
and all four cost fields, even when left at their `0.00` default) is
printed alongside the result, and the output is prefixed `HYPOTHETICAL`
so it can never be confused with a real number. As with `roi`, JSON's
`hypothetical_roi_pct` is a raw ratio, not a percentage.

### Bulk import

```bash
cardroi import --file collection.csv
cardroi import --file collection.json
cardroi import --file checklist.csv --checklist   # catalog only, no cost
```

Format is inferred from the file extension, or set explicitly with
`--format csv|json`. Every row: finds-or-creates the set and card (deduped
by their natural keys above) and — unless `--checklist` — always creates a
new holding and its founding transaction, even on a re-import (importing
the same file twice means you bought it twice). The whole import is one
atomic operation: any invalid row rolls back everything, not just that
row.

Columns (CSV header row or JSON object keys — see
[`tests/fixtures/import_sample.csv`](tests/fixtures/import_sample.csv) and
[`import_sample.json`](tests/fixtures/import_sample.json) for worked
examples):

| Column | Required | Notes |
| --- | --- | --- |
| `set_name` | yes | |
| `sport` | no | defaults to `Basketball` |
| `set_year`, `set_brand` | no | |
| `card_number` | yes | |
| `player_name` | yes | |
| `variant`, `parallel_name`, `print_run` | no | |
| `is_rookie`, `is_autograph`, `is_relic` | no | booleans |
| `serial_number`, `grade`, `grading_company`, `cert_number` | no | |
| `acquired_date` | no | `YYYY-MM-DD`, defaults to today |
| `price` | **yes, unless `--checklist`** | ignored/absent in checklist mode |
| `fees`, `shipping`, `tax`, `other_cost` | no | default `0.00` |
| `counterparty`, `platform`, `external_ref`, `notes` | no | |

### Reports

```bash
cardroi report                              # table, stdout
cardroi report --format csv --output out.csv
cardroi report --format json
```

Table/JSON include the portfolio summary, per-card breakdown, allocation
by card/set, HHI concentration, and attribution by player/sport. CSV is
the per-card breakdown only (the summary sections don't fit a flat
per-row schema).

## Development

```bash
cargo build                                          # debug build
cargo test                                           # full test suite
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo audit                                          # dependency advisories

# the web app (a separate build target, see "The web app" above)
cargo clippy -p cardroi-web --target wasm32-unknown-unknown --all-targets -- -D warnings
```

This project is built following a spec/plan/TDD discipline: every task is
specced, planned, and test-first before implementation, with every
financial calculation backed by a hand-computed or independently
cross-checked reference value — never just "it doesn't panic." CI runs
the CLI's full suite plus `clippy`/`fmt` checks on Linux, macOS, and
Windows, and a `wasm32-unknown-unknown` build+clippy check for the web
app, on every push.

## Contributing

Bug reports, feature requests, and pull requests are welcome — see
[`CONTRIBUTING.md`](CONTRIBUTING.md) for how to file one and what the
acceptance bar looks like (short version: the full check suite above
needs to pass, and anything touching the financial math needs a
hand-computed test value).

## Security

CardROI's CLI has no network code at all. The web app fetches its own
code from wherever it's hosted (like any website) but makes no other
network calls — your data itself never crosses the network. See
[`SECURITY.md`](SECURITY.md) for the realistic attack surface and how to
report a vulnerability privately.

## Status

The engine and CLI are complete: catalog CRUD, buy/sell, edit/delete,
CSV/JSON import, realized/unrealized P&L, comps, XIRR, time-weighted
return, portfolio analytics, loss/damage tracking, and what-if scenario
modeling are all built and tested.

The web app is the primary, actively-developed interface. Built and
cross-checked against the CLI's own output: real SQLite persistence in
the browser (confirmed surviving a tab reload, full browser close/
reopen, and a home-screen-installed close/reopen on a real phone), a
responsive nav shell, Dashboard, Portfolio (grouping + pagination,
verified against a 10,000+ holding synthetic database), per-holding
drill-down (transaction/comp history, What-If, Mark Lost/Damaged, inline
edit, and delete), Buy/Sell/Comp forms, an advanced performance view
(IRR/TWR), and a risk/allocation view (diversification score, by-set/
player/sport allocation).

Not yet built: a Ledger screen and a Settings screen (both still
placeholders), an in-browser import/export UI, a public deployment (the
app currently has to be run locally via `dx serve` — see
[The web app](#the-web-app)), and a Playwright end-to-end test suite.
Tax/insurance reporting and an HTML/PDF dashboard are planned after that
but not started.

## License

MIT — see [`LICENSE`](LICENSE).
