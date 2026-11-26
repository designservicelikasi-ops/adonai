//! Simulated terminal: a network client that speaks the real AEGIS protocol
//! exactly like the MQL5 connector, backed by `SimBroker` and a SYNTHETIC price
//! feed. It always announces itself as `TerminalKind::Simulator`.
//!
//! Market time is accelerated: each tick advances the simulated broker clock by
//! `sim_ms_per_tick` while only `tick_interval_ms` of real time elapses.

use crate::broker::{Fault, SimBroker, SimBrokerConfig, SIM_SYMBOL};
use aegis_backtest::synthetic::{generate, Rng, SyntheticParams};
use aegis_market::aggregate;
use aegis_protocol::io::{write_frame, FrameReader};
use aegis_protocol::{client_finish, client_hello, Body, HelloInfo, Session};
use aegis_types::{now_ms, Candle, TerminalKind, Timeframe};
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum PriceFeed {
    /// Endless seeded random walk.
    Synthetic { seed: u64 },
    /// Fixed sequence of bid prices; the last one is repeated when exhausted.
    Script(Vec<f64>),
}

#[derive(Debug, Clone)]
pub struct SimTerminalConfig {
    pub addr: String,
    pub psk: Vec<u8>,
    pub broker: SimBrokerConfig,
    pub feed: PriceFeed,
    /// Real time between two ticks.
    pub tick_interval_ms: u64,
    /// Simulated market time between two ticks.
    pub sim_ms_per_tick: i64,
    pub spread_points: f64,
    /// M5 bars of history served on request (generated before the first tick).
    pub history: Vec<Candle>,
}

impl SimTerminalConfig {
    pub fn new(addr: &str, psk: &[u8]) -> Self {
        SimTerminalConfig {
            addr: addr.to_string(),
            psk: psk.to_vec(),
            broker: SimBrokerConfig::default(),
            feed: PriceFeed::Synthetic { seed: 7 },
            tick_interval_ms: 250,
            sim_ms_per_tick: 15_000,
            spread_points: 10.0,
            history: Vec::new(),
        }
    }

    /// Fill `history` with `bars` synthetic M5 candles ending right before `end_ts`.
    pub fn with_synthetic_history(mut self, bars: usize, end_ts: i64, seed: u64) -> Self {
        let tf = Timeframe::M5;
        let start = tf.bucket(end_ts) - bars as i64 * tf.millis();
        self.history = generate(&SyntheticParams { seed, bars, timeframe: tf, start_ts: start, ..Default::default() });
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimControl {
    /// Apply this fault to the next instruction.
    Fault(Fault),
    /// Close the connection abruptly (network failure).
    Disconnect,
    /// Stop sending ticks (stale market data) / resume.
    PauseTicks(bool),
    /// Re-send the last frame that was sent (replay attack / duplicate delivery).
    ReplayLastFrame,
    /// Remove the stop-loss of every position (stop dropped by the broker or by hand in MT5).
    StripStops,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimExit {
    Stopped,
    Disconnected(String),
    HandshakeFailed(String),
}

pub struct SimHandle {
    pub control: mpsc::Sender<SimControl>,
    pub task: tokio::task::JoinHandle<SimExit>,
}

impl SimHandle {
    pub async fn stop(self) -> SimExit {
        let _ = self.control.send(SimControl::Disconnect).await;
        self.task.await.unwrap_or(SimExit::Stopped)
    }
}

pub fn spawn(cfg: SimTerminalConfig) -> SimHandle {
    let (tx, rx) = mpsc::channel(16);
    SimHandle { control: tx, task: tokio::spawn(run(cfg, rx)) }
}

struct Feed {
    kind: PriceFeed,
    rng: Rng,
    price: f64,
    index: usize,
}

impl Feed {
    fn next(&mut self) -> f64 {
        match &self.kind {
            PriceFeed::Script(v) => {
                if let Some(p) = v.get(self.index).or(v.last()) {
                    self.price = *p;
                }
                self.index += 1;
            }
            PriceFeed::Synthetic { .. } => {
                self.price *= 1.0 + self.rng.normal() * 0.00012;
                self.price = (self.price * 100_000.0).round() / 100_000.0;
            }
        }
        self.price
    }
}

// `last_frame` is read only by the replay control; the lint cannot see that through the macro.
#[allow(unused_assignments)]
async fn run(cfg: SimTerminalConfig, mut control: mpsc::Receiver<SimControl>) -> SimExit {
    let stream = match TcpStream::connect(&cfg.addr).await {
        Ok(s) => s,
        Err(e) => return SimExit::HandshakeFailed(format!("connect: {e}")),
    };
    let _ = stream.set_nodelay(true);
    let (rd, mut wr) = stream.into_split();
    let mut rd = FrameReader::new(BufReader::new(rd));

    let info = HelloInfo {
        terminal: TerminalKind::Simulator,
        login: cfg.broker.login,
        server: cfg.broker.server.clone(),
        trade_mode: cfg.broker.trade_mode,
        build: "aegis-simterminal".into(),
    };
    let mut session: Session = {
        let (hello, nonce) = match client_hello(&cfg.psk, &info, now_ms()) {
            Ok(v) => v,
            Err(e) => return SimExit::HandshakeFailed(e.to_string()),
        };
        if let Err(e) = write_frame(&mut wr, &hello).await {
            return SimExit::HandshakeFailed(e.to_string());
        }
        let welcome = match tokio::time::timeout(Duration::from_secs(5), rd.next()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => return SimExit::HandshakeFailed("connection closed by the engine".into()),
            Ok(Err(e)) => return SimExit::HandshakeFailed(e.to_string()),
            Err(_) => return SimExit::HandshakeFailed("no welcome received".into()),
        };
        match client_finish(&cfg.psk, &welcome, &nonce, 30_000, now_ms()) {
            Ok(s) => s,
            Err(e) => return SimExit::HandshakeFailed(e.to_string()),
        }
    };

    let mut broker = SimBroker::new(cfg.broker.clone());
    let spread = cfg.spread_points * 0.00001;
    let start_price = cfg.history.last().map(|c| c.close).unwrap_or(1.1000);
    let seed = match &cfg.feed {
        PriceFeed::Synthetic { seed } => *seed,
        PriceFeed::Script(_) => 0,
    };
    let mut feed = Feed { kind: cfg.feed.clone(), rng: Rng::new(seed), price: start_price, index: 0 };
    // Simulated market clock starts right after the history.
    let mut sim_time = cfg
        .history
        .last()
        .map(|c| c.open_ts + Timeframe::M5.millis())
        .unwrap_or_else(|| Timeframe::M5.bucket(now_ms()));
    let mut snap_id = 0i64;
    let mut last_frame = String::new();
    let mut paused = false;
    let mut ticks = 0u64;

    macro_rules! send {
        ($body:expr) => {{
            match session.seal($body, now_ms()) {
                Ok(line) => {
                    if let Err(e) = write_frame(&mut wr, &line).await {
                        return SimExit::Disconnected(e.to_string());
                    }
                    last_frame = line;
                }
                Err(e) => return SimExit::Disconnected(e.to_string()),
            }
        }};
    }
    macro_rules! send_state {
        () => {{
            send!(Body::from_account(&broker.account(now_ms())));
            snap_id += 1;
            for p in broker.positions().to_vec() {
                send!(Body::from_position(snap_id, &p));
            }
            for o in broker.pending().to_vec() {
                send!(Body::from_pending(snap_id, &o));
            }
            send!(Body::SnapEnd { snap: snap_id, positions: broker.positions().len() as u32, orders: broker.pending().len() as u32 });
        }};
    }

    for spec in cfg.broker.specs.clone() {
        send!(Body::from_symbol_spec(&spec));
    }
    send_state!();

    let mut tick_timer = tokio::time::interval(Duration::from_millis(cfg.tick_interval_ms.max(1)));
    tick_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = tick_timer.tick() => {
                if paused { continue; }
                let bid = feed.next();
                let ask = bid + spread;
                sim_time += cfg.sim_ms_per_tick;
                let deals = broker.on_price(SIM_SYMBOL, bid, ask, sim_time);
                send!(Body::Tick { symbol: SIM_SYMBOL.into(), time: sim_time, bid, ask });
                ticks += 1;
                for d in &deals {
                    send!(Body::from_deal(d));
                }
                if !deals.is_empty() || ticks % 4 == 0 {
                    send_state!();
                }
            }
            _ = heartbeat.tick() => {
                send!(Body::Heartbeat {});
            }
            cmd = control.recv() => {
                match cmd {
                    Some(SimControl::Fault(f)) => broker.fault = f,
                    Some(SimControl::PauseTicks(p)) => paused = p,
                    Some(SimControl::StripStops) => {
                        broker.strip_stops();
                        send_state!();
                    }
                    Some(SimControl::ReplayLastFrame) => {
                        if let Err(e) = write_frame(&mut wr, &last_frame).await {
                            return SimExit::Disconnected(e.to_string());
                        }
                    }
                    Some(SimControl::Disconnect) | None => return SimExit::Stopped,
                }
            }
            frame = rd.next() => {
                let line = match frame {
                    Ok(Some(l)) => l,
                    Ok(None) => return SimExit::Disconnected("closed by the engine".into()),
                    Err(e) => return SimExit::Disconnected(e.to_string()),
                };
                let env = match session.open(&line, now_ms()) {
                    Ok(env) => env,
                    Err(e) => return SimExit::Disconnected(format!("protocol error: {e}")),
                };
                match env.body {
                    Body::Order { .. } => {
                        let started = std::time::Instant::now();
                        let (outcome, deals) = broker.execute(&env.body, now_ms());
                        let broker_us = started.elapsed().as_micros() as i64;
                        if let Some(out) = outcome {
                            let mut body = out.to_body();
                            if let Body::Result { broker_us: b, .. } = &mut body {
                                *b = broker_us.max(1);
                            }
                            send!(body);
                        }
                        for d in &deals {
                            send!(Body::from_deal(d));
                        }
                        send_state!();
                    }
                    Body::HistoryReq { symbol, tf, count } => {
                        let bars: Vec<Candle> = match Timeframe::parse(&tf) {
                            Some(Timeframe::M5) => cfg.history.clone(),
                            Some(t) if t > Timeframe::M5 => aggregate(&cfg.history, Timeframe::M5, t),
                            _ => Vec::new(),
                        };
                        let from = bars.len().saturating_sub(count as usize);
                        let slice = &bars[from..];
                        for (i, c) in slice.iter().enumerate() {
                            send!(Body::Bar {
                                symbol: symbol.clone(), tf: tf.clone(), time: c.open_ts, open: c.open, high: c.high, low: c.low,
                                close: c.close, volume: c.tick_volume, spread: c.spread_points, last: i + 1 == slice.len(),
                            });
                        }
                    }
                    Body::Bye { reason } => return SimExit::Disconnected(format!("engine said bye: {reason}")),
                    Body::Error { code, msg } => return SimExit::Disconnected(format!("engine error {code}: {msg}")),
                    _ => {}
                }
            }
        }
    }
}
