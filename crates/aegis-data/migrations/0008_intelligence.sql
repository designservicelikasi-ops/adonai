-- v0.3: market intelligence and recovery. Detailed regime of each trade, kind of entry
-- (normal / reentry / hedge) with the position it follows or covers, role of the initial
-- stop; history of regime changes per instrument and timeframe.
-- Rollback: migrations_down/0008_intelligence.down.sql
ALTER TABLE trade_journal ADD COLUMN regime_detail TEXT NOT NULL DEFAULT '';
ALTER TABLE trade_journal ADD COLUMN entry_kind TEXT NOT NULL DEFAULT 'normal';
ALTER TABLE trade_journal ADD COLUMN related_ticket INTEGER;
ALTER TABLE trade_journal ADD COLUMN stop_role TEXT NOT NULL DEFAULT '';

CREATE TABLE regime_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms INTEGER NOT NULL,
    symbol TEXT NOT NULL,
    timeframe TEXT NOT NULL,
    regime TEXT NOT NULL,
    previous TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_regime_history ON regime_history (symbol, timeframe, ts_ms);
