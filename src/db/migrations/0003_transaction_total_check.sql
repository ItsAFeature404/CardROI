-- Ties total_cents to its components at the database level, so a future
-- direct-SQL write or repository bug can never insert a ledger entry whose
-- stored total silently disagrees with price/fees/shipping/tax/other_cost.
-- Previously this was trusted entirely to NewTransaction::total() in the
-- app layer (src/models/transaction.rs) with no DB-level backstop.
--
-- SQLite has no ALTER TABLE ... ADD CONSTRAINT, so this rebuilds the table
-- in place (standard 12-step procedure). Nothing references transactions
-- as a foreign key target, so this is a safe in-place rebuild.

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
    )
);

INSERT INTO transactions_new
    SELECT id, holding_id, transaction_type, transaction_date,
           price_cents, fees_cents, shipping_cents, tax_cents, other_cost_cents,
           total_cents, currency, counterparty, platform, external_ref, notes, created_at
    FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;

CREATE INDEX idx_transactions_holding_id ON transactions(holding_id);
CREATE INDEX idx_transactions_date ON transactions(transaction_date);
CREATE INDEX idx_transactions_type ON transactions(transaction_type);

PRAGMA foreign_keys = ON;
