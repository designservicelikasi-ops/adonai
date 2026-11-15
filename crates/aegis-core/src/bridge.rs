//! Network bridge: authenticated TCP endpoint the MT5 connector connects to.
//!
//! One terminal at a time. Nothing a peer sends reaches the engine before the
//! handshake succeeded, and any protocol violation closes the connection.

use aegis_protocol::io::{write_frame, FrameReader};
use aegis_protocol::{server_accept, Body, HelloInfo, NonceCache, ProtoError};
use aegis_types::now_ms;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

#[derive(Debug)]
pub enum BridgeEvent {
    Connected { conn: u64, info: HelloInfo, peer: String },
    Frame { conn: u64, body: Body },
    Disconnected { conn: u64, reason: String, violation: bool },
    /// A connection attempt that never became a session.
    Refused { peer: String, reason: String },
}

#[derive(Clone)]
pub struct BridgeConfig {
    pub psk: Arc<Vec<u8>>,
    pub max_skew_ms: i64,
    pub peer_timeout_ms: i64,
}

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);

/// Accept loop. `outbound` carries the engine's messages for the active terminal.
pub async fn serve(
    listener: TcpListener,
    cfg: BridgeConfig,
    events: mpsc::Sender<BridgeEvent>,
    outbound: mpsc::Receiver<Body>,
) {
    let outbound = Arc::new(Mutex::new(outbound));
    let nonces = Arc::new(Mutex::new(NonceCache::new(cfg.max_skew_ms)));
    let busy = Arc::new(AtomicBool::new(false));
    let mut next_conn = 0u64;
    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("bridge accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let peer = addr.to_string();
        if busy.swap(true, Ordering::SeqCst) {
            let _ = events.send(BridgeEvent::Refused { peer, reason: "un terminal est déjà connecté".into() }).await;
            continue; // dropping the stream closes it
        }
        next_conn += 1;
        let (conn, cfg, events, outbound, nonces, busy) = (next_conn, cfg.clone(), events.clone(), outbound.clone(), nonces.clone(), busy.clone());
        tokio::spawn(async move {
            connection(conn, stream, peer, cfg, events, outbound, nonces).await;
            busy.store(false, Ordering::SeqCst);
        });
    }
}

async fn connection(
    conn: u64,
    stream: TcpStream,
    peer: String,
    cfg: BridgeConfig,
    events: mpsc::Sender<BridgeEvent>,
    outbound: Arc<Mutex<mpsc::Receiver<Body>>>,
    nonces: Arc<Mutex<NonceCache>>,
) {
    let _ = stream.set_nodelay(true);
    let (rd, mut wr) = stream.into_split();
    let mut rd = FrameReader::new(BufReader::new(rd));

    // ---- handshake: one frame, bounded in size and time.
    let hello = match tokio::time::timeout(HANDSHAKE_TIMEOUT, rd.next()).await {
        Ok(Ok(Some(line))) => line,
        Ok(Ok(None)) => return refuse(&events, peer, "connexion fermée avant l'identification").await,
        Ok(Err(e)) => return refuse(&events, peer, &format!("lecture impossible: {e}")).await,
        Err(_) => return refuse(&events, peer, "aucune identification reçue").await,
    };
    let accepted = {
        let mut cache = nonces.lock().await;
        server_accept(&cfg.psk, &hello, &mut cache, cfg.max_skew_ms, now_ms())
    };
    let (welcome, mut session, info) = match accepted {
        Ok(v) => v,
        // The peer learns nothing about why it was refused.
        Err(e) => return refuse(&events, peer, &format!("identification refusée: {e}")).await,
    };
    if write_frame(&mut wr, &welcome).await.is_err() {
        return refuse(&events, peer, "écriture impossible pendant l'identification").await;
    }

    let mut outbound = outbound.lock().await;
    // Anything queued while no terminal was connected is stale: discard it.
    while outbound.try_recv().is_ok() {}
    if events.send(BridgeEvent::Connected { conn, info, peer }).await.is_err() {
        return;
    }

    let mut heartbeat = tokio::time::interval(HEARTBEAT_EVERY);
    let mut last_rx = now_ms();
    let (reason, violation) = loop {
        tokio::select! {
            frame = rd.next() => {
                let line = match frame {
                    Ok(Some(l)) => l,
                    Ok(None) => break ("connexion fermée par le terminal".to_string(), false),
                    Err(e) => break (format!("erreur réseau: {e}"), e.kind() == std::io::ErrorKind::InvalidData),
                };
                match session.open(&line, now_ms()) {
                    Ok(env) => {
                        last_rx = now_ms();
                        match env.body {
                            Body::Heartbeat {} => {}
                            Body::Bye { reason } => break (format!("le terminal s'est déconnecté: {reason}"), false),
                            // Engine-only messages are not accepted from a terminal.
                            Body::Order { .. } | Body::HistoryReq { .. } | Body::Welcome { .. } | Body::Hello { .. } => {
                                let _ = send(&mut session, &mut wr, Body::Error { code: "unexpected".into(), msg: "message not allowed in this direction".into() }).await;
                                break ("message interdit dans ce sens".to_string(), true);
                            }
                            body => {
                                if events.send(BridgeEvent::Frame { conn, body }).await.is_err() {
                                    break ("moteur arrêté".to_string(), false);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = send(&mut session, &mut wr, Body::Error { code: "protocol".into(), msg: "frame rejected".into() }).await;
                        break (format!("trame rejetée: {}", describe(&e)), true);
                    }
                }
            }
            out = outbound.recv() => {
                match out {
                    Some(body) => {
                        if let Err(e) = send(&mut session, &mut wr, body).await {
                            break (format!("envoi impossible: {e}"), false);
                        }
                    }
                    None => break ("moteur arrêté".to_string(), false),
                }
            }
            _ = heartbeat.tick() => {
                if now_ms() - last_rx > cfg.peer_timeout_ms {
                    break (format!("terminal silencieux depuis plus de {} s", cfg.peer_timeout_ms / 1000), false);
                }
                if let Err(e) = send(&mut session, &mut wr, Body::Heartbeat {}).await {
                    break (format!("envoi impossible: {e}"), false);
                }
            }
        }
    };
    let _ = events.send(BridgeEvent::Disconnected { conn, reason, violation }).await;
}

async fn send(
    session: &mut aegis_protocol::Session,
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    body: Body,
) -> Result<(), String> {
    let line = session.seal(body, now_ms()).map_err(|e| e.to_string())?;
    write_frame(wr, &line).await.map_err(|e| e.to_string())
}

async fn refuse(events: &mpsc::Sender<BridgeEvent>, peer: String, reason: &str) {
    let _ = events.send(BridgeEvent::Refused { peer, reason: reason.to_string() }).await;
}

fn describe(e: &ProtoError) -> String {
    match e {
        ProtoError::BadMac => "authentification invalide".into(),
        ProtoError::Sequence { .. } => format!("séquence invalide (rejeu ou perte): {e}"),
        ProtoError::Stale(_) => format!("horodatage hors fenêtre: {e}"),
        other => other.to_string(),
    }
}
