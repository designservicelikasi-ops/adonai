//! Engine configuration (TOML file + environment variables for secrets).

use aegis_execution::ExecConfig;
use aegis_learning::GateCriteria;
use aegis_market::MarketConfig;
use aegis_risk::RiskConfig;
use aegis_types::Timeframe;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineSection {
    /// Directory holding the database, logs, backups and the key file.
    pub data_dir: PathBuf,
    /// Instruments AEGIS is allowed to trade. Empty = every instrument the terminal sends.
    pub symbols: Vec<String>,
    /// Timeframes on which strategies are evaluated.
    pub signal_timeframes: Vec<Timeframe>,
    /// Bars of history requested from the terminal for each timeframe at connection.
    pub history_bars: u32,
}

impl Default for EngineSection {
    fn default() -> Self {
        EngineSection {
            data_dir: PathBuf::from("data"),
            symbols: Vec::new(),
            signal_timeframes: vec![Timeframe::M15],
            history_bars: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BridgeSection {
    /// Address the MT5 connector connects to. Keep it on loopback or a private network.
    pub listen: String,
    pub max_skew_ms: i64,
    /// The connection is dropped when the terminal stays silent this long.
    pub peer_timeout_ms: i64,
    /// Environment variable holding the shared key (at least 16 characters).
    pub shared_key_env: String,
}

impl Default for BridgeSection {
    fn default() -> Self {
        BridgeSection {
            listen: "127.0.0.1:7878".into(),
            max_skew_ms: 30_000,
            peer_timeout_ms: 10_000,
            shared_key_env: "AEGIS_SHARED_KEY".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiSection {
    pub enabled: bool,
    pub listen: String,
    /// Environment variable holding the bearer token of the HTTP API.
    pub token_env: String,
}

impl Default for ApiSection {
    fn default() -> Self {
        ApiSection { enabled: false, listen: "127.0.0.1:7879".into(), token_env: "AEGIS_API_TOKEN".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LearningSection {
    /// Model artifact to load at start-up. Empty = no model.
    pub model_path: String,
    pub gate: GateCriteria,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SafetySection {
    /// LIVE AUTO cannot even be requested unless this is set to true in the file.
    pub live_trading_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionSection {
    pub candle_days: i64,
    pub snapshot_days: i64,
}

impl Default for RetentionSection {
    fn default() -> Self {
        RetentionSection { candle_days: 1_095, snapshot_days: 180 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub engine: EngineSection,
    pub bridge: BridgeSection,
    pub api: ApiSection,
    /// Initial risk limits. Once changed in the application, the stored version wins.
    pub risk: RiskConfig,
    pub market: MarketConfig,
    pub execution: ExecConfig,
    pub learning: LearningSection,
    /// Probability Engine (P(TP before SL)) training and display settings.
    pub probability: aegis_probability::ProbabilityConfig,
    /// Economic calendar and news-based trading recommendation.
    pub news: aegis_news::NewsConfig,
    pub safety: SafetySection,
    pub retention: RetentionSection,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("configuration file is invalid: {0}")]
    Parse(String),
    #[error("configuration rejected: {0}")]
    Invalid(String),
}

impl EngineConfig {
    /// Load a TOML file. A missing file yields the defaults; an invalid one is an error.
    pub fn load(path: &Path) -> Result<EngineConfig, ConfigLoadError> {
        let cfg = if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|e| ConfigLoadError::Io(path.to_path_buf(), e))?;
            toml::from_str(&text).map_err(|e| ConfigLoadError::Parse(e.to_string()))?
        } else {
            EngineConfig::default()
        };
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigLoadError> {
        let bad = |m: String| Err(ConfigLoadError::Invalid(m));
        self.risk.validate().map_err(|e| ConfigLoadError::Invalid(e.to_string()))?;
        self.probability.validate().map_err(|e| ConfigLoadError::Invalid(format!("probability: {e}")))?;
        self.news.validate().map_err(ConfigLoadError::Invalid)?;
        if self.engine.signal_timeframes.is_empty() {
            return bad("engine.signal_timeframes must not be empty".into());
        }
        for tf in &self.engine.signal_timeframes {
            if !self.market.timeframes.contains(tf) {
                return bad(format!("signal timeframe {tf} is not in market.timeframes"));
            }
        }
        if self.market.timeframes.iter().any(|t| *t > Timeframe::H1) {
            return bad("market.timeframes above H1 are not supported (broker bars are not UTC-aligned)".into());
        }
        if !(1..=5_000).contains(&self.engine.history_bars) {
            return bad("engine.history_bars must be within 1..5000".into());
        }
        if !(1_000..=300_000).contains(&self.bridge.max_skew_ms) || !(3_000..=300_000).contains(&self.bridge.peer_timeout_ms) {
            return bad("bridge timings out of range".into());
        }
        if self.bridge.listen.parse::<std::net::SocketAddr>().is_err() {
            return bad(format!("bridge.listen `{}` is not an address", self.bridge.listen));
        }
        if self.execution.magic <= 0 || !(1_000..=60_000).contains(&self.execution.result_timeout_ms) {
            return bad("execution settings out of range".into());
        }
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        self.engine.data_dir.join("aegis.db")
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// Resolve the shared key: environment variable first, otherwise a key file in
/// the data directory, created with owner-only permissions on first use.
/// The key is never written to the configuration file or to the logs.
pub fn resolve_shared_key(cfg: &EngineConfig) -> Result<(Vec<u8>, String), ConfigLoadError> {
    if let Ok(v) = std::env::var(&cfg.bridge.shared_key_env) {
        let v = v.trim().to_string();
        if v.len() < aegis_protocol::MIN_KEY_BYTES {
            return Err(ConfigLoadError::Invalid(format!(
                "{} must contain at least {} characters",
                cfg.bridge.shared_key_env,
                aegis_protocol::MIN_KEY_BYTES
            )));
        }
        return Ok((v.into_bytes(), format!("variable d'environnement {}", cfg.bridge.shared_key_env)));
    }
    let path = cfg.engine.data_dir.join("shared.key");
    if !path.exists() {
        std::fs::create_dir_all(&cfg.engine.data_dir).map_err(|e| ConfigLoadError::Io(cfg.engine.data_dir.clone(), e))?;
        let key = hex::encode(rand::random::<[u8; 32]>());
        write_private(&path, &key).map_err(|e| ConfigLoadError::Io(path.clone(), e))?;
    }
    let key = std::fs::read_to_string(&path).map_err(|e| ConfigLoadError::Io(path.clone(), e))?.trim().to_string();
    if key.len() < aegis_protocol::MIN_KEY_BYTES {
        return Err(ConfigLoadError::Invalid(format!("{} is too short", path.display())));
    }
    Ok((key.into_bytes(), format!("fichier {}", path.display())))
}

fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("aegis-cfg-{}", aegis_types::new_id("t")));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_are_safe_and_roundtrip_through_toml() {
        let cfg = EngineConfig::default();
        cfg.validate().unwrap();
        assert!(!cfg.safety.live_trading_enabled, "live trading is off by default");
        assert!(!cfg.api.enabled);
        assert!(cfg.bridge.listen.starts_with("127.0.0.1"), "loopback only by default");
        let back: EngineConfig = toml::from_str(&cfg.to_toml()).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn missing_file_gives_defaults_and_bad_files_are_rejected() {
        let dir = tmp();
        assert_eq!(EngineConfig::load(&dir.join("absent.toml")).unwrap(), EngineConfig::default());
        let p = dir.join("a.toml");
        std::fs::write(&p, "[risk]\nrisk_per_trade_pct = 80.0\n").unwrap();
        assert!(matches!(EngineConfig::load(&p), Err(ConfigLoadError::Invalid(_))));
        std::fs::write(&p, "[safety]\nlive_trading_enbled = true\n").unwrap();
        assert!(matches!(EngineConfig::load(&p), Err(ConfigLoadError::Parse(_))), "typos are errors, not ignored");
        std::fs::write(&p, "[engine]\nsignal_timeframes = [\"H4\"]\n").unwrap();
        assert!(EngineConfig::load(&p).is_err());
        std::fs::write(&p, "[bridge]\nlisten = \"not-an-address\"\n").unwrap();
        assert!(EngineConfig::load(&p).is_err());
        std::fs::write(&p, "[risk]\nrisk_per_trade_pct = 0.25\n[safety]\nlive_trading_enabled = false\n").unwrap();
        assert_eq!(EngineConfig::load(&p).unwrap().risk.risk_per_trade_pct, 0.25);
    }

    #[test]
    fn shared_key_file_is_created_private_and_stable() {
        let dir = tmp();
        let mut cfg = EngineConfig::default();
        cfg.engine.data_dir = dir.clone();
        cfg.bridge.shared_key_env = "AEGIS_TEST_KEY_THAT_IS_NOT_SET".into();
        let (k1, source) = resolve_shared_key(&cfg).unwrap();
        assert_eq!(k1.len(), 64);
        assert!(source.contains("shared.key"));
        let (k2, _) = resolve_shared_key(&cfg).unwrap();
        assert_eq!(k1, k2, "the key is generated once");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("shared.key")).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::write(dir.join("shared.key"), "short").unwrap();
        assert!(resolve_shared_key(&cfg).is_err());
    }
}
