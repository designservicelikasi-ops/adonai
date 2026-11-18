//! Unattended operation: heartbeat file, scheduled backups with rotation,
//! critical alerts (view + log + optional webhook).

use crate::engine::Engine;
use aegis_types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub ts_ms: TimestampMs,
    /// `critical` | `warning`
    pub level: String,
    pub source: String,
    pub text: String,
}

const AUTO_PREFIX: &str = "aegis-auto-";

impl Engine {
    /// Record an alert; sent to the webhook when one is configured. Never blocks the engine.
    pub(crate) fn alert(&mut self, level: &str, source: &str, text: String) {
        let a = Alert { ts_ms: now_ms(), level: level.into(), source: source.into(), text: text.clone() };
        self.log(if level == "critical" { "error" } else { "warn" }, source, format!("ALERTE: {text}"));
        self.alerts.push_front(a.clone());
        self.alerts.truncate(100);
        let url = self.cfg.ops.alert_webhook_url.clone();
        if !url.is_empty() {
            let body = json!({ "app": "AEGIS", "level": a.level, "source": a.source, "text": a.text, "ts_ms": a.ts_ms, "account": self.account.as_ref().map(|x| x.login) });
            tokio::task::spawn_blocking(move || {
                let r = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(10)).build().post(&url).send_json(body);
                if let Err(e) = r {
                    tracing::warn!("alert webhook failed: {e}");
                }
            });
        }
        self.cache.dirty = true;
    }

    pub(crate) async fn run_ops(&mut self, now: TimestampMs) {
        let ops = self.cfg.ops.clone();
        if ops.heartbeat_file && now - self.heartbeat_ms >= 10_000 {
            self.heartbeat_ms = now;
            let line = format!("{now} {} {}\n", self.mode.label(), if self.conn.is_some() { "terminal-connected" } else { "terminal-disconnected" });
            let _ = std::fs::write(self.cfg.engine.data_dir.join("heartbeat"), line);
        }
        if ops.backup_interval_hours > 0 && now - self.last_auto_backup_ms >= ops.backup_interval_hours as i64 * 3_600_000 {
            self.last_auto_backup_ms = now;
            let _ = self.store.kv_set("last_auto_backup_ms", &now).await;
            match self.auto_backup(now, ops.backup_keep).await {
                Ok(msg) => self.log("info", "data", msg),
                Err(e) => self.alert("critical", "data", format!("sauvegarde automatique impossible: {e}")),
            }
        }
        // Terminal silent while automation is on.
        match (&self.conn, self.mode.can_execute()) {
            (None, true) => {
                let since = *self.disconnected_since.get_or_insert(now);
                if now - since >= ops.disconnect_alert_s * 1_000 && !self.disconnect_alerted {
                    self.disconnect_alerted = true;
                    self.alert("critical", "bridge", format!("terminal MT5 déconnecté depuis {} s en mode {}: entrées bloquées", (now - since) / 1_000, self.mode.label()));
                }
            }
            _ => {
                if self.conn.is_some() && self.disconnect_alerted {
                    self.alert("warning", "bridge", "terminal MT5 reconnecté: réconciliation avant toute nouvelle entrée".into());
                }
                self.disconnected_since = None;
                self.disconnect_alerted = false;
            }
        }
        if !self.db_ok && !self.db_alerted {
            self.db_alerted = true;
            self.alert("critical", "data", "base de données indisponible: entrées bloquées".into());
        } else if self.db_ok {
            self.db_alerted = false;
        }
    }

    async fn auto_backup(&mut self, now: TimestampMs, keep: usize) -> Result<String, String> {
        let dir = self.cfg.engine.data_dir.join("backups");
        let dest = dir.join(format!("{AUTO_PREFIX}{}.db", format_ts(now).replace([' ', ':'], "-")));
        self.store.backup_to(&dest).await.map_err(|e| e.to_string())?;
        self.last_backup = Some(dest.display().to_string());
        self.audit("engine", "backup_created", json!({ "path": dest.display().to_string(), "automatic": true })).await;
        // Rotation: only automatic backups are deleted, oldest first.
        let mut autos: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(AUTO_PREFIX) && n.ends_with(".db")))
            .collect();
        autos.sort();
        let removed = autos.len().saturating_sub(keep);
        for p in autos.iter().take(removed) {
            let _ = std::fs::remove_file(p);
        }
        Ok(format!("sauvegarde automatique: {} ({removed} ancienne(s) supprimée(s))", dest.display()))
    }
}
