-- v0.3: AEGIS AUTONOMY COMMANDER. Grants (who authorized what, on which account, until when)
-- and the permission audit trail (every grant, revocation, lapse, and every autonomous action
-- checked against the permissions). Rollback: migrations_down/0007_autonomy.down.sql
CREATE TABLE permission_grants (
    grant_id TEXT PRIMARY KEY,
    profile TEXT NOT NULL,
    account_login INTEGER,
    account_server TEXT,
    granted_by TEXT NOT NULL,
    granted_ms INTEGER NOT NULL,
    expires_ms INTEGER,
    risk_config_hash TEXT,
    persist_across_restart INTEGER NOT NULL DEFAULT 0,
    revoked_ms INTEGER,
    revoked_by TEXT,
    revoke_reason TEXT,
    note TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_permission_grants_time ON permission_grants (granted_ms);

CREATE TABLE permission_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms INTEGER NOT NULL,
    event TEXT NOT NULL,
    actor TEXT NOT NULL,
    requester TEXT NOT NULL DEFAULT '',
    capability TEXT NOT NULL DEFAULT '',
    profile TEXT NOT NULL DEFAULT '',
    grant_id TEXT,
    account_login INTEGER,
    allowed INTEGER NOT NULL DEFAULT 1,
    detail TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_permission_events_time ON permission_events (ts_ms);
