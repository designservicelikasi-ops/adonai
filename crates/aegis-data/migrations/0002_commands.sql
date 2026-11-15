-- Risk-reducing management commands (close / modify / cancel). They are not
-- entries, so they have no decision or authorization, but every one is traced.
CREATE TABLE commands (
    cid        TEXT PRIMARY KEY,
    action     TEXT NOT NULL CHECK (action IN ('close', 'modify', 'cancel')),
    ticket     INTEGER NOT NULL,
    symbol     TEXT NOT NULL,
    volume     REAL NOT NULL,
    sl         REAL NOT NULL,
    tp         REAL NOT NULL,
    reason     TEXT NOT NULL,
    status     TEXT NOT NULL,
    retcode    INTEGER,
    message    TEXT,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE INDEX idx_commands_status ON commands (status);
