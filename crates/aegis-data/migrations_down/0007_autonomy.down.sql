-- Rollback of 0007_autonomy.sql. Grants and the permission trail are lost; nothing else is
-- touched. Run with the engine stopped, after a backup:
--   sqlite3 data/aegis.db < crates/aegis-data/migrations_down/0007_autonomy.down.sql
DROP INDEX IF EXISTS idx_permission_events_time;
DROP TABLE IF EXISTS permission_events;
DROP INDEX IF EXISTS idx_permission_grants_time;
DROP TABLE IF EXISTS permission_grants;
DELETE FROM _sqlx_migrations WHERE version = 7;
