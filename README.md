# CardROI

[![CI](https://github.com/ItsAFeature404/CardROI/actions/workflows/ci.yml/badge.svg)](https://github.com/ItsAFeature404/CardROI/actions/workflows/ci.yml)

**🚧 Actively in development.** The core ledger and analytics are solid
and tested, but the app is missing pieces a first-time visitor would
expect — no Settings screen, no Ledger screen, no camera scan-to-identify
yet (you can attach a photo to a holding, but nothing reads a card from
it). See [Status & Roadmap](#status--roadmap) before you rely on it for
anything real.

**[Try it live →](https://itsafeature404.github.io/CardROI/)**

A local-first investment portfolio manager for trading card collectors —
open it in a phone or computer browser, no install, no account. Every
acquisition cost, sale proceed, fee, shipping expense, tax, and comp (a
comparable sold listing's price) gets tracked down to the cent, so ROI,
IRR, TWR, and P&L come out exact — not eyeballed off a spreadsheet.

**Not a scanner or market-price app.** CardROI never fetches, scrapes, or
estimates a card's market value, and never phones home. Every number in a
report is either what you actually paid or received (a real transaction),
or a value you typed in yourself after pricing a card against comps
(comparable sold listings) — timestamped, sourced, and audit-tracked like
everything else. A comp is always labeled as a user-supplied value as of
a specific date, never presented as a live price.

**Local-first, genuinely.** The app runs the real database and analytics
engine client-side, in your own browser — your collection lives in that
browser's own storage, not on a server anywhere. Your data never leaves
your device unless you explicitly export it.

## Table of contents

- [Features](#features)
- [The app](#the-app)
- [Precision, trust & the audit trail](#precision-trust--the-audit-trail)
- [The CLI](#the-cli)
- [Development](#development)
- [Contributing](#contributing)
- [Security](#security)
- [Status & Roadmap](#status--roadmap)
- [License](#license)

## Features

**The app**
- Runs entirely in your browser ([Dioxus](https://dioxuslabs.com/), web/
  WASM target) via real SQLite compiled to WebAssembly, persisted to
  your browser's own storage — no server, no account, no cloud. See
  [The app](#the-app) below for exactly how that works and what it means
  for your data.
- Responsive from the ground up: a sidebar on desktop, a bottom nav plus
  a floating quick-action button on phone — not a desktop layout
  squeezed down. The sidebar carries the app's own identity (and the
  browser tab gets a real title) — a small thing, but a first-time
  visitor should never wonder what app they're even in.
- A one-time "what should I call you" prompt (answer or skip — it never
  asks twice either way), so the Dashboard's greeting is personal
  ("Good morning, Alex") rather than generic. Stored in the browser's own
  local storage, not the collection database — clearing it doesn't touch
  a single card.
- Dashboard: search your whole collection, jump straight into Buy/Sell/
  Comp, see what's changed since you were last here (a new addition, a
  genuine gain, a comp gone stale), a quick read on whether your
  collection's pricing is current, and a running list of suggested next
  actions — oriented around what you'd actually do in a given visit, not
  a static portfolio-value readout.
- A paginated/groupable portfolio table (by set, player, or sport),
  per-holding drill-down (a merged ownership timeline interleaving
  transactions and comps chronologically, how long you've owned it in
  plain language, your own notes on the holding surfaced right alongside
  the numbers, a photo of the physical card you can add/replace/remove
  any time, What-If sale simulation paired with a direct Sell action,
  Mark Lost/Damaged, inline edit of both the holding and its underlying
  card catalog entry, and a danger-zone delete), Buy/Sell/Comp entry
  forms, an advanced performance view (IRR/TWR behind a "Show advanced"
  toggle) and a risk/allocation view (diversification score,
  concentration bar, allocation donut).
- See [Status & Roadmap](#status--roadmap) for exactly what's built
  versus still in progress.

**Catalog & ledger**
- `Set → Card → Holding → Transaction` data model: cards are catalog
  entries, holdings are specific physical copies (serialized/graded cards
  are never fungible — five copies of the same card are five separate
  holdings), transactions are the ledger (acquisitions, dispositions,
  cost-basis adjustments)
- Full CRUD on sets, cards, and holdings, with referential-integrity
  guards (e.g. a card can't be deleted while holdings still reference it)
- Buy/sell with price, fees, shipping, tax, and other costs tracked
  separately
- Correct a data-entry mistake after the fact: edit a holding, a
  transaction (serial, grade, price, date, ...), or a card's own catalog
  identity (player, number, variant, parallel, print run, rookie/
  autograph/relic) without disturbing anything else. A card correction
  intentionally ripples to every holding that references it — they're
  the same physical print, not independent facts — while a holding's own
  edits (its grading, serial, notes) never touch any other holding.
  Deleting a holding with transactions on it requires an explicit,
  separate confirmation — a deliberate override of the normal safeguard
  that otherwise keeps ledger history from ever being silently lost
- Grading support (grade, grading company, cert number), serial numbers,
  print runs, parallels, variants, rookie/autograph/relic flags
- Attach, replace, or remove a photo of the physical card on a holding —
  a phone's camera or a desktop file picker, one reference photo at a
  time. This is not a scan-to-identify feature: nothing reads a card's
  catalog details off the photo, it's purely a picture you attach
  yourself
- Loss/damage tracking (Mark Lost/Damaged) records a real realized loss —
  optional residual/salvage value and insurance recovery, tracked
  separately — not just a status flip; guarded so a sold holding's status
  and realized P&L can never be silently overwritten
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
  cost) is always shown, never left implicit

**Precision & platform**
- Money is exact integer cents end to end — never a float — with
  overflow checks on; the only narrow exception is the IRR/TWR rate
  itself, which is inherently the root of an equation found by numerical
  approximation (documented at the conversion boundary in the code)
- Everything lives in your browser's own local storage. No account, no
  cloud service, required to function.

**Also included: a CLI**, for scripting/bulk automation/testing — see
[The CLI](#the-cli). Most collectors should just use the app above.

## The app

`crates/cardroi-web` is CardROI — open it in a phone or desktop browser,
no install. It depends on the root `cardroi` crate as a library for its
data/analytics logic, so there's no second, parallel validation layer to
keep in sync.

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
  (not yet built; see [Status & Roadmap](#status--roadmap)).
- Clearing that browser's site data for this page deletes your CardROI
  data completely and permanently, the same as deleting a file — there's
  no server copy to recover it from.

**Live at [itsafeature404.github.io/CardROI](https://itsafeature404.github.io/CardROI/)**,
deployed straight from `main` via GitHub Actions/Pages — no separate
build step to trust, what's in the repo is what's running.

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

## Precision, trust & the audit trail

Four things this project will never compromise on:

- **Money is never a float.** Every amount is parsed to exact integer
  cents and rejects anything with more than 2 decimal places rather than
  silently rounding. Accepts a plain number (`500.00`), an optional
  leading `$`, and optional standard thousands grouping (`1,234.56`); a
  comma is **never** interpreted as a decimal point (`10,00` is rejected
  as ambiguous, not silently read as `1000.00`, since that's how many
  locales write ten).
- **Every mutation lives in the transaction ledger.** A holding's full
  cost basis, disposition, and loss history is reconstructable from its
  transactions alone — nothing about a holding's financial history is
  ever stored only as a derived, overwritable field. The one deliberate
  exception is a full holding delete including its transactions, an
  explicit, irreversible action you have to opt into by name — it exists
  to undo a genuine mistake, not to quietly erase history by accident.
- **Nothing is ever estimated as if it were real.** A comp is
  always labeled as a user-supplied value as of a specific date. A
  what-if result is always clearly marked hypothetical and never
  presented alongside real P&L in a way that could be confused for it.
- **JSON percentage fields (CLI output) are raw ratios, not pre-scaled
  percentages.** `roi_pct` and similar fields serialize as e.g. `"0.50"`
  for 50%. JSON is for further computation, not display.

**This is not financial, tax, or legal advice**, and CardROI does not
provide any — it's a ledger and calculator for numbers you supply
yourself. See the [MIT license](LICENSE) for the full "as-is, no
warranty" terms.

## The CLI

A command-line interface over the exact same engine the web app uses —
useful for scripting, bulk automation, or testing, but not what most
collectors should reach for day to day. Everything in
[Features](#features) above is available from it.

### Install / build

Requires Rust 1.95+ (2024 edition). On Windows, a C toolchain (MSVC Build
Tools or MinGW-w64) is needed to compile the bundled SQLite.

```bash
git clone https://github.com/ItsAFeature404/CardROI.git
cd CardROI
cargo build --release
# binary at target/release/cardroi
cargo install --path .   # to run `cardroi` from anywhere (a snapshot -
                          # re-run after pulling new commits to update it)
```

### Configuration

Stores everything in one SQLite file, `cardroi.db` by default in the
current directory. Override with the global `--db <path>` flag or the
`CARDROI_DB` environment variable (`--db` wins if both are set).

### Quick start

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

### Command reference

<details>
<summary>Catalog: sets and cards</summary>

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
cardroi card edit <id> [--number "..."] [--player "..."] [--variant "..."] \
  [--parallel "..."] [--print-run "..."] [--rookie] [--autograph] \
  [--relic] [--notes "..."]
cardroi card delete <id>       # fails if any holdings still reference it
```

A set is unique on `(name, sport, year)`; a card is unique within a set on
`(number, variant, parallel)`. `card edit` only changes fields you pass a
flag for (omit to leave as-is, empty string to clear an optional one) and
affects every holding referencing that card — they're the same catalog
print. `--rookie`/`--autograph`/`--relic` can only be turned on this way,
not back off, in the current version.
</details>

<details>
<summary>Holdings, buy, sell, edit, delete</summary>

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
the two are legally distinct for tax purposes. Neither is ever estimated
for you — damage discount varies enormously by rarity and damage type, so
you provide the number.

`holding edit`/`transaction edit` only change fields you actually pass a
flag for — omit a flag to leave that field as-is, or pass an empty string
to clear an optional text field. Neither can change a holding's card,
status, or a transaction's type — those stay governed by `sell`/
`mark-lost`/`mark-damaged`. `holding delete --with-transactions` is the
one sanctioned way to fully remove a holding that already has
transactions on it (every holding does, the moment it's bought) — a real,
permanent loss of history, meant for a genuine mistake or test entry, not
routine use.
</details>

<details>
<summary>Comps</summary>

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
</details>

<details>
<summary>Analytics</summary>

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
percentage.
</details>

<details>
<summary>What-if</summary>

```bash
cardroi whatif --holding-id 1 --price 800.00 --date 2026-06-01
cardroi whatif --holding-id 1 --price 800.00 --fees 40.00 --shipping 10.00
cardroi whatif --holding-id 1 --at-comp    # price = latest comp
cardroi whatif --holding-id 1 --price 800.00 --format json
```

Exactly one of `--price`/`--at-comp` is required. Only valid on a
currently-owned holding (an already-sold one has a real answer via `roi`).
Nothing is ever written — every assumption used is printed alongside the
result, and the output is prefixed `HYPOTHETICAL`.
</details>

<details>
<summary>Bulk import</summary>

```bash
cardroi import --file collection.csv
cardroi import --file collection.json
cardroi import --file checklist.csv --checklist   # catalog only, no cost
```

Format is inferred from the file extension, or set explicitly with
`--format csv|json`. Every row: finds-or-creates the set and card (deduped
by their natural keys above) and — unless `--checklist` — always creates a
new holding and its founding transaction, even on a re-import. The whole
import is one atomic operation: any invalid row rolls back everything.

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
</details>

<details>
<summary>Reports</summary>

```bash
cardroi report                              # table, stdout
cardroi report --format csv --output out.csv
cardroi report --format json
```

Table/JSON include the portfolio summary, per-card breakdown, allocation
by card/set, HHI concentration, and attribution by player/sport. CSV is
the per-card breakdown only.
</details>

## Development

```bash
cargo build                                          # debug build
cargo test                                           # full test suite (root crate/CLI only - see below)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo audit                                          # dependency advisories

# the web app (a separate build target, see "The app" above) - a plain
# `cargo test` from the repo root never touches this crate at all
cargo clippy -p cardroi-web --target wasm32-unknown-unknown --all-targets -- -D warnings
cargo test -p cardroi-web --target wasm32-unknown-unknown
```

The web app's tests need a real wasm-aware runner, not a bare `cargo
test` — a plain wasm32 binary can't execute at all on its own (the linked
wasm-bindgen glue needs a JS host). They run via `wasm-bindgen-test-runner`
under Node (configured in `.cargo/config.toml`); install a matching
`wasm-bindgen-cli` with
`cargo install wasm-bindgen-cli --version <version from Cargo.lock> --locked`
if you don't already have Node + a version-matched CLI.

This project is built following a spec/plan/TDD discipline: every task is
specced, planned, and test-first before implementation, with every
financial calculation backed by a hand-computed or independently
cross-checked reference value — never just "it doesn't panic." CI runs
the full test suite plus `clippy`/`fmt` checks on Linux, macOS, and
Windows, and a `wasm32-unknown-unknown` build, clippy, **and full test**
run for the app, on every push.

## Contributing

Bug reports, feature requests, and pull requests are welcome — see
[`CONTRIBUTING.md`](CONTRIBUTING.md) for how to file one and what the
acceptance bar looks like (short version: the full check suite above
needs to pass, and anything touching the financial math needs a
hand-computed test value).

## Security

The app fetches its own code from wherever it's hosted (like any
website) but makes no other network calls — your data itself never
crosses the network. The CLI has no network code at all. See
[`SECURITY.md`](SECURITY.md) for the realistic attack surface and how to
report a vulnerability privately.

## Status & Roadmap

**This project is under active development.** The ledger and analytics
engine are complete and well-tested, but the app is deliberately shipped
early and openly, with real gaps — don't rely on it for anything you
can't afford to re-enter by hand yet, and expect the below to change
often.

**Works today**, cross-checked against the CLI's own output: real SQLite
persistence in the browser (confirmed surviving a tab reload, full
browser close/reopen, and a home-screen-installed close/reopen on a
real phone), a responsive nav shell carrying the app's own identity, a
one-time personalized-greeting prompt, a Dashboard organized around
search/capture, what's changed since your last visit, a pricing-coverage
status check, and suggested next actions, Portfolio (grouping +
pagination, verified against a 10,000+ holding synthetic database),
per-holding drill-down (a merged ownership timeline of transactions and
comps, ownership duration in plain language, your own notes surfaced
alongside the numbers, a photo of the physical card you can add/replace/
remove, What-If paired with Sell, Mark Lost/Damaged, inline edit of both
the holding and its card, and delete), Buy/Sell/Comp forms, an advanced
performance view (IRR/TWR), and a risk/allocation view (diversification
score, by-set/player/sport allocation).

**In the nav, but not built yet** — clicking these gets you an honest
"not built yet" placeholder, not a broken screen:
- **Settings** — nothing configurable yet.
- **Ledger** — the full mutation history exists in the database (every
  screen's numbers already come from it) but there's no dedicated view
  of it yet.

**Not in the app at all yet** (no menu entry, no placeholder):
- **Scan-to-identify a card.** A holding can have a reference photo
  attached (phone camera or file picker) — but nothing reads a card's
  catalog details off it. Auto-identifying a card from a photo (what
  competitors' "Scan Card" features do) is a separate, unbuilt
  capability, and a materially harder problem (parallels and older cards
  are notoriously easy to misidentify even for purpose-built scanners) —
  not just a smaller version of photo capture.
- **In-browser import/export.** CSV/JSON import exists and is tested,
  but only via the CLI (`cardroi import`) — no UI for it yet.
- **Cross-device sync.** See [The app](#the-app) above — each browser's
  data is currently its own island.
- **Duplicate detection.** Deliberately not attempted yet: a naive
  same-card-twice check would misfire on completely normal collecting
  (owning two raw copies of the same card on purpose isn't a duplicate
  risk) — needs its own design pass, not a quick heuristic.
- A Playwright end-to-end test suite, and beyond that, tax/insurance
  reporting and an HTML/PDF dashboard — both still unspecced.

The engine and CLI underneath all of this are complete: catalog CRUD,
buy/sell, edit/delete, CSV/JSON import, realized/unrealized P&L, comps,
XIRR, time-weighted return, portfolio analytics, loss/damage tracking,
and what-if scenario modeling are all built and tested — the roadmap
above is entirely about surfacing more of that engine in the app, not
building new financial logic.

Longer-term direction and day-to-day progress live in
[GitHub Issues](https://github.com/ItsAFeature404/CardROI/issues) rather
than a separate roadmap document — a `planned` label marks accepted,
not-yet-started work; open ones under active work get assigned.

## License

MIT — see [`LICENSE`](LICENSE).
