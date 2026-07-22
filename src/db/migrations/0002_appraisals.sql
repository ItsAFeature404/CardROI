-- Manual, timestamped valuations the user types in themselves. Never
-- derived from a market-price feed — see SPEC.md's non-goal section.
-- These unlock unrealized P&L and open-position IRR/TWR without CardROI
-- ever touching a comp/pricing source.
CREATE TABLE appraisals (
    id                      INTEGER PRIMARY KEY,
    holding_id              INTEGER NOT NULL REFERENCES holdings(id) ON DELETE CASCADE,
    appraised_value_cents   INTEGER NOT NULL,
    appraised_date          TEXT NOT NULL,
    source                  TEXT,     -- e.g. "PSA pop report comp", "insurance rider renewal"
    notes                   TEXT,
    created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_appraisals_holding_id ON appraisals(holding_id);
CREATE INDEX idx_appraisals_date ON appraisals(appraised_date);
