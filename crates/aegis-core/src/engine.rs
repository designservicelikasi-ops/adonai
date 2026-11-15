//! Engine state, start-up, event loop and handling of terminal messages.

use crate::bridge::{self, BridgeConfig, BridgeEvent};
use crate::command::{Command, Internal, Reply};
use crate::config::{resolve_shared_key, EngineConfig};
use crate::view::*;
use aegis_data::{AuditEntry, BacktestRunRow, CommandRecord, ModelRow, OrderRecord, Store, StrategyRow};
use aegis_execution::{ExecEvent, Executor};
use aegis_learning::{ModelArtifact, ModelMonitor};
use aegis_market::MarketEngine;
use aegis_protocol::{Body, HelloInfo};
use aegis_risk::{drawdown_pct, floating_pnl, DayStats, KillSwitchState, RiskConfig, RiskEngine};
use aegis_strategy::{default_strategies, wilson_lower_bound, Strategy, StrategyStage};
use aegis_types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};

pub(crate) struct StrategySlot {
    pub strategy: Box<dyn Strategy>,
    pub stage: StrategyStage,
    pub enabled: bool,
    pub signals: u64,
}

pub(crate) struct Conn {
    pub id: u64,
    pub info: HelloInfo,
    pub peer: String,
    pub connected_ms: TimestampMs,
    pub last_rx_ms: TimestampMs,
    pub frames_in: u64,
    pub frames_out: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Accounting {
    pub login: i64,
    pub day: DayStats,
    pub peak_equity: f64,
}

pub(crate) struct LoadedModel {
    pub artifact: ModelArtifact,
    pub path: String,
    pub gate: aegis_learning::GateResult,
    pub monitor: ModelMonitor,
}

#[derive(Default)]
pub(crate) struct Cache {
    pub orders: Vec<OrderRecord>,
    pub commands: Vec<CommandRecord>,
    pub audit: Vec<AuditEntry>,
    pub backtests: Vec<BacktestRunRow>,
    pub equity_history: Vec<(TimestampMs, f64)>,
    pub candles: Vec<(String, String, String, i64)>,
    pub refreshed_ms: TimestampMs,
    pub dirty: bool,
}

pub(crate) struct Engine {
    pub cfg: EngineConfig,
    pub store: Store,
    pub risk: RiskEngine,
    pub market: MarketEngine,
    pub executor: Executor,
    pub strategies: Vec<StrategySlot>,
    pub mode: Mode,
    pub live_armed: bool,
    pub conn: Option<Conn>,
    pub bridge_stats: BridgeStats,
    pub account: Option<AccountSnapshot>,
    pub specs: HashMap<String, SymbolSpec>,
    pub positions: Vec<Position>,
    pub pending: Vec<PendingOrder>,
    pub snap: (i64, Vec<Position>, Vec<PendingOrder>),
    pub accounting: Option<Accounting>,
    pub decisions: VecDeque<DecisionView>,
    pub deals: VecDeque<Deal>,
    pub balance_ops: HashSet<i64>,
    /// Broker ticket -> strategy that opened it.
    pub owners: HashMap<i64, String>,
    /// Entry instructions awaiting their outcome: cid -> symbol.
    pub inflight: HashMap<String, String>,
    pub evidence: HashMap<String, EvidenceView>,
    pub forward: HashMap<String, ForwardStats>,
    pub model: Option<LoadedModel>,
    pub model_error: Option<String>,
    pub last_backtest: Option<BacktestDetail>,
    pub history_buf: HashMap<(String, Timeframe), Vec<Candle>>,
    pub logs: VecDeque<LogLine>,
    pub cache: Cache,
    pub db_ok: bool,
    pub audit_verified: Option<u64>,
    pub audit_error: Option<String>,
    pub last_backup: Option<String>,
    pub kill_plan_pending: bool,
    pub started_ms: TimestampMs,
    pub last_snapshot_persist_ms: TimestampMs,
    pub last_db_ping_ms: TimestampMs,
    pub last_retention_ms: TimestampMs,
    pub out_tx: mpsc::Sender<Body>,
    pub internal_tx: mpsc::Sender<Internal>,
    pub view_tx: watch::Sender<Arc<EngineView>>,
}

/// Handle owned by the host application (desktop app, daemon, tests).
pub struct EngineHandle {
    pub view: watch::Receiver<Arc<EngineView>>,
    pub bridge_addr: SocketAddr,
    commands: mpsc::Sender<(Command, oneshot::Sender<Reply>)>,
    task: tokio::task::JoinHandle<()>,
}

impl EngineHandle {
    /// Send a command and wait for its outcome.
    pub async fn command(&self, c: Command) -> Reply {
        let (tx, rx) = oneshot::channel();
        self.commands.send((c, tx)).await.map_err(|_| "le moteur est arrêté".to_string())?;
        rx.await.map_err(|_| "le moteur n'a pas répondu".to_string())?
    }

    /// Non-blocking variant for a UI thread: poll the returned receiver.
    pub fn send(&self, c: Command) -> Result<oneshot::Receiver<Reply>, String> {
        let (tx, rx) = oneshot::channel();
        self.commands.try_send((c, tx)).map_err(|_| "le moteur est occupé ou arrêté".to_string())?;
        Ok(rx)
    }

    pub fn snapshot(&self) -> Arc<EngineView> {
        self.view.borrow().clone()
    }

    pub async fn shutdown(self) {
        let _ = self.command(Command::Shutdown).await;
        let _ = self.task.await;
    }
}

pub(crate) fn evidence_key(strategy: &str, symbol: &str, tf: &str) -> String {
    format!("{strategy}|{symbol}|{tf}")
}

pub(crate) fn evidence_from_run(run: &BacktestRunRow) -> Option<EvidenceView> {
    let seg = run.metrics.get("segments")?.as_array()?.iter().find(|s| s["segment"] == "oos")?;
    let m = &seg["metrics"];
    let trades = m["trades"].as_u64()? as u32;
    let wins = m["wins"].as_u64()? as u32;
    Some(EvidenceView {
        strategy_id: run.strategy_id.clone(),
        symbol: run.symbol.clone(),
        timeframe: run.timeframe.clone(),
        run_id: run.run_id.clone(),
        origin: run.origin,
        trades,
        wins,
        net: m["net_profit"].as_f64()?,
        win_probability: if trades >= 30 { wilson_lower_bound(wins, trades, 1.0) } else { None },
    })
}

/// Start the engine: open storage, restore durable state, listen for the terminal.
pub async fn start(cfg: EngineConfig) -> anyhow::Result<EngineHandle> {
    cfg.validate()?;
    std::fs::create_dir_all(&cfg.engine.data_dir)?;
    let store = Store::open(&cfg.db_path()).await?;
    let (psk, key_source) = resolve_shared_key(&cfg)?;
    let now = now_ms();

    // Risk limits: the version saved from the application wins over the file.
    let risk_cfg: RiskConfig = match store.latest_risk_config().await? {
        Some(v) => match serde_json::from_value::<RiskConfig>(v) {
            Ok(c) if c.validate().is_ok() => c,
            _ => cfg.risk.clone(),
        },
        None => {
            store.save_risk_config("config-file", &serde_json::to_value(&cfg.risk)?).await?;
            cfg.risk.clone()
        }
    };
    let mut risk = RiskEngine::new(risk_cfg)?;
    if let Some(k) = store.kv_get::<KillSwitchState>("kill_switch").await? {
        risk.restore_kill_state(k);
    }

    let mut executor = Executor::new(store.clone(), cfg.execution.clone());
    let recovered = executor.recover(now).await?;

    let mut strategies = Vec::new();
    for s in default_strategies() {
        let m = s.meta();
        store
            .upsert_strategy(&StrategyRow { strategy_id: m.id.clone(), name: m.name.clone(), stage: "research".into(), enabled: true, config: s.config() })
            .await?;
        strategies.push(StrategySlot { strategy: s, stage: StrategyStage::Research, enabled: true, signals: 0 });
    }
    for row in store.load_strategies().await? {
        if let Some(slot) = strategies.iter_mut().find(|s| s.strategy.meta().id == row.strategy_id) {
            slot.stage = StrategyStage::parse(&row.stage).unwrap_or(StrategyStage::Research);
            slot.enabled = row.enabled;
        }
    }

    let mut market = MarketEngine::new(cfg.market.clone());
    let mut specs = HashMap::new();
    for s in store.load_symbols().await? {
        market.register_symbol(&s.symbol, s.point);
        specs.insert(s.symbol.clone(), s);
    }

    let mut evidence = HashMap::new();
    let mut runs = store.list_backtests(1_000).await?;
    runs.reverse(); // oldest first, so the newest run of each key wins
    for run in &runs {
        if let Some(ev) = evidence_from_run(run) {
            evidence.insert(evidence_key(&run.strategy_id, &run.symbol, &run.timeframe), ev);
        }
    }

    let owners: HashMap<i64, String> = store.ticket_strategies().await?.into_iter().collect();
    let accounting = store.kv_get::<Accounting>("accounting").await?;

    let listener = TcpListener::bind(&cfg.bridge.listen).await.map_err(|e| anyhow::anyhow!("cannot listen on {}: {e}", cfg.bridge.listen))?;
    let bridge_addr = listener.local_addr()?;
    let (ev_tx, ev_rx) = mpsc::channel(4_096);
    let (out_tx, out_rx) = mpsc::channel(256);
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let (internal_tx, internal_rx) = mpsc::channel(16);
    tokio::spawn(bridge::serve(
        listener,
        BridgeConfig { psk: Arc::new(psk), max_skew_ms: cfg.bridge.max_skew_ms, peer_timeout_ms: cfg.bridge.peer_timeout_ms },
        ev_tx,
        out_rx,
    ));

    let (view_tx, view_rx) = watch::channel(Arc::new(empty_view(&cfg, now)));
    let mut engine = Engine {
        risk,
        market,
        executor,
        strategies,
        mode: Mode::Monitor, // always: automation is never restored across restarts
        live_armed: false,
        conn: None,
        bridge_stats: BridgeStats { listening: bridge_addr.to_string(), key_source, ..Default::default() },
        account: None,
        specs,
        positions: Vec::new(),
        pending: Vec::new(),
        snap: (0, Vec::new(), Vec::new()),
        accounting,
        decisions: VecDeque::new(),
        deals: VecDeque::new(),
        balance_ops: HashSet::new(),
        owners,
        inflight: HashMap::new(),
        evidence,
        forward: HashMap::new(),
        model: None,
        model_error: None,
        last_backtest: None,
        history_buf: HashMap::new(),
        logs: VecDeque::new(),
        cache: Cache { dirty: true, ..Default::default() },
        db_ok: true,
        audit_verified: None,
        audit_error: None,
        last_backup: None,
        kill_plan_pending: false,
        started_ms: now,
        last_snapshot_persist_ms: 0,
        last_db_ping_ms: now,
        last_retention_ms: 0,
        out_tx,
        internal_tx,
        view_tx,
        store,
        cfg,
    };
    for d in engine.store.deals_since(now - 30 * 86_400_000, 200).await?.into_iter().rev() {
        engine.deals.push_back(d);
    }
    if !engine.cfg.learning.model_path.is_empty() {
        let path = std::path::PathBuf::from(engine.cfg.learning.model_path.clone());
        if let Err(e) = engine.load_model(&path).await {
            engine.model_error = Some(e.clone());
            engine.log("warn", "learning", format!("modèle non chargé: {e}"));
        }
    }
    engine.audit("engine", "engine_start", json!({ "version": env!("CARGO_PKG_VERSION"), "recovered_orders": recovered, "mode": "monitor" })).await;
    engine.log("info", "core", format!("moteur démarré en mode MONITOR, écoute sur {bridge_addr}"));
    if recovered > 0 {
        engine.log("warn", "execution", format!("{recovered} ordre(s) au sort inconnu après redémarrage: en attente de réconciliation"));
    }
    if engine.risk.kill_state().active {
        engine.log("warn", "risk", "arrêt d'urgence toujours actif (restauré depuis la base)".into());
    }
    engine.publish().await;
    let task = tokio::spawn(engine.run(ev_rx, cmd_rx, internal_rx));
    Ok(EngineHandle { view: view_rx, bridge_addr, commands: cmd_tx, task })
}

fn empty_view(cfg: &EngineConfig, now: TimestampMs) -> EngineView {
    EngineView {
        now_ms: now,
        started_ms: now,
        version: env!("CARGO_PKG_VERSION").into(),
        mode: Mode::Monitor,
        live_armed: false,
        live_trading_enabled_in_config: cfg.safety.live_trading_enabled,
        connection: None,
        bridge: BridgeStats::default(),
        account: None,
        specs: Vec::new(),
        markets: Vec::new(),
        signal_timeframes: cfg.engine.signal_timeframes.clone(),
        positions: Vec::new(),
        foreign_tickets: Vec::new(),
        pending: Vec::new(),
        deals: Vec::new(),
        orders: Vec::new(),
        commands: Vec::new(),
        decisions: Vec::new(),
        strategies: Vec::new(),
        risk_config: cfg.risk.clone(),
        kill: KillSwitchState::default(),
        day: None,
        peak_equity: 0.0,
        drawdown_pct: None,
        floating_pnl: None,
        daily_net: None,
        daily_net_pct: None,
        daily_halt: None,
        model: None,
        model_error: None,
        backtests: Vec::new(),
        last_backtest: None,
        equity_history: Vec::new(),
        health: Vec::new(),
        logs: Vec::new(),
        audit: Vec::new(),
        data: DataStats { db_path: cfg.db_path().display().to_string(), db_ok: true, audit_entries_verified: None, audit_error: None, last_backup: None, candles: Vec::new() },
    }
}

impl Engine {
    async fn run(
        mut self,
        mut events: mpsc::Receiver<BridgeEvent>,
        mut commands: mpsc::Receiver<(Command, oneshot::Sender<Reply>)>,
        mut internal: mpsc::Receiver<Internal>,
    ) {
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(ev) => self.on_bridge(ev).await,
                    None => break,
                },
                cmd = commands.recv() => match cmd {
                    Some((Command::Shutdown, reply)) => {
                        self.audit("engine", "engine_stop", json!({})).await;
                        let _ = reply.send(Ok("arrêt du moteur".into()));
                        break;
                    }
                    Some((c, reply)) => self.on_command(c, reply).await,
                    None => break,
                },
                done = internal.recv() => if let Some(done) = done {
                    self.on_internal(done).await;
                },
                _ = timer.tick() => {
                    self.on_timer().await;
                    self.publish().await;
                }
            }
        }
        self.store.close().await;
    }

    // ------------------------------------------------------------ utilities

    pub(crate) fn log(&mut self, level: &str, source: &str, text: String) {
        match level {
            "error" => tracing::error!(source, "{text}"),
            "warn" => tracing::warn!(source, "{text}"),
            _ => tracing::info!(source, "{text}"),
        }
        self.logs.push_back(LogLine { ts_ms: now_ms(), level: level.into(), source: source.into(), text });
        while self.logs.len() > 400 {
            self.logs.pop_front();
        }
    }

    /// Append to the audit trail. A failure marks the database as down (fail closed).
    pub(crate) async fn audit(&mut self, actor: &str, event: &str, detail: serde_json::Value) {
        if let Err(e) = self.store.audit(actor, event, &detail).await {
            self.db_failed(&format!("audit `{event}`: {e}"));
        }
        self.cache.dirty = true;
    }

    pub(crate) fn db_failed(&mut self, what: &str) {
        if self.db_ok {
            self.db_ok = false;
            self.log("error", "data", format!("base de données indisponible ({what}): nouvelles entrées bloquées"));
        }
    }

    pub(crate) fn terminal(&self) -> Option<TerminalKind> {
        self.conn.as_ref().map(|c| c.info.terminal)
    }

    pub(crate) fn origin(&self) -> DataOrigin {
        if self.terminal() == Some(TerminalKind::Simulator) { DataOrigin::Synthetic } else { DataOrigin::Broker }
    }

    pub(crate) fn symbol_allowed(&self, symbol: &str) -> bool {
        self.cfg.engine.symbols.is_empty()
            || self.cfg.engine.symbols.iter().any(|s| s == symbol)
            || self.terminal() == Some(TerminalKind::Simulator)
    }

    /// The single exit towards the terminal. Nothing is ever sent in MONITOR mode.
    pub(crate) fn send_order(&mut self, body: Body) -> Result<(), String> {
        if !self.mode.can_execute() {
            return Err("mode MONITOR: aucun ordre n'est envoyé".into());
        }
        self.send_raw(body)
    }

    fn send_raw(&mut self, body: Body) -> Result<(), String> {
        let Some(conn) = self.conn.as_mut() else { return Err("terminal déconnecté".into()) };
        self.out_tx.try_send(body).map_err(|_| "file d'envoi saturée".to_string())?;
        conn.frames_out += 1;
        Ok(())
    }

    pub(crate) async fn set_mode(&mut self, mode: Mode, why: &str) {
        if self.mode != mode {
            let from = self.mode;
            self.mode = mode;
            if mode != Mode::LiveAuto {
                self.live_armed = false;
            }
            self.audit("engine", "mode_change", json!({ "from": from.label(), "to": mode.label(), "reason": why })).await;
            self.log("warn", "core", format!("mode {} -> {} ({why})", from.label(), mode.label()));
        }
    }

    // --------------------------------------------------------------- bridge

    async fn on_bridge(&mut self, ev: BridgeEvent) {
        match ev {
            BridgeEvent::Refused { peer, reason } => {
                self.bridge_stats.handshakes_refused += 1;
                self.bridge_stats.last_refusal = format!("{peer}: {reason}");
                self.audit("bridge", "connection_refused", json!({ "peer": peer, "reason": reason })).await;
                self.log("warn", "bridge", format!("connexion refusée ({peer}): {reason}"));
            }
            BridgeEvent::Connected { conn, info, peer } => {
                let now = now_ms();
                self.bridge_stats.connections_accepted += 1;
                let sim = info.terminal == TerminalKind::Simulator;
                self.market.set_clock_check(!sim, self.cfg.market.max_clock_offset_ms);
                // A different account than before: automation is switched off.
                if self.accounting.as_ref().is_some_and(|a| a.login != info.login) {
                    self.set_mode(Mode::Monitor, "compte différent du précédent").await;
                }
                self.audit("bridge", "terminal_connected", json!({ "peer": peer, "terminal": info.terminal, "login": info.login, "server": info.server, "trade_mode": info.trade_mode.as_str() })).await;
                self.log("info", "bridge", format!(
                    "{} connecté: compte {} sur {} ({})",
                    if sim { "SIMULATEUR (adaptateur de test)" } else { "terminal MT5" },
                    info.login, info.server, info.trade_mode.as_str()
                ));
                self.conn = Some(Conn { id: conn, info, peer, connected_ms: now, last_rx_ms: now, frames_in: 0, frames_out: 0 });
                self.snap = (0, Vec::new(), Vec::new());
                self.load_forward().await;
            }
            BridgeEvent::Disconnected { conn, reason, violation } => {
                if self.conn.as_ref().is_some_and(|c| c.id == conn) {
                    self.conn = None;
                    if violation {
                        self.bridge_stats.protocol_violations += 1;
                    }
                    self.bridge_stats.last_disconnect = reason.clone();
                    self.audit("bridge", "terminal_disconnected", json!({ "reason": reason, "violation": violation })).await;
                    self.log(if violation { "error" } else { "warn" }, "bridge", format!("terminal déconnecté: {reason}"));
                    if self.mode == Mode::LiveAuto {
                        self.set_mode(Mode::Monitor, "déconnexion du terminal").await;
                    }
                }
            }
            BridgeEvent::Frame { conn, body } => {
                let Some(c) = self.conn.as_mut().filter(|c| c.id == conn) else { return };
                c.frames_in += 1;
                c.last_rx_ms = now_ms();
                self.on_frame(body).await;
            }
        }
    }

    async fn on_frame(&mut self, body: Body) {
        let now = now_ms();
        match body {
            Body::Spec { .. } => {
                let Some(spec) = body.to_symbol_spec() else { return };
                if spec.validate().is_err() || !self.symbol_allowed(&spec.symbol) {
                    return;
                }
                if let Err(e) = self.store.upsert_symbol(&spec).await {
                    self.db_failed(&e.to_string());
                }
                let first = !self.market.symbols().contains(&spec.symbol);
                self.market.register_symbol(&spec.symbol, spec.point);
                let tfs = self.market.config().timeframes.clone();
                for tf in tfs {
                    let _ = self.send_raw(Body::HistoryReq { symbol: spec.symbol.clone(), tf: tf.as_str().into(), count: self.cfg.engine.history_bars });
                }
                if first {
                    self.log("info", "market", format!("instrument {} enregistré", spec.symbol));
                }
                self.specs.insert(spec.symbol.clone(), spec);
            }
            Body::Tick { symbol, time, bid, ask } => {
                let closed = self.market.on_tick(&symbol, Tick { ts_ms: time, bid, ask }, now);
                for bar in closed {
                    self.on_closed_bar(bar).await;
                }
            }
            Body::Account { .. } => {
                if let Some(a) = body.to_account(now) {
                    self.on_account(a).await;
                }
            }
            Body::Pos { snap, .. } => {
                if snap != self.snap.0 {
                    self.snap = (snap, Vec::new(), Vec::new());
                }
                if let Some(p) = body.to_position() {
                    self.snap.1.push(p);
                }
            }
            Body::Ord { snap, .. } => {
                if snap != self.snap.0 {
                    self.snap = (snap, Vec::new(), Vec::new());
                }
                if let Some(o) = body.to_pending() {
                    self.snap.2.push(o);
                }
            }
            Body::SnapEnd { snap, positions, orders } => {
                if snap != self.snap.0 {
                    self.snap = (snap, Vec::new(), Vec::new());
                }
                if self.snap.1.len() != positions as usize || self.snap.2.len() != orders as usize {
                    self.log("warn", "execution", "instantané des positions incomplet: ignoré".into());
                    self.snap = (0, Vec::new(), Vec::new());
                    return;
                }
                let (_, pos, ord) = std::mem::replace(&mut self.snap, (0, Vec::new(), Vec::new()));
                self.on_snapshot(pos, ord, now).await;
            }
            Body::Deal { .. } => {
                if let Some(d) = body.to_deal() {
                    self.on_deal(d).await;
                }
            }
            Body::BalanceOp { deal, amount, .. } => {
                if self.balance_ops.insert(deal) {
                    if let Some(acc) = self.accounting.as_mut() {
                        acc.day.on_balance_operation(amount);
                        acc.peak_equity += amount;
                    }
                    self.persist_accounting().await;
                    self.audit("broker", "balance_operation", json!({ "deal": deal, "amount": amount })).await;
                    self.log("info", "account", format!("opération de solde: {amount:+.2}"));
                }
            }
            Body::Result { cid, ok, retcode, ticket, price, volume, msg } => {
                match self.executor.on_result(&cid, ok, retcode, ticket, price, volume, &msg).await {
                    Ok(ev) => self.on_exec_event(ev).await,
                    Err(e) => self.db_failed(&e.to_string()),
                }
            }
            Body::Bar { .. } => {
                if let Some((symbol, tf, candle, last)) = body.to_candle() {
                    if self.market.on_history_bar(&symbol, tf, candle) {
                        let buf = self.history_buf.entry((symbol.clone(), tf)).or_default();
                        buf.push(candle);
                        if last || buf.len() >= 250 {
                            self.flush_history(&symbol, tf).await;
                        }
                    }
                }
            }
            Body::Error { code, msg } => self.log("warn", "bridge", format!("le terminal signale une erreur {code}: {msg}")),
            _ => {}
        }
    }

    async fn flush_history(&mut self, symbol: &str, tf: Timeframe) {
        let Some(mut bars) = self.history_buf.remove(&(symbol.to_string(), tf)) else { return };
        bars.sort_by_key(|c| c.open_ts);
        bars.dedup_by_key(|c| c.open_ts);
        let origin = self.origin();
        match self.store.insert_candles(symbol, tf, origin, &bars).await {
            Ok(_) => self.log("info", "market", format!("{} bougies {symbol} {tf} reçues ({})", bars.len(), origin.as_str())),
            Err(e) => self.log("warn", "data", format!("historique {symbol} {tf} non enregistré: {e}")),
        }
        self.cache.dirty = true;
    }

    async fn on_account(&mut self, a: AccountSnapshot) {
        if !a.is_sane() {
            self.log("error", "account", "informations de compte incohérentes reçues: ignorées".into());
            return;
        }
        if self.conn.as_ref().is_some_and(|c| c.info.login != a.login) {
            self.log("error", "account", "le compte annoncé ne correspond pas à l'identification: ignoré".into());
            self.set_mode(Mode::Monitor, "identité du compte incohérente").await;
            return;
        }
        let mut changed = false;
        match self.accounting.as_mut() {
            Some(acc) if acc.login == a.login => {
                if acc.day.roll_if_new_day(a.ts_ms, a.equity, a.balance) {
                    changed = true;
                }
                if a.equity > acc.peak_equity {
                    acc.peak_equity = a.equity;
                    changed = true;
                }
            }
            _ => {
                self.accounting = Some(Accounting { login: a.login, day: DayStats::new(a.ts_ms, a.equity, a.balance), peak_equity: a.equity });
                changed = true;
            }
        }
        if changed {
            self.persist_accounting().await;
        }
        if a.ts_ms - self.last_snapshot_persist_ms >= 10_000 {
            self.last_snapshot_persist_ms = a.ts_ms;
            if let Err(e) = self.store.insert_account_snapshot(&a).await {
                self.db_failed(&e.to_string());
            }
        }
        // Automatic emergency stop on maximum drawdown.
        let peak = self.accounting.as_ref().map(|x| x.peak_equity).unwrap_or(a.equity);
        let dd = drawdown_pct(peak, a.equity);
        let max_dd = self.risk.config().max_drawdown_pct;
        self.account = Some(a);
        if dd >= max_dd && !self.risk.kill_state().active {
            self.engage_kill(&format!("drawdown maximal atteint: {dd:.2} % (limite {max_dd:.2} %)"), "system").await;
        }
    }

    pub(crate) async fn persist_accounting(&mut self) {
        if let Some(acc) = &self.accounting {
            if let Err(e) = self.store.kv_set("accounting", acc).await {
                self.db_failed(&e.to_string());
            }
        }
    }

    async fn on_snapshot(&mut self, positions: Vec<Position>, pending: Vec<PendingOrder>, now: TimestampMs) {
        let closed: Vec<i64> = self.positions.iter().map(|p| p.ticket).filter(|t| !positions.iter().any(|p| p.ticket == *t)).collect();
        let opened: Vec<i64> = positions.iter().map(|p| p.ticket).filter(|t| !self.positions.iter().any(|p| p.ticket == *t)).collect();
        if let Err(e) = self.store.sync_positions(&positions).await {
            self.db_failed(&e.to_string());
        }
        match self.executor.reconcile(&positions, &pending, now).await {
            Ok(events) => {
                for ev in events {
                    self.on_exec_event(ev).await;
                }
            }
            Err(e) => self.db_failed(&e.to_string()),
        }
        let magic = self.cfg.execution.magic;
        for t in &opened {
            if let Some(p) = positions.iter().find(|p| p.ticket == *t) {
                if p.magic != magic {
                    self.log("warn", "execution", format!("position {} ouverte hors d'AEGIS sur {} (non gérée)", p.ticket, p.symbol));
                }
            }
        }
        if !closed.is_empty() || !opened.is_empty() {
            self.cache.dirty = true;
        }
        self.positions = positions;
        self.pending = pending;
        if self.kill_plan_pending && self.risk.kill_state().active {
            self.apply_kill_plan().await;
        }
    }

    async fn on_deal(&mut self, d: Deal) {
        match self.store.insert_deal(&d).await {
            Ok(true) => {}
            Ok(false) => return, // already recorded
            Err(e) => {
                self.db_failed(&e.to_string());
                return;
            }
        }
        if let Some(acc) = self.accounting.as_mut() {
            acc.day.on_deal(&d);
        }
        self.persist_accounting().await;
        // A deal that closes a position whose opening was never confirmed proves the order ran.
        if let Some(cid) = self.executor.note_deal(&d.origin, d.position_ticket) {
            self.bind_owner(&cid, d.position_ticket).await;
            match self.executor.reconcile(&self.positions.clone(), &self.pending.clone(), now_ms()).await {
                Ok(events) => {
                    for ev in events {
                        self.on_exec_event(ev).await;
                    }
                }
                Err(e) => self.db_failed(&e.to_string()),
            }
        } else if !self.owners.contains_key(&d.position_ticket) && !d.origin.is_empty() {
            self.bind_owner(&d.origin, d.position_ticket).await;
        }
        if let Some(strategy) = self.owners.get(&d.position_ticket).cloned() {
            let kind = if self.terminal() == Some(TerminalKind::Simulator) { "simulator" } else { "mt5" };
            let key = format!("forward:{kind}:{strategy}");
            let mut f: ForwardStats = self.store.kv_get(&key).await.ok().flatten().unwrap_or_default();
            f.trades += 1;
            f.net += d.net();
            if d.net() > 0.0 {
                f.wins += 1;
            }
            let _ = self.store.kv_set(&key, &f).await;
            self.forward.insert(strategy, f);
        }
        self.audit("broker", "deal", json!({ "deal": d.deal_ticket, "position": d.position_ticket, "symbol": d.symbol, "volume": d.volume, "price": d.price, "profit": d.profit, "swap": d.swap, "commission": d.commission, "comment": d.comment, "origin": d.origin })).await;
        self.log("info", "execution", format!(
            "position {} clôturée ({}): résultat net {:+.2} (brut {:+.2}, commission {:+.2}, swap {:+.2})",
            d.position_ticket, if d.comment.is_empty() { "-" } else { &d.comment }, d.net(), d.profit, d.commission, d.swap
        ));
        self.deals.push_back(d);
        while self.deals.len() > 300 {
            self.deals.pop_front();
        }
    }

    pub(crate) async fn on_exec_event(&mut self, ev: ExecEvent) {
        let detail = serde_json::to_value(&ev).unwrap_or_default();
        match &ev {
            ExecEvent::Filled { cid, ticket, price, volume, requested_volume, slippage_points } => {
                self.inflight.remove(cid);
                self.bind_owner(cid, *ticket).await;
                let slip = slippage_points.map(|s| format!(", glissement {s:+.1} pts")).unwrap_or_default();
                let part = if *volume + 1e-9 < *requested_volume { format!(" PARTIEL ({volume}/{requested_volume})") } else { String::new() };
                self.log("info", "execution", format!("ordre {cid} exécuté{part}: ticket {ticket}, {volume} lot(s) à {price}{slip}"));
            }
            ExecEvent::Placed { cid, ticket } => {
                self.inflight.remove(cid);
                self.bind_owner(cid, *ticket).await;
                self.log("info", "execution", format!("ordre en attente {cid} accepté: ticket {ticket}"));
            }
            ExecEvent::Rejected { cid, retcode, message } => {
                self.inflight.remove(cid);
                self.log("warn", "execution", format!("ordre {cid} REJETÉ par le broker (code {retcode}): {message}"));
            }
            ExecEvent::TimedOut { cid } => {
                self.log("warn", "execution", format!("ordre {cid}: aucune confirmation — état INCONNU, réconciliation requise"));
            }
            ExecEvent::ReconciledFilled { cid, ticket } => {
                self.inflight.remove(cid);
                self.bind_owner(cid, *ticket).await;
                self.log("warn", "execution", format!("ordre {cid} retrouvé sur le compte (ticket {ticket}) lors de la réconciliation"));
            }
            ExecEvent::ReconciledNotExecuted { cid } => {
                self.inflight.remove(cid);
                self.log("warn", "execution", format!("ordre {cid}: non exécuté (confirmé par la réconciliation)"));
            }
            ExecEvent::CommandDone { cid, action, ticket } => {
                self.log("info", "execution", format!("commande {action} {cid} exécutée (ticket {ticket})"));
            }
            ExecEvent::CommandFailed { cid, action, ticket, message } => {
                self.log("warn", "execution", format!("commande {action} {cid} refusée (ticket {ticket}): {message}"));
            }
            ExecEvent::UnexpectedResult { cid } => {
                self.log("warn", "execution", format!("résultat inattendu pour {cid}: ignoré"));
            }
        }
        self.audit("execution", "order_event", detail).await;
    }

    async fn bind_owner(&mut self, cid: &str, ticket: i64) {
        if let Ok(Some(strategy)) = self.store.strategy_of_order(cid).await {
            self.owners.insert(ticket, strategy);
        }
    }

    // ---------------------------------------------------------------- timer

    async fn on_timer(&mut self) {
        let now = now_ms();
        match self.executor.check_timeouts(now).await {
            Ok(events) => {
                for ev in events {
                    self.on_exec_event(ev).await;
                }
            }
            Err(e) => self.db_failed(&e.to_string()),
        }
        if now - self.last_db_ping_ms >= 3_000 {
            self.last_db_ping_ms = now;
            match self.store.ping().await {
                Ok(()) => {
                    if !self.db_ok {
                        self.db_ok = true;
                        self.log("info", "data", "base de données de nouveau disponible".into());
                    }
                }
                Err(e) => self.db_failed(&e.to_string()),
            }
        }
        if now - self.last_retention_ms >= 86_400_000 {
            self.last_retention_ms = now;
            if let Ok((c, s)) = self.store.apply_retention(self.cfg.retention.candle_days, self.cfg.retention.snapshot_days).await {
                if c + s > 0 {
                    self.log("info", "data", format!("rétention: {c} bougies et {s} instantanés supprimés"));
                }
            }
        }
    }

    // ----------------------------------------------------------------- view

    async fn refresh_cache(&mut self, now: TimestampMs) {
        if !self.cache.dirty && now - self.cache.refreshed_ms < 5_000 {
            return;
        }
        self.cache.dirty = false;
        self.cache.refreshed_ms = now;
        if let Ok(v) = self.store.recent_orders(200).await {
            self.cache.orders = v;
        }
        if let Ok(v) = self.store.recent_commands(100).await {
            self.cache.commands = v;
        }
        if let Ok(v) = self.store.audit_tail(200).await {
            self.cache.audit = v;
        }
        if let Ok(v) = self.store.list_backtests(50).await {
            self.cache.backtests = v;
        }
        if let Ok(v) = self.store.equity_history(2_000).await {
            self.cache.equity_history = v;
        }
        let mut stats = Vec::new();
        for symbol in self.specs.keys().cloned().collect::<Vec<_>>() {
            for tf in Timeframe::ALL {
                for origin in [DataOrigin::Broker, DataOrigin::Synthetic] {
                    if let Ok(n) = self.store.candle_count(&symbol, tf, origin).await {
                        if n > 0 {
                            stats.push((symbol.clone(), tf.as_str().to_string(), origin.as_str().to_string(), n));
                        }
                    }
                }
            }
        }
        stats.sort();
        self.cache.candles = stats;
    }

    fn health(&self, now: TimestampMs) -> Vec<ModuleHealth> {
        let m = |name: &str, status, detail: String| ModuleHealth { name: name.into(), status, detail };
        let mut h = vec![m("Moteur central", HealthStatus::Ok, format!("actif depuis {} s", (now - self.started_ms) / 1000))];
        h.push(if self.db_ok {
            m("Données", HealthStatus::Ok, "base accessible".into())
        } else {
            m("Données", HealthStatus::Down, "base de données inaccessible: entrées bloquées".into())
        });
        h.push(match &self.conn {
            Some(c) if now - c.last_rx_ms > 5_000 => m("Connexion terminal", HealthStatus::Degraded, format!("silencieux depuis {} s", (now - c.last_rx_ms) / 1000)),
            Some(c) if c.info.terminal == TerminalKind::Simulator => m("Connexion terminal", HealthStatus::Ok, "SIMULATEUR connecté (adaptateur de test)".into()),
            Some(c) => m("Connexion terminal", HealthStatus::Ok, format!("MT5 connecté, compte {}", c.info.login)),
            None => m("Connexion terminal", HealthStatus::Down, format!("aucun terminal connecté (écoute sur {})", self.bridge_stats.listening)),
        });
        let symbols = self.market.symbols();
        let tradable = symbols.iter().filter(|s| self.market.quality(s, now).iter().all(|i| !i.blocks_trading())).count();
        h.push(if symbols.is_empty() || self.conn.is_none() {
            m("Marché", HealthStatus::Inactive, "aucune donnée de marché en direct".into())
        } else if tradable == symbols.len() {
            m("Marché", HealthStatus::Ok, format!("{tradable} instrument(s) avec des prix valides"))
        } else {
            m("Marché", HealthStatus::Degraded, format!("{} instrument(s) sur {} avec des données inutilisables", symbols.len() - tradable, symbols.len()))
        });
        let active = self.strategies.iter().filter(|s| s.enabled && s.stage != StrategyStage::Disabled).count();
        h.push(m("Stratégies", if active > 0 { HealthStatus::Ok } else { HealthStatus::Inactive }, format!("{active} stratégie(s) active(s)")));
        h.push(if self.risk.kill_state().active {
            m("Risque", HealthStatus::Degraded, format!("ARRÊT D'URGENCE: {}", self.risk.kill_state().reason))
        } else {
            m("Risque", HealthStatus::Ok, "limites actives".into())
        });
        h.push(m(
            "Exécution",
            if self.mode.can_execute() && self.conn.is_some() { HealthStatus::Ok } else { HealthStatus::Inactive },
            format!("mode {}; {} ordre(s) en cours", self.mode.label(), self.executor.in_flight()),
        ));
        h.push(match (&self.model, &self.model_error) {
            (Some(l), _) if l.gate.passed => m("Intelligence artificielle", HealthStatus::Ok, format!("modèle {} validé", l.artifact.model_id)),
            (Some(l), _) => m("Intelligence artificielle", HealthStatus::Degraded, format!("modèle {} NON validé: indicatif uniquement", l.artifact.model_id)),
            (None, Some(e)) => m("Intelligence artificielle", HealthStatus::Down, format!("modèle illisible: {e}")),
            (None, None) => m("Intelligence artificielle", HealthStatus::Inactive, "aucun modèle chargé".into()),
        });
        h
    }

    pub(crate) async fn publish(&mut self) {
        let now = now_ms();
        self.refresh_cache(now).await;
        let equity = self.account.as_ref().map(|a| a.equity);
        let day = self.accounting.as_ref().map(|a| a.day.clone());
        let peak = self.accounting.as_ref().map(|a| a.peak_equity).unwrap_or(0.0);
        let daily_net_pct = match (&day, equity) {
            (Some(d), Some(e)) => Some(d.net_pnl_pct(e)),
            _ => None,
        };
        let rc = self.risk.config().clone();
        let daily_halt = daily_net_pct.and_then(|p| {
            if p <= -rc.max_daily_loss_pct {
                Some(format!("perte journalière maximale atteinte ({p:.2} %)"))
            } else if rc.daily_profit_target_pct > 0.0 && p >= rc.daily_profit_target_pct {
                Some(format!("objectif journalier atteint ({p:.2} %): nouvelles entrées arrêtées"))
            } else {
                None
            }
        });
        let magic = self.cfg.execution.magic;
        let strategies = self
            .strategies
            .iter()
            .map(|s| {
                let id = s.strategy.meta().id.clone();
                let mut evidence: Vec<EvidenceView> = self.evidence.values().filter(|e| e.strategy_id == id).cloned().collect();
                evidence.sort_by(|a, b| (&a.symbol, &a.timeframe).cmp(&(&b.symbol, &b.timeframe)));
                StrategyView {
                    meta: s.strategy.meta().clone(),
                    config: s.strategy.config(),
                    stage: s.stage.as_str().into(),
                    enabled: s.enabled,
                    evidence,
                    forward: self.forward.get(&id).cloned().unwrap_or_default(),
                    signals: s.signals,
                }
            })
            .collect();
        let model = self.model.as_ref().map(|l| ModelView {
            model_id: l.artifact.model_id.clone(),
            path: l.path.clone(),
            origin: l.artifact.origin,
            symbol: l.artifact.symbol.clone(),
            timeframe: l.artifact.timeframe.clone(),
            horizon_bars: l.artifact.horizon_bars,
            target: l.artifact.target.clone(),
            validation: l.artifact.validation.clone(),
            gate: l.gate.clone(),
            last_prediction: l.monitor.last_prediction,
            performance: l.monitor.performance(),
            drift: l.monitor.drift(&l.artifact),
        });
        let mut specs: Vec<SymbolSpec> = self.specs.values().cloned().collect();
        specs.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        let view = EngineView {
            now_ms: now,
            started_ms: self.started_ms,
            version: env!("CARGO_PKG_VERSION").into(),
            mode: self.mode,
            live_armed: self.live_armed,
            live_trading_enabled_in_config: self.cfg.safety.live_trading_enabled,
            connection: self.conn.as_ref().map(|c| ConnectionView {
                terminal: c.info.terminal,
                login: c.info.login,
                server: c.info.server.clone(),
                trade_mode: c.info.trade_mode,
                build: c.info.build.clone(),
                peer: c.peer.clone(),
                connected_ms: c.connected_ms,
                last_rx_ms: c.last_rx_ms,
                frames_in: c.frames_in,
                frames_out: c.frames_out,
            }),
            bridge: self.bridge_stats.clone(),
            account: self.account.clone(),
            specs,
            markets: self.market.symbols().iter().filter_map(|s| self.market.snapshot(s, now)).collect(),
            signal_timeframes: self.cfg.engine.signal_timeframes.clone(),
            foreign_tickets: self.positions.iter().filter(|p| p.magic != magic).map(|p| p.ticket).collect(),
            positions: self.positions.clone(),
            pending: self.pending.clone(),
            deals: self.deals.iter().rev().cloned().collect(),
            orders: self.cache.orders.clone(),
            commands: self.cache.commands.clone(),
            decisions: self.decisions.iter().rev().cloned().collect(),
            strategies,
            risk_config: rc,
            kill: self.risk.kill_state().clone(),
            drawdown_pct: equity.filter(|_| peak > 0.0).map(|e| drawdown_pct(peak, e)),
            floating_pnl: self.account.as_ref().map(|_| floating_pnl(&self.positions)),
            daily_net: match (&day, equity) {
                (Some(d), Some(e)) => Some(d.net_pnl(e)),
                _ => None,
            },
            daily_net_pct,
            daily_halt,
            day,
            peak_equity: peak,
            model,
            model_error: self.model_error.clone(),
            backtests: self.cache.backtests.clone(),
            last_backtest: self.last_backtest.clone(),
            equity_history: self.cache.equity_history.clone(),
            health: self.health(now),
            logs: self.logs.iter().rev().cloned().collect(),
            audit: self.cache.audit.clone(),
            data: DataStats {
                db_path: self.cfg.db_path().display().to_string(),
                db_ok: self.db_ok,
                audit_entries_verified: self.audit_verified,
                audit_error: self.audit_error.clone(),
                last_backup: self.last_backup.clone(),
                candles: self.cache.candles.clone(),
            },
        };
        let _ = self.view_tx.send(Arc::new(view));
    }

    // ---------------------------------------------------------------- model

    pub(crate) async fn load_model(&mut self, path: &std::path::Path) -> Result<String, String> {
        let artifact = ModelArtifact::load(path).map_err(|e| e.to_string())?;
        let gate = artifact.gate(&self.cfg.learning.gate);
        let status = if gate.passed { "validated" } else { "rejected" };
        self.store
            .register_model(&ModelRow {
                model_id: artifact.model_id.clone(),
                created_ms: artifact.created_ms,
                status: status.into(),
                origin: artifact.origin,
                path: path.display().to_string(),
                metrics: serde_json::to_value(&artifact.validation).unwrap_or_default(),
            })
            .await
            .map_err(|e| e.to_string())?;
        self.audit("learning", "model_loaded", json!({ "model_id": artifact.model_id, "origin": artifact.origin.as_str(), "gate_passed": gate.passed, "failures": gate.failures })).await;
        let msg = if gate.passed {
            format!("modèle {} chargé et VALIDÉ", artifact.model_id)
        } else {
            format!("modèle {} chargé mais NON validé ({}): affiché à titre indicatif, sans effet sur l'exécution", artifact.model_id, gate.failures.join("; "))
        };
        self.log(if gate.passed { "info" } else { "warn" }, "learning", msg.clone());
        self.model = Some(LoadedModel { monitor: ModelMonitor::new(&artifact), artifact, path: path.display().to_string(), gate });
        self.model_error = None;
        Ok(msg)
    }
}
