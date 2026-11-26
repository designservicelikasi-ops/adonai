//! Autonomy Commander persistence: grants and the permission audit trail.

use crate::{DataResult, Store};
use aegis_types::TimestampMs;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionGrantRow {
    pub grant_id: String,
    pub profile: String,
    pub account_login: Option<i64>,
    pub account_server: Option<String>,
    pub granted_by: String,
    pub granted_ms: TimestampMs,
    pub expires_ms: Option<TimestampMs>,
    pub risk_config_hash: Option<String>,
    pub persist_across_restart: bool,
    pub revoked_ms: Option<TimestampMs>,
    pub revoked_by: Option<String>,
    pub revoke_reason: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PermissionEventRow {
    pub id: i64,
    pub ts_ms: TimestampMs,
    /// grant | revoke | lapse | denied | action | default_profile
    pub event: String,
    pub actor: String,
    pub requester: String,
    pub capability: String,
    pub profile: String,
    pub grant_id: Option<String>,
    pub account_login: Option<i64>,
    pub allowed: bool,
    pub detail: String,
}

fn grant_of(r: &sqlx::sqlite::SqliteRow) -> PermissionGrantRow {
    PermissionGrantRow {
        grant_id: r.get("grant_id"),
        profile: r.get("profile"),
        account_login: r.get("account_login"),
        account_server: r.get("account_server"),
        granted_by: r.get("granted_by"),
        granted_ms: r.get("granted_ms"),
        expires_ms: r.get("expires_ms"),
        risk_config_hash: r.get("risk_config_hash"),
        persist_across_restart: r.get::<i64, _>("persist_across_restart") != 0,
        revoked_ms: r.get("revoked_ms"),
        revoked_by: r.get("revoked_by"),
        revoke_reason: r.get("revoke_reason"),
        note: r.get("note"),
    }
}

impl Store {
    /// Insert a grant, or update its revocation fields when it already exists.
    pub async fn upsert_grant(&self, g: &PermissionGrantRow) -> DataResult<()> {
        sqlx::query(
            "INSERT INTO permission_grants (grant_id, profile, account_login, account_server, granted_by, granted_ms, expires_ms,
                risk_config_hash, persist_across_restart, revoked_ms, revoked_by, revoke_reason, note)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(grant_id) DO UPDATE SET revoked_ms = excluded.revoked_ms, revoked_by = excluded.revoked_by,
                revoke_reason = excluded.revoke_reason",
        )
        .bind(&g.grant_id)
        .bind(&g.profile)
        .bind(g.account_login)
        .bind(&g.account_server)
        .bind(&g.granted_by)
        .bind(g.granted_ms)
        .bind(g.expires_ms)
        .bind(&g.risk_config_hash)
        .bind(g.persist_across_restart as i64)
        .bind(g.revoked_ms)
        .bind(&g.revoked_by)
        .bind(&g.revoke_reason)
        .bind(&g.note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn grants(&self, limit: i64) -> DataResult<Vec<PermissionGrantRow>> {
        let rows = sqlx::query("SELECT * FROM permission_grants ORDER BY granted_ms DESC LIMIT ?").bind(limit).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(grant_of).collect())
    }

    /// Unrevoked grants, most recent first (the engine decides which one may be restored).
    pub async fn open_grants(&self) -> DataResult<Vec<PermissionGrantRow>> {
        let rows = sqlx::query("SELECT * FROM permission_grants WHERE revoked_ms IS NULL ORDER BY granted_ms DESC").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(grant_of).collect())
    }

    pub async fn insert_permission_event(&self, e: &PermissionEventRow) -> DataResult<i64> {
        let r = sqlx::query(
            "INSERT INTO permission_events (ts_ms, event, actor, requester, capability, profile, grant_id, account_login, allowed, detail)
             VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(e.ts_ms)
        .bind(&e.event)
        .bind(&e.actor)
        .bind(&e.requester)
        .bind(&e.capability)
        .bind(&e.profile)
        .bind(&e.grant_id)
        .bind(e.account_login)
        .bind(e.allowed as i64)
        .bind(&e.detail)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn permission_events(&self, limit: i64) -> DataResult<Vec<PermissionEventRow>> {
        let rows = sqlx::query("SELECT * FROM permission_events ORDER BY id DESC LIMIT ?").bind(limit).fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| PermissionEventRow {
                id: r.get("id"),
                ts_ms: r.get("ts_ms"),
                event: r.get("event"),
                actor: r.get("actor"),
                requester: r.get("requester"),
                capability: r.get("capability"),
                profile: r.get("profile"),
                grant_id: r.get("grant_id"),
                account_login: r.get("account_login"),
                allowed: r.get::<i64, _>("allowed") != 0,
                detail: r.get("detail"),
            })
            .collect())
    }

    /// Execute a rollback script (`migrations_down/*.down.sql`). Used by tests and by the
    /// `aegisd db-rollback` tool; the engine must be stopped.
    pub async fn run_sql_script(&self, sql: &str) -> DataResult<()> {
        sqlx::raw_sql(sql).execute(&self.pool).await?;
        Ok(())
    }

    /// Apply pending migrations again (after a rollback).
    pub async fn migrate(&self) -> DataResult<()> {
        crate::MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
}
