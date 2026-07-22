-- Adds composite indexes for the two hot query patterns that filter by
-- holding_id then order by a date column: latest_appraisal_for_holding
-- (appraisals.rs) and list_transactions_for_holding (transactions.rs).
-- Previously only single-column indexes existed on holding_id and the date
-- column separately, so SQLite could use one or the other but not both in
-- a single index scan. Self-acknowledged as a scale-only concern (not a
-- correctness bug) in analytics::roi's N+1 rollup doc comment - addressed
-- here since it's a cheap, safe additive change.

CREATE INDEX idx_appraisals_holding_id_date ON appraisals(holding_id, appraised_date);
CREATE INDEX idx_transactions_holding_id_date ON transactions(holding_id, transaction_date);
