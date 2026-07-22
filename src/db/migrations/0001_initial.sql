-- CardROI initial schema
-- All monetary values are stored as integer minor units (cents) to guarantee
-- exact accounting arithmetic — never use REAL for money.
-- All timestamps are ISO-8601 UTC strings ("YYYY-MM-DDTHH:MM:SS.sssZ").

PRAGMA foreign_keys = ON;

-- A product/set, e.g. "2023 Topps Chrome".
CREATE TABLE sets (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    sport         TEXT NOT NULL DEFAULT 'Basketball',
    year          INTEGER,
    brand         TEXT,
    total_cards   INTEGER,
    notes         TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (name, sport, year)
);

-- A catalog entry for a card within a set (not a specific physical copy).
CREATE TABLE cards (
    id              INTEGER PRIMARY KEY,
    set_id          INTEGER NOT NULL REFERENCES sets(id) ON DELETE RESTRICT,
    card_number     TEXT NOT NULL,
    player_name     TEXT NOT NULL,
    variant         TEXT,           -- e.g. "Refractor"
    parallel_name   TEXT,           -- e.g. "Gold", "Superfractor"
    print_run       INTEGER,        -- e.g. 25 for a /25 parallel; NULL if unnumbered
    is_rookie       INTEGER NOT NULL DEFAULT 0 CHECK (is_rookie IN (0, 1)),
    is_autograph    INTEGER NOT NULL DEFAULT 0 CHECK (is_autograph IN (0, 1)),
    is_relic        INTEGER NOT NULL DEFAULT 0 CHECK (is_relic IN (0, 1)),
    notes           TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (set_id, card_number, variant, parallel_name)
);

-- A specific physical unit of a card that is (or was) owned.
-- Each row is one unique item — a serial-numbered or graded card cannot be
-- fungibly merged with another copy, so quantity is always 1.
CREATE TABLE holdings (
    id                INTEGER PRIMARY KEY,
    card_id           INTEGER NOT NULL REFERENCES cards(id) ON DELETE RESTRICT,
    serial_number     TEXT,     -- e.g. "12/25", specific to this physical copy
    grade             TEXT,     -- e.g. "10", "9.5"
    grading_company   TEXT,     -- e.g. "PSA", "BGS", "SGC"
    cert_number       TEXT,
    status            TEXT NOT NULL DEFAULT 'owned'
                          CHECK (status IN ('owned', 'sold', 'lost', 'damaged')),
    acquired_date     TEXT,
    disposed_date     TEXT,
    notes             TEXT,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- The financial ledger. Every cost and every proceed is its own row so the
-- full audit trail is reconstructable.
CREATE TABLE transactions (
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
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_cards_set_id ON cards(set_id);
CREATE INDEX idx_cards_player_name ON cards(player_name);
CREATE INDEX idx_holdings_card_id ON holdings(card_id);
CREATE INDEX idx_holdings_status ON holdings(status);
CREATE INDEX idx_transactions_holding_id ON transactions(holding_id);
CREATE INDEX idx_transactions_date ON transactions(transaction_date);
CREATE INDEX idx_transactions_type ON transactions(transaction_type);
