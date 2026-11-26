-- Rollback of 0008_intelligence.sql (SQLite >= 3.35 for DROP COLUMN). Engine stopped, after a backup:
--   sqlite3 data/aegis.db < crates/aegis-data/migrations_down/0008_intelligence.down.sql
DROP INDEX IF EXISTS idx_regime_history;
DROP TABLE IF EXISTS regime_history;
ALTER TABLE trade_journal DROP COLUMN stop_role;
ALTER TABLE trade_journal DROP COLUMN related_ticket;
ALTER TABLE trade_journal DROP COLUMN entry_kind;
ALTER TABLE trade_journal DROP COLUMN regime_detail;
DELETE FROM _sqlx_migrations WHERE version = 8;
