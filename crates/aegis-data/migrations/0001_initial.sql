-- AEGIS initial schema. SQLite dialect, written to stay portable to PostgreSQL
-- (no SQLite-only types; INTEGER timestamps in ms; JSON stored as TEXT).

CREATE TABLE symbols (
    symbol      TEXT PRIMARY KEY,
    spec_json   TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL
);

CREATE TABLE candles (
    symbol        TEXT NOT NULL,
    timeframe     TEXT NOT NULL,
    open_ts       INTEGER NOT NULL,
    open          REAL NOT NULL,
    high          REAL NOT NULL,
    low           REAL NOT NULL,
    close         REAL NOT NULL,
    tick_volume   INTEGER NOT NULL,
    spread_points REAL NOT NULL,
    origin        TEXT NOT NULL CHECK (origin IN ('broker', 'synthetic')),
    PRIMARY KEY (symbol, timeframe, open_ts, origin),
    CHECK (high >= low AND low > 0)
);

CREATE TABLE account_snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms       INTEGER NOT NULL,
    login       INTEGER NOT NULL,
    balance     REAL NOT NULL,
    equity      REAL NOT NULL,
    margin      REAL NOT NULL,
    margin_free REAL NOT NULL,
    snapshot_json TEXT NOT NULL
);
CREATE INDEX idx_account_snapshots_ts ON account_snapshots (ts_ms);

CREATE TABLE strategies (
    strategy_id TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    stage       TEXT NOT NULL CHECK (stage IN ('research', 'demo', 'live_approved', 'disabled')),
    enabled     INTEGER NOT NULL DEFAULT 1,
    config_json TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL
);

CREATE TABLE decisions (
    decision_id  TEXT PRIMARY KEY,
    ts_ms        INTEGER NOT NULL,
    symbol       TEXT NOT NULL,
    strategy_id  TEXT NOT NULL REFERENCES strategies (strategy_id),
    side         TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    mode         TEXT NOT NULL,
    signal_json  TEXT NOT NULL,
    outcome      TEXT NOT NULL CHECK (outcome IN ('pending', 'approved', 'rejected', 'observed')),
    verdict_json TEXT
);
CREATE INDEX idx_decisions_ts ON decisions (ts_ms);

CREATE TABLE orders (
    cid          TEXT PRIMARY KEY,
    decision_id  TEXT NOT NULL REFERENCES decisions (decision_id),
    auth_id      TEXT NOT NULL UNIQUE,
    symbol       TEXT NOT NULL,
    side         TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    kind         TEXT NOT NULL CHECK (kind IN ('market', 'limit', 'stop')),
    volume       REAL NOT NULL CHECK (volume > 0),
    price        REAL NOT NULL,
    sl           REAL NOT NULL,
    tp           REAL NOT NULL,
    status       TEXT NOT NULL,
    ticket       INTEGER,
    fill_price   REAL,
    fill_volume  REAL,
    retcode      INTEGER,
    message      TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE INDEX idx_orders_status ON orders (status);

CREATE TABLE order_events (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    cid     TEXT NOT NULL REFERENCES orders (cid),
    ts_ms   INTEGER NOT NULL,
    status  TEXT NOT NULL,
    detail  TEXT NOT NULL
);
CREATE INDEX idx_order_events_cid ON order_events (cid);

CREATE TABLE positions (
    ticket      INTEGER PRIMARY KEY,
    symbol      TEXT NOT NULL,
    side        TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    volume      REAL NOT NULL,
    price_open  REAL NOT NULL,
    sl          REAL NOT NULL,
    tp          REAL NOT NULL,
    open_ts     INTEGER NOT NULL,
    is_open     INTEGER NOT NULL,
    position_json TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL
);

CREATE TABLE deals (
    deal_ticket     INTEGER PRIMARY KEY,
    position_ticket INTEGER NOT NULL,
    symbol          TEXT NOT NULL,
    side            TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    volume          REAL NOT NULL,
    price           REAL NOT NULL,
    profit          REAL NOT NULL,
    swap            REAL NOT NULL,
    commission      REAL NOT NULL,
    ts_ms           INTEGER NOT NULL,
    comment         TEXT NOT NULL
);
CREATE INDEX idx_deals_ts ON deals (ts_ms);

CREATE TABLE risk_configs (
    version     INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms       INTEGER NOT NULL,
    actor       TEXT NOT NULL,
    config_json TEXT NOT NULL
);

CREATE TABLE model_versions (
    model_id     TEXT PRIMARY KEY,
    created_ms   INTEGER NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('candidate', 'rejected', 'validated', 'retired')),
    origin       TEXT NOT NULL CHECK (origin IN ('broker', 'synthetic')),
    path         TEXT NOT NULL,
    metrics_json TEXT NOT NULL
);

CREATE TABLE backtest_runs (
    run_id       TEXT PRIMARY KEY,
    ts_ms        INTEGER NOT NULL,
    strategy_id  TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    timeframe    TEXT NOT NULL,
    origin       TEXT NOT NULL CHECK (origin IN ('broker', 'synthetic')),
    params_json  TEXT NOT NULL,
    metrics_json TEXT NOT NULL
);

CREATE TABLE backtest_trades (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id    TEXT NOT NULL REFERENCES backtest_runs (run_id) ON DELETE CASCADE,
    segment   TEXT NOT NULL,
    trade_json TEXT NOT NULL
);
CREATE INDEX idx_backtest_trades_run ON backtest_trades (run_id);

-- Append-only, hash-chained audit trail.
CREATE TABLE audit_log (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms     INTEGER NOT NULL,
    actor     TEXT NOT NULL,
    event     TEXT NOT NULL,
    detail    TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash      TEXT NOT NULL
);

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

-- Small durable state (kill switch, peak equity, day accounting...).
CREATE TABLE kv_state (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
