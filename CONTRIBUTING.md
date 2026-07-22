# Contributing to CardROI

CardROI is currently a solo-maintained project — there's no formal process,
no CLA, and no guaranteed review turnaround. That said, issues and pull
requests are welcome. Everyone participating is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting a bug or requesting a feature

Open a [GitHub issue](https://github.com/ItsAFeature404/CardROI/issues).
For a bug, include:
- what you ran (the exact `cardroi` command/flags, or which screen of the
  web app)
- what you expected vs. what happened
- your OS/browser and `cardroi --version` (or the commit hash if built
  from source)

For anything involving money math (P&L, ROI, IRR, TWR), a hand-computed
expected value is the most useful thing you can include — this project's
whole bar for a financial calculation is that it matches a value someone
worked out independently, not just "it ran without crashing."

## Sending a pull request

1. Fork the repo, branch off `main`.
2. Make sure the full local check suite passes before opening the PR:
   ```bash
   cargo test
   cargo clippy --all-targets --all-features -- -D warnings
   cargo fmt --check
   # if you touched crates/cardroi-web:
   cargo clippy -p cardroi-web --target wasm32-unknown-unknown --all-targets -- -D warnings
   ```
   CI runs the same checks (plus a pinned-MSRV build, the wasm32 build,
   and `cargo audit`) on every push and PR. Branch protection on `main`
   isn't currently turned on, so this isn't enforced automatically yet —
   merging is manual, so treat a red run as a hard blocker regardless.
3. If you're touching `src/analytics/` (or anything else that produces a
   number), add a test with a hand-computed or independently cross-checked
   expected value, not just a "doesn't panic" assertion — see the existing
   tests in that directory for the pattern.
4. Keep commits focused and write a commit message that explains *why*,
   not just what changed.

## Code style

- Rust 2024 edition, `anyhow` + `thiserror` for errors, no comments that
  just restate what the code already says — a comment should explain
  *why*, not what.
- Money is always exact integer cents via the `Money` type — never `f64`.
- Layout: `src/db/` (database layer), `src/models/` (domain entities),
  `src/commands/` (CLI subcommands, thin — delegate to repository +
  analytics), `src/analytics/` (ROI/IRR/TWR/portfolio calculations),
  `src/reports/` (export), `crates/cardroi-web/` (the Dioxus web/WASM
  app, a separate workspace crate depending on the root crate as a
  library).

## What this project is not looking for (yet)

Tax/insurance reporting and an HTML/PDF dashboard are both still
unstarted and not yet spec'd in detail — if you want to work on either,
open an issue first to check the current direction before sinking time
into a PR that might not match where it's headed.
