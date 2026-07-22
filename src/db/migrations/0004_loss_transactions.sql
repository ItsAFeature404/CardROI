-- Supports recording a real realized loss when a holding is marked
-- Lost/Damaged (see SPEC.md "Lost/Damaged Holding Loss Treatment"). Before
-- this, marking a holding lost/damaged only flipped its status column with
-- no transaction, so its cost basis silently vanished from every P&L
-- rollup. These three columns are populated only on the Disposition
-- transaction a loss event creates; every other transaction leaves them
-- NULL.
--
-- residual_value_cents and insurance_recovery_cents are kept as separate
-- amounts (not merged into one figure) because they are legally distinct
-- for tax purposes (IRS Pub 547: salvage/residual value retained vs.
-- insurance reimbursement received are both subtracted from adjusted basis,
-- but from different sources) and because insurers track them separately
-- (partial-damage repair/residual value vs. a claim payout).
--
-- SQLite has no ALTER TABLE ... ADD CONSTRAINT, so this rebuilds the table
-- in place, same as migration 0003. Nothing references transactions as a
-- foreign key target.

PRAGMA foreign_keys = OFF;

CREATE TABLE transactions_new (
    id                  INTEGER PRIMARY KEY,
    holding_id          INTEGER NOT NULL REFERENCES holdings(id) ON DELETE RESTRICT,
    transaction_type    TEXT NOT NULL
                            CHECK (transaction_type IN ('acquisition', 'disposition', 'adjustment')),
    transaction_date    TEXT NOT NULL,
    price_cents         INTEGER NOT NULL DEFAULT 0,
    fees_cents          INTEGER NOT NULL DEFAULT 0,
    shipping_cents      INTEGER NOT NULL DEFAULT 0,
    tax_cents           INTEGER NOT NULL DEFAULT 0,
    other_cost_cents    INTEGER NOT NULL DEFAULT 0,
    -- Acquisitions/adjustments: total cost added to basis (positive).
    -- Dispositions: net proceeds received (positive).
    total_cents         INTEGER NOT NULL,
    currency            TEXT NOT NULL DEFAULT 'USD',
    counterparty        TEXT,     -- who you bought from / sold to
    platform            TEXT,     -- eBay, COMC, in-person, etc.
    external_ref         TEXT,     -- order id / invoice number
    notes               TEXT,
    -- Loss-event-only fields (NULL on every other transaction). See
    -- SPEC.md "Lost/Damaged Holding Loss Treatment" for the research
    -- grounding on why these are separate amounts, not one merged figure.
    residual_value_cents      INTEGER,  -- salvage/market value retained (Damaged); 0/NULL for a total loss
    insurance_recovery_cents  INTEGER,  -- reimbursement received, distinct from residual_value
    loss_cause                TEXT,     -- e.g. "water damage", "theft" - informational, not used in any calculation
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (
            transaction_type IN ('acquisition', 'adjustment')
            AND total_cents = price_cents + fees_cents + shipping_cents + tax_cents + other_cost_cents
        )
        OR
        (
            transaction_type = 'disposition'
            AND total_cents = price_cents - fees_cents - shipping_cents - tax_cents - other_cost_cents
        )
    ),
    CHECK (
        residual_value_cents IS NULL
        OR insurance_recovery_cents IS NULL
        OR price_cents = residual_value_cents + insurance_recovery_cents
    )
);

INSERT INTO transactions_new
    SELECT id, holding_id, transaction_type, transaction_date,
           price_cents, fees_cents, shipping_cents, tax_cents, other_cost_cents,
           total_cents, currency, counterparty, platform, external_ref, notes,
           NULL, NULL, NULL,
           created_at
    FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;

CREATE INDEX idx_transactions_holding_id ON transactions(holding_id);
CREATE INDEX idx_transactions_date ON transactions(transaction_date);
CREATE INDEX idx_transactions_type ON transactions(transaction_type);

PRAGMA foreign_keys = ON;
