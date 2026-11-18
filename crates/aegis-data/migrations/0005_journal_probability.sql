-- v0.2: trade journal, versioned probability models, dataset registry.

-- One row per position opened by AEGIS: what was decided, what was paid, what came out.
CREATE TABLE trade_journal (
    ticket              INTEGER PRIMARY KEY,
    decision_id         TEXT NOT NULL DEFAULT '',
    cid                 TEXT NOT NULL DEFAULT '',
    strategy_id         TEXT NOT NULL DEFAULT '',
    symbol              TEXT NOT NULL,
    timeframe           TEXT NOT NULL DEFAULT '',
    side                TEXT NOT NULL,
    regime              TEXT NOT NULL DEFAULT '',
    terminal            TEXT NOT NULL DEFAULT '',
    opened_ms           INTEGER NOT NULL,
    closed_ms           INTEGER,
    volume              REAL NOT NULL,
    entry_requested     REAL NOT NULL DEFAULT 0,
    entry_filled        REAL NOT NULL DEFAULT 0,
    exit_price          REAL,
    sl_initial          REAL NOT NULL DEFAULT 0,
    tp_initial          REAL NOT NULL DEFAULT 0,
    probability         REAL,
    probability_source  TEXT NOT NULL DEFAULT '',
    expected_net        REAL,
    est_spread_cost     REAL NOT NULL DEFAULT 0,
    est_commission      REAL NOT NULL DEFAULT 0,
    slippage_cost       REAL NOT NULL DEFAULT 0,
    gross               REAL NOT NULL DEFAULT 0,
    commission          REAL NOT NULL DEFAULT 0,
    swap                REAL NOT NULL DEFAULT 0,
    fee                 REAL NOT NULL DEFAULT 0,
    net                 REAL NOT NULL DEFAULT 0,
    exit_reason         TEXT NOT NULL DEFAULT '',
    mfe_price           REAL NOT NULL DEFAULT 0,
    mae_price           REAL NOT NULL DEFAULT 0
);
CREATE INDEX idx_journal_closed ON trade_journal(closed_ms);
CREATE INDEX idx_journal_strategy ON trade_journal(strategy_id, closed_ms);

CREATE TABLE probability_models (
    model_id        TEXT PRIMARY KEY,
    created_ms      INTEGER NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    origin          TEXT NOT NULL,
    dataset_hash    TEXT NOT NULL,
    buy_selected    TEXT NOT NULL,
    sell_selected   TEXT NOT NULL,
    active          INTEGER NOT NULL DEFAULT 0,
    model           TEXT NOT NULL
);
CREATE INDEX idx_probability_models_key ON probability_models(symbol, timeframe, created_ms);

-- Every dataset a model was trained on, identified by the hash of its candles.
CREATE TABLE datasets (
    dataset_hash    TEXT PRIMARY KEY,
    created_ms      INTEGER NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    origin          TEXT NOT NULL,
    bars            INTEGER NOT NULL,
    from_ts         INTEGER NOT NULL,
    to_ts           INTEGER NOT NULL
);
