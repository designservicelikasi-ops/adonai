//! AEGIS NEWS INTELLIGENCE — economic calendar and trading recommendation.
//!
//! Events come from configured sources only (an economic-calendar JSON feed or
//! a local file). Nothing is inferred or invented: a value the source does not
//! publish stays `None`. The module never trades; it only recommends, for each
//! instrument, `allowed`, `caution` or `suspended`, with the events behind it.

use aegis_types::TimestampMs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    Holiday,
    Low,
    Medium,
    High,
    Unknown,
}

impl Impact {
    pub fn parse(s: &str) -> Impact {
        match s.trim().to_ascii_lowercase().as_str() {
            "high" => Impact::High,
            "medium" => Impact::Medium,
            "low" => Impact::Low,
            "holiday" | "non-economic" => Impact::Holiday,
            _ => Impact::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsEvent {
    /// Stable identifier: hash of source, title, currency and time.
    pub id: String,
    pub title: String,
    /// ISO currency the event concerns (USD, EUR...).
    pub currency: String,
    pub ts_ms: TimestampMs,
    pub impact: Impact,
    pub forecast: Option<String>,
    pub previous: Option<String>,
    /// Published value. `None` when the source does not provide it (never guessed).
    pub actual: Option<String>,
    pub source: String,
    pub fetched_ms: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsSource {
    pub name: String,
    /// `http(s)://...` or `file://...` (or a plain path).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewsConfig {
    pub enabled: bool,
    pub sources: Vec<NewsSource>,
    pub refresh_minutes: u32,
    /// Suspend trading on the event's currencies this long before/after a HIGH impact event.
    pub high_before_min: i64,
    pub high_after_min: i64,
    /// Caution around MEDIUM impact events.
    pub medium_before_min: i64,
    pub medium_after_min: i64,
    /// While in caution, a signal needs this much more probability than the normal threshold.
    pub caution_probability_margin: f64,
    /// When the calendar cannot be fetched: `true` = suspend, `false` = caution.
    pub suspend_when_unavailable: bool,
}

impl Default for NewsConfig {
    fn default() -> Self {
        NewsConfig {
            enabled: true,
            sources: vec![NewsSource { name: "faireconomy".into(), url: "https://nfs.faireconomy.media/ff_calendar_thisweek.json".into() }],
            refresh_minutes: 60,
            high_before_min: 30,
            high_after_min: 30,
            medium_before_min: 10,
            medium_after_min: 10,
            caution_probability_margin: 0.05,
            suspend_when_unavailable: false,
        }
    }
}

impl NewsConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.sources.is_empty() {
            return Err("news.sources must not be empty when news is enabled".into());
        }
        if !(5..=1_440).contains(&self.refresh_minutes) {
            return Err("news.refresh_minutes must be within 5..1440".into());
        }
        for v in [self.high_before_min, self.high_after_min, self.medium_before_min, self.medium_after_min] {
            if !(0..=1_440).contains(&v) {
                return Err("news windows must be within 0..1440 minutes".into());
            }
        }
        if !(0.0..=0.3).contains(&self.caution_probability_margin) {
            return Err("news.caution_probability_margin must be within 0..0.3".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recommendation {
    Allowed,
    Caution,
    Suspended,
}

impl Recommendation {
    pub fn label(self) -> &'static str {
        match self {
            Recommendation::Allowed => "trading autorisé",
            Recommendation::Caution => "trading prudent",
            Recommendation::Suspended => "trading suspendu temporairement",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsAssessment {
    pub recommendation: Recommendation,
    pub reasons: Vec<String>,
    /// Events inside the active windows.
    pub active: Vec<NewsEvent>,
    /// Next relevant (medium/high) event for these currencies.
    pub next: Option<NewsEvent>,
}

fn event_id(source: &str, title: &str, currency: &str, ts: TimestampMs) -> String {
    let mut h = Sha256::new();
    h.update(format!("{source}|{title}|{currency}|{ts}"));
    hex::encode(&h.finalize()[..12])
}

fn text(v: Option<&serde_json::Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Parse a calendar in the common JSON layout:
/// `[{"title","country","date" (RFC 3339),"impact","forecast","previous","actual"?}]`.
/// Malformed entries are skipped and counted, never repaired by guessing.
pub fn parse_calendar(json: &str, source: &str, fetched_ms: TimestampMs) -> Result<(Vec<NewsEvent>, usize), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("calendrier illisible: {e}"))?;
    let items = v.as_array().ok_or("le calendrier doit être une liste d'événements")?;
    let (mut out, mut skipped) = (Vec::new(), 0);
    for it in items {
        let (Some(title), Some(currency), Some(date)) = (text(it.get("title")), text(it.get("country")), text(it.get("date"))) else {
            skipped += 1;
            continue;
        };
        let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&date) else {
            skipped += 1;
            continue;
        };
        let currency = currency.to_ascii_uppercase();
        if currency.len() != 3 && currency != "ALL" {
            skipped += 1;
            continue;
        }
        let ts = dt.timestamp_millis();
        out.push(NewsEvent {
            id: event_id(source, &title, &currency, ts),
            title,
            currency,
            ts_ms: ts,
            impact: Impact::parse(it.get("impact").and_then(|x| x.as_str()).unwrap_or("")),
            forecast: text(it.get("forecast")),
            previous: text(it.get("previous")),
            actual: text(it.get("actual")),
            source: source.to_string(),
            fetched_ms,
        });
    }
    out.sort_by_key(|e| e.ts_ms);
    Ok((out, skipped))
}

/// Read one source (HTTP(S) or file). Blocking: call it from a blocking task.
pub fn fetch(source: &NewsSource, now_ms: TimestampMs) -> Result<Vec<NewsEvent>, String> {
    let body = if source.url.starts_with("http://") || source.url.starts_with("https://") {
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).user_agent("AEGIS-news/0.2").build();
        agent
            .get(&source.url)
            .call()
            .map_err(|e| format!("{}: {e}", source.name))?
            .into_string()
            .map_err(|e| format!("{}: {e}", source.name))?
    } else {
        let path = source.url.strip_prefix("file://").unwrap_or(&source.url);
        std::fs::read_to_string(path).map_err(|e| format!("{}: {path}: {e}", source.name))?
    };
    parse_calendar(&body, &source.name, now_ms).map(|(e, _)| e)
}

/// Merge fresh events into the known list (same id = replaced), keep 14 days around now.
pub fn merge(known: &mut Vec<NewsEvent>, fresh: Vec<NewsEvent>, now_ms: TimestampMs) {
    for e in fresh {
        match known.iter_mut().find(|k| k.id == e.id) {
            Some(k) => *k = e,
            None => known.push(e),
        }
    }
    known.retain(|e| (e.ts_ms - now_ms).abs() <= 14 * 86_400_000);
    known.sort_by_key(|e| e.ts_ms);
}

/// Recommendation for an instrument exposed to `currencies`.
/// `feed_ok` is false when the calendar could not be refreshed recently.
pub fn assess(events: &[NewsEvent], currencies: &[String], now_ms: TimestampMs, cfg: &NewsConfig, feed_ok: bool) -> NewsAssessment {
    let mut out = NewsAssessment { recommendation: Recommendation::Allowed, reasons: Vec::new(), active: Vec::new(), next: None };
    if !cfg.enabled {
        out.reasons.push("module d'actualités désactivé".into());
        return out;
    }
    if !feed_ok {
        out.recommendation = if cfg.suspend_when_unavailable { Recommendation::Suspended } else { Recommendation::Caution };
        out.reasons.push("calendrier économique indisponible: impossible d'exclure une annonce".into());
    }
    let concerns = |e: &NewsEvent| e.currency == "ALL" || currencies.iter().any(|c| c.eq_ignore_ascii_case(&e.currency));
    let min = 60_000;
    for e in events.iter().filter(|e| concerns(e)) {
        let (before, after, level) = match e.impact {
            Impact::High => (cfg.high_before_min, cfg.high_after_min, Recommendation::Suspended),
            Impact::Medium => (cfg.medium_before_min, cfg.medium_after_min, Recommendation::Caution),
            _ => continue,
        };
        if now_ms >= e.ts_ms - before * min && now_ms <= e.ts_ms + after * min {
            out.recommendation = out.recommendation.max(level);
            let when = (e.ts_ms - now_ms) / min;
            out.reasons.push(format!(
                "{} {} ({:?}) {}",
                e.currency,
                e.title,
                e.impact,
                if when >= 0 { format!("dans {when} min") } else { format!("il y a {} min", -when) }
            ));
            out.active.push(e.clone());
        } else if e.ts_ms > now_ms && out.next.is_none() {
            out.next = Some(e.clone());
        }
    }
    if out.reasons.is_empty() {
        out.reasons.push("aucune annonce importante dans les fenêtres configurées".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[
        {"title":"Non-Farm Employment Change","country":"USD","date":"2026-11-20T08:30:00-05:00","impact":"High","forecast":"150K","previous":"120K"},
        {"title":"German ZEW","country":"EUR","date":"2026-11-20T05:00:00-05:00","impact":"Medium","forecast":"","previous":"10.2"},
        {"title":"Bank Holiday","country":"JPY","date":"2026-11-20T00:00:00-05:00","impact":"Holiday","forecast":"","previous":""},
        {"title":"broken","country":"USD","date":"not a date","impact":"High"},
        {"title":"no currency","date":"2026-11-20T08:30:00-05:00","impact":"High"}
    ]"#;

    fn ms(rfc: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(rfc).unwrap().timestamp_millis()
    }

    fn usd_eur() -> Vec<String> {
        vec!["EUR".into(), "USD".into()]
    }

    #[test]
    fn parses_known_fields_and_skips_malformed_entries() {
        let (ev, skipped) = parse_calendar(FIXTURE, "fixture", 1).unwrap();
        assert_eq!((ev.len(), skipped), (3, 2));
        let nfp = ev.iter().find(|e| e.currency == "USD").unwrap();
        assert_eq!(nfp.ts_ms, ms("2026-11-20T13:30:00Z"), "time zone converted to UTC");
        assert_eq!(nfp.impact, Impact::High);
        assert_eq!(nfp.forecast.as_deref(), Some("150K"));
        assert_eq!(nfp.actual, None, "not published by the source: never guessed");
        let zew = ev.iter().find(|e| e.currency == "EUR").unwrap();
        assert_eq!(zew.forecast, None, "empty string is no value");
        assert!(parse_calendar("{}", "x", 0).is_err());
        assert!(parse_calendar("not json", "x", 0).is_err());
    }

    #[test]
    fn windows_around_high_and_medium_events() {
        let (ev, _) = parse_calendar(FIXTURE, "fixture", 1).unwrap();
        let cfg = NewsConfig::default();
        let nfp = ms("2026-11-20T13:30:00Z");
        let a = assess(&ev, &usd_eur(), nfp - 20 * 60_000, &cfg, true);
        assert_eq!(a.recommendation, Recommendation::Suspended);
        assert!(a.reasons[0].contains("Non-Farm") && a.reasons[0].contains("dans 20 min"), "{:?}", a.reasons);
        assert_eq!(assess(&ev, &usd_eur(), nfp + 29 * 60_000, &cfg, true).recommendation, Recommendation::Suspended);
        assert_eq!(assess(&ev, &usd_eur(), nfp + 31 * 60_000, &cfg, true).recommendation, Recommendation::Allowed);
        // An instrument without USD/EUR exposure is not concerned.
        assert_eq!(assess(&ev, &["GBP".into(), "JPY".into()], nfp, &cfg, true).recommendation, Recommendation::Allowed);
        let zew = ms("2026-11-20T10:00:00Z");
        let a = assess(&ev, &usd_eur(), zew - 5 * 60_000, &cfg, true);
        assert_eq!(a.recommendation, Recommendation::Caution);
        assert_eq!(a.next.as_ref().map(|e| e.currency.as_str()), Some("USD"), "next relevant event");
    }

    #[test]
    fn unavailable_calendar_is_never_read_as_quiet() {
        let cfg = NewsConfig::default();
        assert_eq!(assess(&[], &usd_eur(), 0, &cfg, false).recommendation, Recommendation::Caution);
        let strict = NewsConfig { suspend_when_unavailable: true, ..Default::default() };
        assert_eq!(assess(&[], &usd_eur(), 0, &strict, false).recommendation, Recommendation::Suspended);
        let off = NewsConfig { enabled: false, ..Default::default() };
        assert_eq!(assess(&[], &usd_eur(), 0, &off, false).recommendation, Recommendation::Allowed);
    }

    #[test]
    fn merge_replaces_and_expires() {
        let now = ms("2026-11-20T00:00:00Z");
        let (ev, _) = parse_calendar(FIXTURE, "fixture", 1).unwrap();
        let mut known = ev.clone();
        let mut updated = ev[0].clone();
        updated.actual = Some("170K".into());
        let mut old = ev[1].clone();
        old.id = "old".into();
        old.ts_ms = now - 30 * 86_400_000;
        known.push(old);
        merge(&mut known, vec![updated.clone()], now);
        assert_eq!(known.len(), 3, "updated in place, old event expired");
        assert!(known.contains(&updated));
    }

    /// Network test, excluded from the normal run: `cargo test -p aegis-news -- --ignored`.
    #[test]
    #[ignore]
    fn live_default_feed() {
        let cfg = NewsConfig::default();
        let ev = fetch(&cfg.sources[0], 0).expect("the public calendar answers");
        assert!(!ev.is_empty());
        assert!(ev.iter().any(|e| e.impact == Impact::High));
        println!("{} events, first: {:?}", ev.len(), ev.first().map(|e| (&e.currency, &e.title)));
    }

    #[test]
    fn file_source_and_config_validation() {
        let dir = std::env::temp_dir().join(format!("aegis-news-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("calendar.json");
        std::fs::write(&path, FIXTURE).unwrap();
        let src = NewsSource { name: "local".into(), url: format!("file://{}", path.display()) };
        assert_eq!(fetch(&src, 0).unwrap().len(), 3);
        assert!(fetch(&NewsSource { name: "x".into(), url: "/no/such/file.json".into() }, 0).is_err());
        NewsConfig::default().validate().unwrap();
        assert!(NewsConfig { sources: vec![], ..Default::default() }.validate().is_err());
        assert!(NewsConfig { refresh_minutes: 1, ..Default::default() }.validate().is_err());
    }
}
