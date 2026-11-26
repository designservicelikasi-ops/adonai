//! AEGIS DATA — durable storage (SQLite via SQLx).
//!
//! Every write that spans several rows runs inside a transaction. The audit log is
//! append-only (enforced by triggers) and hash-chained so tampering is detectable.

mod journal;
mod permissions;
mod records;
pub use journal::{DatasetRow, JournalEntry, ProbabilityModelRow};
pub use permissions::{PermissionEventRow, PermissionGrantRow};
pub use records::*;

use aegis_types::{
    now_ms, AccountSnapshot, Candle, DataOrigin, Deal, Position, Side, SymbolSpec, Timeframe,
    TimestampMs,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::Row;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("duplicate record: {0}")]
    Duplicate(String),
    #[error("invalid data: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("audit chain broken at entry {0}")]
    AuditChainBroken(i64),
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type DataResult<T> = Result<T, DataError>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    audit_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    path: Option<PathBuf>,
}

fn map_unique(e: sqlx::Error, what: &str) -> DataError {
    match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => DataError::Duplicate(what.into()),
        _ => DataError::Sqlx(e),
    }
}

fn side_of(s: &str) -> DataResult<Side> {
    Side::parse(s).ok_or_else(|| DataError::Invalid(format!("side `{s}`")))
}

impl Store {
    /// Open (creating if needed) the database file and apply pending migrations.
    pub async fn open(path: &Path) -> DataResult<Store> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            // Slow disks (Windows, VPS) can hold a write lock for a few seconds; giving up
            // early would mark the database as failed and block every entry.
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Store { pool, audit_lock: Default::default(), path: Some(path.to_path_buf()) })
    }

    /// In-memory database (tests only).
    pub async fn open_in_memory() -> DataResult<Store> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Store { pool, audit_lock: Default::default(), path: None })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Close the database. The write-ahead log is first folded back into the main
    /// file, so that after a clean stop `aegis.db` alone holds everything.
    pub async fn close(&self) {
        if self.path.is_some() {
            if let Err(e) = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&self.pool).await {
                tracing::warn!("checkpoint on close failed (the data stays safe in the write-ahead log): {e}");
            }
        }
        self.pool.close().await;
    }

    /// Liveness probe used by the supervisor. It WRITES, because a database that can
    /// be read but not written cannot record decisions and must count as down.
    pub async fn ping(&self) -> DataResult<()> {
        self.kv_set("_ping", &now_ms()).await
    }

    // ---------------------------------------------------------------- audit

    pub async fn audit(&self, actor: &str, event: &str, detail: &Value) -> DataResult<i64> {
        let _guard = self.audit_lock.lock().await;
        let ts = now_ms();
        let detail_s = serde_json::to_string(detail)?;
        let mut tx = self.pool.begin().await?;
        let prev: Option<String> =
            sqlx::query_scalar("SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?;
        let prev = prev.unwrap_or_else(|| GENESIS_HASH.to_string());
        let hash = audit_hash(&prev, ts, actor, event, &detail_s);
        let id = sqlx::query(
            "INSERT INTO audit_log (ts_ms, actor, event, detail, prev_hash, hash) VALUES (?,?,?,?,?,?)",
        )
        .bind(ts)
        .bind(actor)
        .bind(event)
        .bind(&detail_s)
        .bind(&prev)
        .bind(&hash)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        tx.commit().await?;
        Ok(id)
    }

    pub async fn audit_tail(&self, limit: i64) -> DataResult<Vec<AuditEntry>> {
        let rows = sqlx::query(
            "SELECT id, ts_ms, actor, event, detail FROM audit_log ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| AuditEntry {
                id: r.get(0),
                ts_ms: r.get(1),
                actor: r.get(2),
                event: r.get(3),
                detail: r.get(4),
            })
            .collect())
    }

    /// Recompute the whole hash chain. Returns the number of verified entries.
    pub async fn verify_audit_chain(&self) -> DataResult<u64> {
        let rows = sqlx::query(
            "SELECT id, ts_ms, actor, event, detail, prev_hash, hash FROM audit_log ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut prev = GENESIS_HASH.to_string();
        let mut n = 0u64;
        for r in rows {
            let id: i64 = r.get(0);
            let stored_prev: String = r.get(5);
            let stored_hash: String = r.get(6);
            let expect = audit_hash(
                &prev,
                r.get(1),
                r.get::<String, _>(2).as_str(),
                r.get::<String, _>(3).as_str(),
                r.get::<String, _>(4).as_str(),
            );
            if stored_prev != prev || stored_hash != expect {
                return Err(DataError::AuditChainBroken(id));
            }
            prev = stored_hash;
            n += 1;
        }
        Ok(n)
    }

    // ------------------------------------------------------------- kv state

    pub async fn kv_set<T: Serialize>(&self, key: &str, value: &T) -> DataResult<()> {
        let v = serde_json::to_string(value)?;
        sqlx::query(
            "INSERT INTO kv_state (key, value_json, updated_ms) VALUES (?,?,?)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_ms = excluded.updated_ms",
        )
        .bind(key)
        .bind(v)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn kv_get<T: DeserializeOwned>(&self, key: &str) -> DataResult<Option<T>> {
        let v: Option<String> = sqlx::query_scalar("SELECT value_json FROM kv_state WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        match v {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------- symbols

    pub async fn upsert_symbol(&self, spec: &SymbolSpec) -> DataResult<()> {
        spec.validate().map_err(|e| DataError::Invalid(e.to_string()))?;
        sqlx::query(
            "INSERT INTO symbols (symbol, spec_json, updated_ms) VALUES (?,?,?)
             ON CONFLICT(symbol) DO UPDATE SET spec_json = excluded.spec_json, updated_ms = excluded.updated_ms",
        )
        .bind(&spec.symbol)
        .bind(serde_json::to_string(spec)?)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_symbols(&self) -> DataResult<Vec<SymbolSpec>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT spec_json FROM symbols ORDER BY symbol")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }

    // -------------------------------------------------------------- candles

    /// Insert or refresh candles atomically. Any invalid candle aborts the whole batch.
    pub async fn insert_candles(
        &self,
        symbol: &str,
        tf: Timeframe,
        origin: DataOrigin,
        candles: &[Candle],
    ) -> DataResult<u64> {
        for c in candles {
            if !c.is_valid() || c.open_ts < 0 || c.open_ts % tf.millis() != 0 {
                return Err(DataError::Invalid(format!(
                    "candle {symbol} {tf} @ {} failed validation",
                    c.open_ts
                )));
            }
        }
        let mut tx = self.pool.begin().await?;
        let mut n = 0;
        for c in candles {
            n += sqlx::query(
                "INSERT INTO candles (symbol, timeframe, open_ts, open, high, low, close, tick_volume, spread_points, origin)
                 VALUES (?,?,?,?,?,?,?,?,?,?)
                 ON CONFLICT(symbol, timeframe, open_ts, origin) DO UPDATE SET
                   open = excluded.open, high = excluded.high, low = excluded.low, close = excluded.close,
                   tick_volume = excluded.tick_volume, spread_points = excluded.spread_points",
            )
            .bind(symbol)
            .bind(tf.as_str())
            .bind(c.open_ts)
            .bind(c.open)
            .bind(c.high)
            .bind(c.low)
            .bind(c.close)
            .bind(c.tick_volume)
            .bind(c.spread_points)
            .bind(origin.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok(n)
    }

    /// The most recent `limit` candles, oldest first.
    pub async fn load_candles(
        &self,
        symbol: &str,
        tf: Timeframe,
        origin: DataOrigin,
        limit: i64,
    ) -> DataResult<Vec<Candle>> {
        let rows = sqlx::query(
            "SELECT open_ts, open, high, low, close, tick_volume, spread_points FROM candles
             WHERE symbol = ? AND timeframe = ? AND origin = ? ORDER BY open_ts DESC LIMIT ?",
        )
        .bind(symbol)
        .bind(tf.as_str())
        .bind(origin.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<Candle> = rows
            .into_iter()
            .map(|r| Candle {
                open_ts: r.get(0),
                open: r.get(1),
                high: r.get(2),
                low: r.get(3),
                close: r.get(4),
                tick_volume: r.get(5),
                spread_points: r.get(6),
            })
            .collect();
        out.reverse();
        Ok(out)
    }

    pub async fn candle_count(&self, symbol: &str, tf: Timeframe, origin: DataOrigin) -> DataResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM candles WHERE symbol = ? AND timeframe = ? AND origin = ?",
        )
        .bind(symbol)
        .bind(tf.as_str())
        .bind(origin.as_str())
        .fetch_one(&self.pool)
        .await?)
    }

    // -------------------------------------------------------------- account

    pub async fn insert_account_snapshot(&self, a: &AccountSnapshot) -> DataResult<()> {
        if !a.is_sane() {
            return Err(DataError::Invalid("account snapshot".into()));
        }
        sqlx::query(
            "INSERT INTO account_snapshots (ts_ms, login, balance, equity, margin, margin_free, snapshot_json)
             VALUES (?,?,?,?,?,?,?)",
        )
        .bind(a.ts_ms)
        .bind(a.login)
        .bind(a.balance)
        .bind(a.equity)
        .bind(a.margin)
        .bind(a.margin_free)
        .bind(serde_json::to_string(a)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn equity_history(&self, limit: i64) -> DataResult<Vec<(TimestampMs, f64)>> {
        let rows = sqlx::query("SELECT ts_ms, equity FROM account_snapshots ORDER BY id DESC LIMIT ?")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        let mut v: Vec<(i64, f64)> = rows.into_iter().map(|r| (r.get(0), r.get(1))).collect();
        v.reverse();
        Ok(v)
    }

    // ----------------------------------------------------------- strategies

    pub async fn upsert_strategy(&self, row: &StrategyRow) -> DataResult<()> {
        sqlx::query(
            "INSERT INTO strategies (strategy_id, name, stage, enabled, config_json, updated_ms) VALUES (?,?,?,?,?,?)
             ON CONFLICT(strategy_id) DO UPDATE SET name = excluded.name, config_json = excluded.config_json,
               updated_ms = excluded.updated_ms",
        )
        .bind(&row.strategy_id)
        .bind(&row.name)
        .bind(&row.stage)
        .bind(row.enabled)
        .bind(serde_json::to_string(&row.config)?)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_strategy_stage(&self, id: &str, stage: &str) -> DataResult<()> {
        let n = sqlx::query("UPDATE strategies SET stage = ?, updated_ms = ? WHERE strategy_id = ?")
            .bind(stage)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(DataError::NotFound(format!("strategy {id}")));
        }
        Ok(())
    }

    pub async fn set_strategy_enabled(&self, id: &str, enabled: bool) -> DataResult<()> {
        let n = sqlx::query("UPDATE strategies SET enabled = ?, updated_ms = ? WHERE strategy_id = ?")
            .bind(enabled)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(DataError::NotFound(format!("strategy {id}")));
        }
        Ok(())
    }

    pub async fn load_strategies(&self) -> DataResult<Vec<StrategyRow>> {
        let rows = sqlx::query(
            "SELECT strategy_id, name, stage, enabled, config_json FROM strategies ORDER BY strategy_id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(StrategyRow {
                    strategy_id: r.get(0),
                    name: r.get(1),
                    stage: r.get(2),
                    enabled: r.get(3),
                    config: serde_json::from_str(r.get::<String, _>(4).as_str())?,
                })
            })
            .collect()
    }

    // ------------------------------------------------------------ decisions

    pub async fn insert_decision(&self, d: &DecisionRecord) -> DataResult<()> {
        sqlx::query(
            "INSERT INTO decisions (decision_id, ts_ms, symbol, strategy_id, side, mode, signal_json, outcome, verdict_json)
             VALUES (?,?,?,?,?,?,?,?,?)",
        )
        .bind(&d.decision_id)
        .bind(d.ts_ms)
        .bind(&d.symbol)
        .bind(&d.strategy_id)
        .bind(d.side.as_str())
        .bind(&d.mode)
        .bind(serde_json::to_string(&d.signal)?)
        .bind(&d.outcome)
        .bind(d.verdict.as_ref().map(serde_json::to_string).transpose()?)
        .execute(&self.pool)
        .await
        .map_err(|e| map_unique(e, &d.decision_id))?;
        Ok(())
    }

    pub async fn set_decision_outcome(&self, id: &str, outcome: &str, verdict: &Value) -> DataResult<()> {
        let n = sqlx::query("UPDATE decisions SET outcome = ?, verdict_json = ? WHERE decision_id = ?")
            .bind(outcome)
            .bind(serde_json::to_string(verdict)?)
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(DataError::NotFound(format!("decision {id}")));
        }
        Ok(())
    }

    pub async fn recent_decisions(&self, limit: i64) -> DataResult<Vec<DecisionRecord>> {
        let rows = sqlx::query(
            "SELECT decision_id, ts_ms, symbol, strategy_id, side, mode, signal_json, outcome, verdict_json
             FROM decisions ORDER BY ts_ms DESC, decision_id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let verdict: Option<String> = r.get(8);
                Ok(DecisionRecord {
                    decision_id: r.get(0),
                    ts_ms: r.get(1),
                    symbol: r.get(2),
                    strategy_id: r.get(3),
                    side: side_of(r.get::<String, _>(4).as_str())?,
                    mode: r.get(5),
                    signal: serde_json::from_str(r.get::<String, _>(6).as_str())?,
                    outcome: r.get(7),
                    verdict: verdict.map(|v| serde_json::from_str(&v)).transpose()?,
                })
            })
            .collect()
    }

    // --------------------------------------------------------------- orders

    /// Insert a new order and its first lifecycle event atomically.
    /// Fails with `Duplicate` if the `cid` or the `auth_id` was already used.
    pub async fn insert_order(&self, o: &OrderRecord) -> DataResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO orders (cid, decision_id, auth_id, symbol, side, kind, volume, price, sl, tp, status,
               ticket, fill_price, fill_volume, retcode, message, created_ms, updated_ms)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&o.cid)
        .bind(&o.decision_id)
        .bind(&o.auth_id)
        .bind(&o.symbol)
        .bind(o.side.as_str())
        .bind(o.kind.as_str())
        .bind(o.volume)
        .bind(o.price)
        .bind(o.sl)
        .bind(o.tp)
        .bind(&o.status)
        .bind(o.ticket)
        .bind(o.fill_price)
        .bind(o.fill_volume)
        .bind(o.retcode)
        .bind(&o.message)
        .bind(o.created_ms)
        .bind(o.updated_ms)
        .execute(&mut *tx)
        .await
        .map_err(|e| map_unique(e, &o.cid))?;
        sqlx::query("INSERT INTO order_events (cid, ts_ms, status, detail) VALUES (?,?,?,?)")
            .bind(&o.cid)
            .bind(o.created_ms)
            .bind(&o.status)
            .bind("created")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_order(&self, u: &OrderUpdate) -> DataResult<()> {
        let ts = now_ms();
        let mut tx = self.pool.begin().await?;
        let n = sqlx::query(
            "UPDATE orders SET status = ?, ticket = COALESCE(?, ticket), fill_price = COALESCE(?, fill_price),
               fill_volume = COALESCE(?, fill_volume), retcode = COALESCE(?, retcode),
               message = COALESCE(?, message), updated_ms = ? WHERE cid = ?",
        )
        .bind(&u.status)
        .bind(u.ticket)
        .bind(u.fill_price)
        .bind(u.fill_volume)
        .bind(u.retcode)
        .bind(&u.message)
        .bind(ts)
        .bind(&u.cid)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if n == 0 {
            return Err(DataError::NotFound(format!("order {}", u.cid)));
        }
        sqlx::query("INSERT INTO order_events (cid, ts_ms, status, detail) VALUES (?,?,?,?)")
            .bind(&u.cid)
            .bind(ts)
            .bind(&u.status)
            .bind(u.message.clone().unwrap_or_default())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    fn order_from_row(r: &sqlx::sqlite::SqliteRow) -> DataResult<OrderRecord> {
        Ok(OrderRecord {
            cid: r.get("cid"),
            decision_id: r.get("decision_id"),
            auth_id: r.get("auth_id"),
            symbol: r.get("symbol"),
            side: side_of(r.get::<String, _>("side").as_str())?,
            kind: aegis_types::OrderKind::parse(r.get::<String, _>("kind").as_str())
                .ok_or_else(|| DataError::Invalid("order kind".into()))?,
            volume: r.get("volume"),
            price: r.get("price"),
            sl: r.get("sl"),
            tp: r.get("tp"),
            status: r.get("status"),
            ticket: r.get("ticket"),
            fill_price: r.get("fill_price"),
            fill_volume: r.get("fill_volume"),
            retcode: r.get("retcode"),
            message: r.get("message"),
            created_ms: r.get("created_ms"),
            updated_ms: r.get("updated_ms"),
        })
    }

    pub async fn get_order(&self, cid: &str) -> DataResult<Option<OrderRecord>> {
        let row = sqlx::query("SELECT * FROM orders WHERE cid = ?")
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(Self::order_from_row).transpose()
    }

    pub async fn orders_with_status(&self, statuses: &[&str]) -> DataResult<Vec<OrderRecord>> {
        let rows = sqlx::query("SELECT * FROM orders ORDER BY created_ms ASC").fetch_all(&self.pool).await?;
        let mut out = Vec::new();
        for r in &rows {
            let o = Self::order_from_row(r)?;
            if statuses.contains(&o.status.as_str()) {
                out.push(o);
            }
        }
        Ok(out)
    }

    pub async fn recent_orders(&self, limit: i64) -> DataResult<Vec<OrderRecord>> {
        let rows = sqlx::query("SELECT * FROM orders ORDER BY created_ms DESC LIMIT ?")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(Self::order_from_row).collect()
    }

    pub async fn order_events(&self, cid: &str) -> DataResult<Vec<(TimestampMs, String, String)>> {
        let rows = sqlx::query("SELECT ts_ms, status, detail FROM order_events WHERE cid = ? ORDER BY id ASC")
            .bind(cid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect())
    }

    /// Broker ticket -> strategy that opened it (through its order and decision).
    pub async fn ticket_strategies(&self) -> DataResult<Vec<(i64, String)>> {
        let rows = sqlx::query(
            "SELECT o.ticket, d.strategy_id FROM orders o JOIN decisions d ON d.decision_id = o.decision_id
             WHERE o.ticket IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    pub async fn strategy_of_order(&self, cid: &str) -> DataResult<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT d.strategy_id FROM orders o JOIN decisions d ON d.decision_id = o.decision_id WHERE o.cid = ?",
        )
        .bind(cid)
        .fetch_optional(&self.pool)
        .await?)
    }

    // ------------------------------------------------------------- commands

    pub async fn insert_command(&self, c: &CommandRecord) -> DataResult<()> {
        sqlx::query(
            "INSERT INTO commands (cid, action, ticket, symbol, volume, sl, tp, reason, status, retcode, message, created_ms, updated_ms)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&c.cid)
        .bind(&c.action)
        .bind(c.ticket)
        .bind(&c.symbol)
        .bind(c.volume)
        .bind(c.sl)
        .bind(c.tp)
        .bind(&c.reason)
        .bind(&c.status)
        .bind(c.retcode)
        .bind(&c.message)
        .bind(c.created_ms)
        .bind(c.updated_ms)
        .execute(&self.pool)
        .await
        .map_err(|e| map_unique(e, &c.cid))?;
        Ok(())
    }

    pub async fn update_command(&self, cid: &str, status: &str, retcode: Option<i64>, message: Option<&str>) -> DataResult<()> {
        let n = sqlx::query(
            "UPDATE commands SET status = ?, retcode = COALESCE(?, retcode), message = COALESCE(?, message), updated_ms = ? WHERE cid = ?",
        )
        .bind(status)
        .bind(retcode)
        .bind(message)
        .bind(now_ms())
        .bind(cid)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if n == 0 {
            return Err(DataError::NotFound(format!("command {cid}")));
        }
        Ok(())
    }

    pub async fn recent_commands(&self, limit: i64) -> DataResult<Vec<CommandRecord>> {
        let rows = sqlx::query("SELECT * FROM commands ORDER BY created_ms DESC LIMIT ?")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| CommandRecord {
                cid: r.get("cid"),
                action: r.get("action"),
                ticket: r.get("ticket"),
                symbol: r.get("symbol"),
                volume: r.get("volume"),
                sl: r.get("sl"),
                tp: r.get("tp"),
                reason: r.get("reason"),
                status: r.get("status"),
                retcode: r.get("retcode"),
                message: r.get("message"),
                created_ms: r.get("created_ms"),
                updated_ms: r.get("updated_ms"),
            })
            .collect())
    }

    // ------------------------------------------------------------ positions

    /// Replace the open-position picture with the broker's snapshot (atomic).
    /// Positions absent from the snapshot are marked closed, never deleted.
    pub async fn sync_positions(&self, positions: &[Position]) -> DataResult<()> {
        let ts = now_ms();
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE positions SET is_open = 0, updated_ms = ? WHERE is_open = 1")
            .bind(ts)
            .execute(&mut *tx)
            .await?;
        for p in positions {
            sqlx::query(
                "INSERT INTO positions (ticket, symbol, side, volume, price_open, sl, tp, open_ts, is_open, position_json, updated_ms)
                 VALUES (?,?,?,?,?,?,?,?,1,?,?)
                 ON CONFLICT(ticket) DO UPDATE SET volume = excluded.volume, sl = excluded.sl, tp = excluded.tp,
                   is_open = 1, position_json = excluded.position_json, updated_ms = excluded.updated_ms",
            )
            .bind(p.ticket)
            .bind(&p.symbol)
            .bind(p.side.as_str())
            .bind(p.volume)
            .bind(p.price_open)
            .bind(p.sl)
            .bind(p.tp)
            .bind(p.open_ts)
            .bind(serde_json::to_string(p)?)
            .bind(ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn open_positions(&self) -> DataResult<Vec<Position>> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT position_json FROM positions WHERE is_open = 1 ORDER BY ticket")
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }

    // ---------------------------------------------------------------- deals

    /// Returns `true` when the deal was new, `false` when it was already recorded.
    pub async fn insert_deal(&self, d: &Deal) -> DataResult<bool> {
        let n = sqlx::query(
            "INSERT OR IGNORE INTO deals (deal_ticket, position_ticket, symbol, side, volume, price, profit, swap, commission, ts_ms, comment, origin, fee)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(d.deal_ticket)
        .bind(d.position_ticket)
        .bind(&d.symbol)
        .bind(d.side.as_str())
        .bind(d.volume)
        .bind(d.price)
        .bind(d.profit)
        .bind(d.swap)
        .bind(d.commission)
        .bind(d.ts_ms)
        .bind(&d.comment)
        .bind(&d.origin)
        .bind(d.fee)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    pub async fn deals_since(&self, since_ms: TimestampMs, limit: i64) -> DataResult<Vec<Deal>> {
        let rows = sqlx::query(
            "SELECT deal_ticket, position_ticket, symbol, side, volume, price, profit, swap, commission, ts_ms, comment, origin, fee
             FROM deals WHERE ts_ms >= ? ORDER BY ts_ms DESC LIMIT ?",
        )
        .bind(since_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Deal {
                    deal_ticket: r.get(0),
                    position_ticket: r.get(1),
                    symbol: r.get(2),
                    side: side_of(r.get::<String, _>(3).as_str())?,
                    volume: r.get(4),
                    price: r.get(5),
                    profit: r.get(6),
                    swap: r.get(7),
                    commission: r.get(8),
                    ts_ms: r.get(9),
                    comment: r.get(10),
                    origin: r.get(11),
                    fee: r.get(12),
                })
            })
            .collect()
    }

    // ---------------------------------------------------------- risk config

    pub async fn save_risk_config(&self, actor: &str, config: &Value) -> DataResult<i64> {
        Ok(sqlx::query("INSERT INTO risk_configs (ts_ms, actor, config_json) VALUES (?,?,?)")
            .bind(now_ms())
            .bind(actor)
            .bind(serde_json::to_string(config)?)
            .execute(&self.pool)
            .await?
            .last_insert_rowid())
    }

    pub async fn latest_risk_config(&self) -> DataResult<Option<Value>> {
        let v: Option<String> =
            sqlx::query_scalar("SELECT config_json FROM risk_configs ORDER BY version DESC LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(v.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    // --------------------------------------------------------------- models

    pub async fn register_model(&self, m: &ModelRow) -> DataResult<()> {
        sqlx::query(
            "INSERT INTO model_versions (model_id, created_ms, status, origin, path, metrics_json) VALUES (?,?,?,?,?,?)
             ON CONFLICT(model_id) DO UPDATE SET status = excluded.status, path = excluded.path,
               metrics_json = excluded.metrics_json",
        )
        .bind(&m.model_id)
        .bind(m.created_ms)
        .bind(&m.status)
        .bind(m.origin.as_str())
        .bind(&m.path)
        .bind(serde_json::to_string(&m.metrics)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_models(&self) -> DataResult<Vec<ModelRow>> {
        let rows = sqlx::query(
            "SELECT model_id, created_ms, status, origin, path, metrics_json FROM model_versions ORDER BY created_ms DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(ModelRow {
                    model_id: r.get(0),
                    created_ms: r.get(1),
                    status: r.get(2),
                    origin: DataOrigin::parse(r.get::<String, _>(3).as_str())
                        .ok_or_else(|| DataError::Invalid("origin".into()))?,
                    path: r.get(4),
                    metrics: serde_json::from_str(r.get::<String, _>(5).as_str())?,
                })
            })
            .collect()
    }

    // ------------------------------------------------------------ backtests

    pub async fn save_backtest(&self, run: &BacktestRunRow, trades: &[(String, Value)]) -> DataResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO backtest_runs (run_id, ts_ms, strategy_id, symbol, timeframe, origin, params_json, metrics_json)
             VALUES (?,?,?,?,?,?,?,?)",
        )
        .bind(&run.run_id)
        .bind(run.ts_ms)
        .bind(&run.strategy_id)
        .bind(&run.symbol)
        .bind(&run.timeframe)
        .bind(run.origin.as_str())
        .bind(serde_json::to_string(&run.params)?)
        .bind(serde_json::to_string(&run.metrics)?)
        .execute(&mut *tx)
        .await
        .map_err(|e| map_unique(e, &run.run_id))?;
        for (segment, trade) in trades {
            sqlx::query("INSERT INTO backtest_trades (run_id, segment, trade_json) VALUES (?,?,?)")
                .bind(&run.run_id)
                .bind(segment)
                .bind(serde_json::to_string(trade)?)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_backtests(&self, limit: i64) -> DataResult<Vec<BacktestRunRow>> {
        let rows = sqlx::query(
            "SELECT run_id, ts_ms, strategy_id, symbol, timeframe, origin, params_json, metrics_json
             FROM backtest_runs ORDER BY ts_ms DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(BacktestRunRow {
                    run_id: r.get(0),
                    ts_ms: r.get(1),
                    strategy_id: r.get(2),
                    symbol: r.get(3),
                    timeframe: r.get(4),
                    origin: DataOrigin::parse(r.get::<String, _>(5).as_str())
                        .ok_or_else(|| DataError::Invalid("origin".into()))?,
                    params: serde_json::from_str(r.get::<String, _>(6).as_str())?,
                    metrics: serde_json::from_str(r.get::<String, _>(7).as_str())?,
                })
            })
            .collect()
    }

    pub async fn backtest_trade_count(&self, run_id: &str) -> DataResult<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM backtest_trades WHERE run_id = ?")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await?)
    }

    // ------------------------------------------- backup / restore / retention

    pub async fn integrity_check(&self) -> DataResult<bool> {
        let r: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&self.pool).await?;
        let fk = sqlx::query("PRAGMA foreign_key_check").fetch_all(&self.pool).await?;
        Ok(r == "ok" && fk.is_empty())
    }

    /// Consistent online backup into a new file (`VACUUM INTO`). Fails if `dest` exists.
    pub async fn backup_to(&self, dest: &Path) -> DataResult<()> {
        if dest.exists() {
            return Err(DataError::Invalid(format!("backup target {} already exists", dest.display())));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let dest_s = dest
            .to_str()
            .ok_or_else(|| DataError::Invalid("backup path is not valid UTF-8".into()))?;
        sqlx::query("VACUUM INTO ?").bind(dest_s).execute(&self.pool).await?;
        Ok(())
    }

    /// Verify a backup file and copy it over `target`. The engine must be stopped.
    /// An existing target is first preserved as `<target>.before-restore`.
    pub async fn restore_from(backup: &Path, target: &Path) -> DataResult<()> {
        let probe = Store::open(backup).await?;
        let ok = probe.integrity_check().await?;
        probe.verify_audit_chain().await?;
        probe.close().await;
        if !ok {
            return Err(DataError::Invalid("backup failed integrity check".into()));
        }
        if target.exists() {
            let mut keep = target.as_os_str().to_owned();
            keep.push(".before-restore");
            std::fs::copy(target, PathBuf::from(keep))?;
        }
        for ext in ["-wal", "-shm"] {
            let mut side = target.as_os_str().to_owned();
            side.push(ext);
            let side = PathBuf::from(side);
            if side.exists() {
                std::fs::remove_file(side)?;
            }
        }
        std::fs::copy(backup, target)?;
        Ok(())
    }

    /// Delete market data and account snapshots older than the retention windows.
    /// Decisions, orders, deals and the audit log are never purged.
    pub async fn apply_retention(&self, candle_days: i64, snapshot_days: i64) -> DataResult<(u64, u64)> {
        let now = now_ms();
        let c = sqlx::query("DELETE FROM candles WHERE open_ts < ?")
            .bind(now - candle_days * 86_400_000)
            .execute(&self.pool)
            .await?
            .rows_affected();
        let s = sqlx::query("DELETE FROM account_snapshots WHERE ts_ms < ?")
            .bind(now - snapshot_days * 86_400_000)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok((c, s))
    }
}

fn audit_hash(prev: &str, ts: i64, actor: &str, event: &str, detail: &str) -> String {
    let mut h = Sha256::new();
    for part in [prev, &ts.to_string(), actor, event, detail] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())
}
