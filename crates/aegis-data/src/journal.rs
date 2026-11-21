//! Trade journal, versioned probability models and dataset registry (v0.2).

use crate::{side_of, DataResult, Store};
use aegis_types::{Side, TimestampMs};
use serde::{Deserialize, Serialize};
use sqlx::Row;

/// One position opened by AEGIS, from decision to final result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct JournalEntry {
    pub ticket: i64,
    pub decision_id: String,
    pub cid: String,
    pub strategy_id: String,
    pub symbol: String,
    pub timeframe: String,
    pub side: Option<Side>,
    pub regime: String,
    /// `mt5` | `simulator`
    pub terminal: String,
    pub opened_ms: TimestampMs,
    pub closed_ms: Option<TimestampMs>,
    pub volume: f64,
    pub entry_requested: f64,
    pub entry_filled: f64,
    pub exit_price: Option<f64>,
    pub sl_initial: f64,
    pub tp_initial: f64,
    /// Probability of reaching TP before SL used for the decision.
    pub probability: Option<f64>,
    pub probability_source: String,
    pub expected_net: Option<f64>,
    pub est_spread_cost: f64,
    pub est_commission: f64,
    /// Measured at the fill (positive = cost).
    pub slippage_cost: f64,
    /// Broker results, summed over the closing deals.
    pub gross: f64,
    pub commission: f64,
    pub swap: f64,
    pub fee: f64,
    pub net: f64,
    pub exit_reason: String,
    /// Maximum favourable / adverse excursion while open, in price.
    pub mfe_price: f64,
    pub mae_price: f64,
    /// Probability before the brain's calibration haircut (None = same as `probability`).
    pub probability_raw: Option<f64>,
    /// The micro-profit brain chose this trade's target and volume.
    pub brain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbabilityModelRow {
    pub model_id: String,
    pub created_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: String,
    pub origin: String,
    pub dataset_hash: String,
    pub buy_selected: String,
    pub sell_selected: String,
    pub active: bool,
    /// The full model as JSON.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetRow {
    pub dataset_hash: String,
    pub created_ms: TimestampMs,
    pub symbol: String,
    pub timeframe: String,
    pub origin: String,
    pub bars: i64,
    pub from_ts: TimestampMs,
    pub to_ts: TimestampMs,
}

fn journal_from_row(r: &sqlx::sqlite::SqliteRow) -> DataResult<JournalEntry> {
    let side: String = r.get("side");
    Ok(JournalEntry {
        ticket: r.get("ticket"),
        decision_id: r.get("decision_id"),
        cid: r.get("cid"),
        strategy_id: r.get("strategy_id"),
        symbol: r.get("symbol"),
        timeframe: r.get("timeframe"),
        side: Some(side_of(&side)?),
        regime: r.get("regime"),
        terminal: r.get("terminal"),
        opened_ms: r.get("opened_ms"),
        closed_ms: r.get("closed_ms"),
        volume: r.get("volume"),
        entry_requested: r.get("entry_requested"),
        entry_filled: r.get("entry_filled"),
        exit_price: r.get("exit_price"),
        sl_initial: r.get("sl_initial"),
        tp_initial: r.get("tp_initial"),
        probability: r.get("probability"),
        probability_source: r.get("probability_source"),
        expected_net: r.get("expected_net"),
        est_spread_cost: r.get("est_spread_cost"),
        est_commission: r.get("est_commission"),
        slippage_cost: r.get("slippage_cost"),
        gross: r.get("gross"),
        commission: r.get("commission"),
        swap: r.get("swap"),
        fee: r.get("fee"),
        net: r.get("net"),
        exit_reason: r.get("exit_reason"),
        mfe_price: r.get("mfe_price"),
        mae_price: r.get("mae_price"),
        probability_raw: r.get("probability_raw"),
        brain: r.get("brain"),
    })
}

impl Store {
    /// Record an opened position. Idempotent on the ticket (a replayed fill changes nothing).
    pub async fn journal_open(&self, e: &JournalEntry) -> DataResult<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO trade_journal (ticket, decision_id, cid, strategy_id, symbol, timeframe, side, regime, terminal,
                opened_ms, volume, entry_requested, entry_filled, sl_initial, tp_initial, probability, probability_source,
                expected_net, est_spread_cost, est_commission, slippage_cost, probability_raw, brain)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(e.ticket)
        .bind(&e.decision_id)
        .bind(&e.cid)
        .bind(&e.strategy_id)
        .bind(&e.symbol)
        .bind(&e.timeframe)
        .bind(e.side.map(Side::as_str).unwrap_or("buy"))
        .bind(&e.regime)
        .bind(&e.terminal)
        .bind(e.opened_ms)
        .bind(e.volume)
        .bind(e.entry_requested)
        .bind(e.entry_filled)
        .bind(e.sl_initial)
        .bind(e.tp_initial)
        .bind(e.probability)
        .bind(&e.probability_source)
        .bind(e.expected_net)
        .bind(e.est_spread_cost)
        .bind(e.est_commission)
        .bind(e.slippage_cost)
        .bind(e.probability_raw)
        .bind(e.brain)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Add a closing deal to the position's result. Returns false if the ticket is not journaled.
    #[allow(clippy::too_many_arguments)]
    pub async fn journal_close(&self, ticket: i64, ts_ms: TimestampMs, price: f64, gross: f64, commission: f64, swap: f64, fee: f64, reason: &str) -> DataResult<bool> {
        let n = sqlx::query(
            "UPDATE trade_journal SET closed_ms = ?, exit_price = ?, gross = gross + ?, commission = commission + ?,
                swap = swap + ?, fee = fee + ?, net = net + ? + ? + ? + ?, exit_reason = ?
             WHERE ticket = ?",
        )
        .bind(ts_ms)
        .bind(price)
        .bind(gross)
        .bind(commission)
        .bind(swap)
        .bind(fee)
        .bind(gross)
        .bind(commission)
        .bind(swap)
        .bind(fee)
        .bind(reason)
        .bind(ticket)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    pub async fn journal_excursion(&self, ticket: i64, mfe_price: f64, mae_price: f64) -> DataResult<()> {
        sqlx::query("UPDATE trade_journal SET mfe_price = MAX(mfe_price, ?), mae_price = MAX(mae_price, ?) WHERE ticket = ?")
            .bind(mfe_price)
            .bind(mae_price)
            .bind(ticket)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Closed trades since `since_ms`, oldest first.
    pub async fn journal_closed_since(&self, since_ms: TimestampMs, limit: i64) -> DataResult<Vec<JournalEntry>> {
        let rows = sqlx::query("SELECT * FROM trade_journal WHERE closed_ms IS NOT NULL AND closed_ms >= ? ORDER BY closed_ms ASC LIMIT ?")
            .bind(since_ms)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(journal_from_row).collect()
    }

    pub async fn journal_recent(&self, limit: i64) -> DataResult<Vec<JournalEntry>> {
        let rows = sqlx::query("SELECT * FROM trade_journal ORDER BY opened_ms DESC LIMIT ?").bind(limit).fetch_all(&self.pool).await?;
        rows.iter().map(journal_from_row).collect()
    }

    pub async fn journal_get(&self, ticket: i64) -> DataResult<Option<JournalEntry>> {
        let row = sqlx::query("SELECT * FROM trade_journal WHERE ticket = ?").bind(ticket).fetch_optional(&self.pool).await?;
        row.as_ref().map(journal_from_row).transpose()
    }

    // --------------------------------------------------- probability models

    /// Store a new model version. When `active`, it replaces the previous active one for its key.
    pub async fn save_probability_model(&self, m: &ProbabilityModelRow) -> DataResult<()> {
        let mut tx = self.pool.begin().await?;
        if m.active {
            sqlx::query("UPDATE probability_models SET active = 0 WHERE symbol = ? AND timeframe = ?")
                .bind(&m.symbol)
                .bind(&m.timeframe)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "INSERT INTO probability_models (model_id, created_ms, symbol, timeframe, origin, dataset_hash, buy_selected, sell_selected, active, model)
             VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&m.model_id)
        .bind(m.created_ms)
        .bind(&m.symbol)
        .bind(&m.timeframe)
        .bind(&m.origin)
        .bind(&m.dataset_hash)
        .bind(&m.buy_selected)
        .bind(&m.sell_selected)
        .bind(m.active)
        .bind(&m.model)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Make one stored version the active model of its key (used for rollback).
    pub async fn activate_probability_model(&self, model_id: &str) -> DataResult<Option<ProbabilityModelRow>> {
        let Some(row) = self.probability_models(None).await?.into_iter().find(|m| m.model_id == model_id) else { return Ok(None) };
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE probability_models SET active = 0 WHERE symbol = ? AND timeframe = ?").bind(&row.symbol).bind(&row.timeframe).execute(&mut *tx).await?;
        sqlx::query("UPDATE probability_models SET active = 1 WHERE model_id = ?").bind(model_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(ProbabilityModelRow { active: true, ..row }))
    }

    /// All versions, newest first; `Some(true)` = active ones only.
    pub async fn probability_models(&self, active: Option<bool>) -> DataResult<Vec<ProbabilityModelRow>> {
        let rows = sqlx::query("SELECT * FROM probability_models ORDER BY created_ms DESC").fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| ProbabilityModelRow {
                model_id: r.get("model_id"),
                created_ms: r.get("created_ms"),
                symbol: r.get("symbol"),
                timeframe: r.get("timeframe"),
                origin: r.get("origin"),
                dataset_hash: r.get("dataset_hash"),
                buy_selected: r.get("buy_selected"),
                sell_selected: r.get("sell_selected"),
                active: r.get("active"),
                model: r.get("model"),
            })
            .filter(|m| active.is_none_or(|a| m.active == a))
            .collect())
    }

    // ------------------------------------------------------------- datasets

    pub async fn register_dataset(&self, d: &DatasetRow) -> DataResult<()> {
        sqlx::query("INSERT OR IGNORE INTO datasets (dataset_hash, created_ms, symbol, timeframe, origin, bars, from_ts, to_ts) VALUES (?,?,?,?,?,?,?,?)")
            .bind(&d.dataset_hash)
            .bind(d.created_ms)
            .bind(&d.symbol)
            .bind(&d.timeframe)
            .bind(&d.origin)
            .bind(d.bars)
            .bind(d.from_ts)
            .bind(d.to_ts)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn datasets(&self) -> DataResult<Vec<DatasetRow>> {
        let rows = sqlx::query("SELECT * FROM datasets ORDER BY created_ms DESC").fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| DatasetRow {
                dataset_hash: r.get("dataset_hash"),
                created_ms: r.get("created_ms"),
                symbol: r.get("symbol"),
                timeframe: r.get("timeframe"),
                origin: r.get("origin"),
                bars: r.get("bars"),
                from_ts: r.get("from_ts"),
                to_ts: r.get("to_ts"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ticket: i64) -> JournalEntry {
        JournalEntry { ticket, symbol: "EURUSD".into(), side: Some(Side::Buy), opened_ms: 1_000, volume: 0.1, probability: Some(0.8), ..Default::default() }
    }

    #[tokio::test]
    async fn journal_accumulates_partial_closes_and_is_idempotent() {
        let s = Store::open_in_memory().await.unwrap();
        s.journal_open(&entry(7)).await.unwrap();
        s.journal_open(&JournalEntry { volume: 9.0, ..entry(7) }).await.unwrap(); // replay ignored
        assert!(s.journal_close(7, 2_000, 1.1, 0.30, -0.07, 0.0, -0.01, "tp").await.unwrap());
        assert!(s.journal_close(7, 3_000, 1.2, 0.20, -0.07, 0.0, 0.0, "tp").await.unwrap());
        assert!(!s.journal_close(99, 3_000, 1.2, 1.0, 0.0, 0.0, 0.0, "x").await.unwrap());
        s.journal_excursion(7, 0.0010, 0.0004).await.unwrap();
        s.journal_excursion(7, 0.0005, 0.0006).await.unwrap();
        let e = s.journal_get(7).await.unwrap().unwrap();
        assert_eq!(e.volume, 0.1);
        assert!((e.gross - 0.50).abs() < 1e-12 && (e.net - 0.35).abs() < 1e-12, "{e:?}");
        assert_eq!((e.mfe_price, e.mae_price), (0.0010, 0.0006));
        assert_eq!(s.journal_closed_since(0, 10).await.unwrap().len(), 1);
        assert_eq!(s.journal_closed_since(5_000, 10).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn one_active_probability_model_per_key_with_rollback() {
        let s = Store::open_in_memory().await.unwrap();
        let row = |id: &str, t: i64| ProbabilityModelRow {
            model_id: id.into(),
            created_ms: t,
            symbol: "EURUSD".into(),
            timeframe: "M5".into(),
            origin: "broker".into(),
            dataset_hash: "h".into(),
            buy_selected: "full".into(),
            sell_selected: "none".into(),
            active: true,
            model: "{}".into(),
        };
        s.save_probability_model(&row("a", 1)).await.unwrap();
        s.save_probability_model(&row("b", 2)).await.unwrap();
        let active = s.probability_models(Some(true)).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].model_id, "b");
        assert!(s.activate_probability_model("a").await.unwrap().is_some());
        assert_eq!(s.probability_models(Some(true)).await.unwrap()[0].model_id, "a");
        assert!(s.activate_probability_model("zz").await.unwrap().is_none());
        assert_eq!(s.probability_models(None).await.unwrap().len(), 2);
        let d = DatasetRow { dataset_hash: "h".into(), created_ms: 1, symbol: "EURUSD".into(), timeframe: "M5".into(), origin: "broker".into(), bars: 10, from_ts: 0, to_ts: 9 };
        s.register_dataset(&d).await.unwrap();
        s.register_dataset(&d).await.unwrap();
        assert_eq!(s.datasets().await.unwrap(), vec![d]);
    }
}
