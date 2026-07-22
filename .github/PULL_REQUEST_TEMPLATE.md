## What does this change?

<!-- What does this PR do, and why? -->

## Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean
- [ ] `cargo fmt --check` is clean
- [ ] If this touches `crates/cardroi-web`: `cargo clippy -p cardroi-web --target wasm32-unknown-unknown --all-targets -- -D warnings` is clean
- [ ] If this changes financial math (`src/analytics/`): a test with a hand-computed or independently cross-checked expected value is included, not just "it doesn't panic"
