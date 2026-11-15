//! Candle CSV import/export.
//!
//! Accepted input formats:
//! * AEGIS: `open_ts_ms,open,high,low,close,tick_volume,spread_points` (header optional)
//! * MetaTrader 5 history export (tab separated):
//!   `<DATE> <TIME> <OPEN> <HIGH> <LOW> <CLOSE> <TICKVOL> <VOL> <SPREAD>` with dates
//!   like `2024.01.02` and times like `13:45:00`. MT5 exports use broker server
//!   time; pass the server's UTC offset so bars are stored in UTC.

use aegis_types::{Candle, Timeframe};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CsvError {
    #[error("line {line}: {problem}")]
    Line { line: usize, problem: String },
    #[error("no candle found in the file")]
    Empty,
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Howard Hinnant's algorithm, valid for the proleptic Gregorian calendar.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_mt5_datetime(date: &str, time: &str) -> Option<i64> {
    let d: Vec<i64> = date.split('.').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if d.len() != 3 || t.len() < 2 || !(1..=12).contains(&d[1]) || !(1..=31).contains(&d[2]) {
        return None;
    }
    let secs = t[0] * 3600 + t[1] * 60 + t.get(2).copied().unwrap_or(0);
    Some((days_from_civil(d[0], d[1], d[2]) * 86_400 + secs) * 1000)
}

/// Parse candles. Every row is validated; the first bad row aborts the import.
pub fn parse_candles(text: &str, tf: Timeframe, server_utc_offset_hours: i64) -> Result<Vec<Candle>, CsvError> {
    let mut out: Vec<Candle> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let err = |problem: &str| CsvError::Line { line: i + 1, problem: problem.to_string() };
        let fields: Vec<&str> = if line.contains('\t') { line.split('\t').collect() } else { line.split(',').collect() };
        let fields: Vec<&str> = fields.iter().map(|f| f.trim()).collect();
        // Header rows start with a letter or '<'.
        if fields[0].starts_with('<') || fields[0].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let num = |s: &str| s.parse::<f64>().map_err(|_| err("invalid number"));
        let candle = if fields[0].contains('.') && fields.len() >= 6 && fields[0].split('.').count() == 3 {
            let ts = parse_mt5_datetime(fields[0], fields[1]).ok_or_else(|| err("invalid date/time"))?
                - server_utc_offset_hours * 3_600_000;
            Candle {
                open_ts: ts,
                open: num(fields[2])?,
                high: num(fields[3])?,
                low: num(fields[4])?,
                close: num(fields[5])?,
                tick_volume: fields.get(6).map(|v| num(v)).transpose()?.unwrap_or(0.0) as i64,
                spread_points: fields.get(8).map(|v| num(v)).transpose()?.unwrap_or(0.0),
            }
        } else if fields.len() >= 5 {
            Candle {
                open_ts: fields[0].parse::<i64>().map_err(|_| err("invalid timestamp"))?,
                open: num(fields[1])?,
                high: num(fields[2])?,
                low: num(fields[3])?,
                close: num(fields[4])?,
                tick_volume: fields.get(5).map(|v| num(v)).transpose()?.unwrap_or(0.0) as i64,
                spread_points: fields.get(6).map(|v| num(v)).transpose()?.unwrap_or(0.0),
            }
        } else {
            return Err(err("unrecognised row"));
        };
        if !candle.is_valid() {
            return Err(err("inconsistent OHLC values"));
        }
        if candle.open_ts % tf.millis() != 0 {
            return Err(err("timestamp is not aligned with the timeframe"));
        }
        if let Some(prev) = out.last() {
            if candle.open_ts <= prev.open_ts {
                return Err(err("timestamps must be strictly increasing"));
            }
        }
        out.push(candle);
    }
    if out.is_empty() {
        return Err(CsvError::Empty);
    }
    Ok(out)
}

pub fn to_csv(candles: &[Candle]) -> String {
    let mut s = String::from("open_ts_ms,open,high,low,close,tick_volume,spread_points\n");
    for c in candles {
        s.push_str(&format!("{},{},{},{},{},{},{}\n", c.open_ts, c.open, c.high, c.low, c.close, c.tick_volume, c.spread_points));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aegis_csv_roundtrip() {
        let bars = crate::synthetic::generate(&crate::synthetic::SyntheticParams { bars: 50, ..Default::default() });
        let parsed = parse_candles(&to_csv(&bars), Timeframe::M15, 0).unwrap();
        assert_eq!(parsed.len(), 50);
        for (a, b) in bars.iter().zip(&parsed) {
            assert_eq!(a.open_ts, b.open_ts);
            assert!((a.close - b.close).abs() < 1e-12);
        }
    }

    #[test]
    fn mt5_export_is_parsed_and_shifted_to_utc() {
        let text = "<DATE>\t<TIME>\t<OPEN>\t<HIGH>\t<LOW>\t<CLOSE>\t<TICKVOL>\t<VOL>\t<SPREAD>\n\
                    2024.01.02\t02:00:00\t1.10400\t1.10450\t1.10380\t1.10420\t120\t0\t9\n\
                    2024.01.02\t02:15:00\t1.10420\t1.10480\t1.10410\t1.10470\t98\t0\t10\n";
        let bars = parse_candles(text, Timeframe::M15, 2).unwrap();
        assert_eq!(bars.len(), 2);
        // 2024-01-02 02:00 server time at UTC+2 is 2024-01-02 00:00 UTC.
        assert_eq!(bars[0].open_ts, 1_704_153_600_000);
        assert_eq!(bars[0].tick_volume, 120);
        assert_eq!(bars[1].spread_points, 10.0);
        assert_eq!(bars[1].open_ts - bars[0].open_ts, 900_000);
    }

    #[test]
    fn bad_rows_abort_the_import() {
        let ok = "0,1.1,1.2,1.0,1.15,5,10\n";
        assert!(parse_candles(ok, Timeframe::M15, 0).is_ok());
        let cases = [
            ("0,1.1,1.0,1.2,1.15,5,10\n", "inconsistent OHLC"),
            ("0,1.1,abc,1.0,1.15\n", "invalid number"),
            ("1234,1.1,1.2,1.0,1.15\n", "not aligned"),
            ("900000,1.1,1.2,1.0,1.15\n0,1.1,1.2,1.0,1.15\n", "strictly increasing"),
            ("1,2\n", "unrecognised"),
        ];
        for (text, needle) in cases {
            let e = parse_candles(text, Timeframe::M15, 0).unwrap_err();
            assert!(e.to_string().contains(needle), "{e} should mention `{needle}`");
        }
        assert_eq!(parse_candles("open_ts_ms,open\n\n", Timeframe::M15, 0), Err(CsvError::Empty));
    }
}
