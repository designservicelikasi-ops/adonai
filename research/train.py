"""AEGIS — model training and validation.

Trains a calibrated logistic regression that estimates
    P(close[t + horizon] > close[t])
from the features computed by the Rust engine (`aegisd export-dataset`), and
writes a JSON artifact that the engine loads.

Honesty rules built into this script:
  * validation is walk-forward (train on the past, test on the future) with an
    embargo of `horizon` bars, so overlapping labels cannot leak;
  * the calibration set is always later than the fitting set;
  * the artifact records the raw metrics. AEGIS re-evaluates its own gate from
    those metrics; nothing written here can "approve" a model;
  * a model trained on synthetic data is tagged as such and can never be used
    for execution.

Usage:
    uv run train.py --dataset out/dataset.csv --out ../models/model.json
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

import numpy as np
import polars as pl
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score
from sklearn.preprocessing import StandardScaler

FEATURES = [
    "ret_1", "ret_5", "ret_20", "ema_gap_atr", "rsi_14", "atr_rel", "efficiency_20",
    "vol_ratio", "channel_pos_20", "hour_sin", "hour_cos",
]


def sigmoid(z: np.ndarray) -> np.ndarray:
    out = np.empty_like(z, dtype=float)
    pos = z >= 0
    out[pos] = 1.0 / (1.0 + np.exp(-z[pos]))
    e = np.exp(z[~pos])
    out[~pos] = e / (1.0 + e)
    return out


class Model:
    """Standardised logistic regression + Platt scaling, in plain numbers."""

    def __init__(self, mean, scale, coef, intercept, a, b):
        self.mean, self.scale, self.coef, self.intercept, self.a, self.b = mean, scale, coef, intercept, a, b

    def logit(self, x: np.ndarray) -> np.ndarray:
        return ((x - self.mean) / self.scale) @ self.coef + self.intercept

    def predict(self, x: np.ndarray) -> np.ndarray:
        return sigmoid(self.a * self.logit(x) + self.b)


def fit(x: np.ndarray, y: np.ndarray, embargo: int) -> Model:
    """Fit on the first 80 % of the rows, calibrate on the last 20 % (after an embargo)."""
    n = len(y)
    cut = int(n * 0.8)
    fit_end = max(cut - embargo, 1)
    scaler = StandardScaler().fit(x[:fit_end])
    scale = np.where(scaler.scale_ > 0, scaler.scale_, 1.0)
    mean = scaler.mean_
    lr = LogisticRegression(C=1.0, max_iter=1000)
    lr.fit((x[:fit_end] - mean) / scale, y[:fit_end])
    coef, intercept = lr.coef_[0], float(lr.intercept_[0])
    z_cal = ((x[cut:] - mean) / scale) @ coef + intercept
    y_cal = y[cut:]
    if len(np.unique(y_cal)) < 2:
        a, b = 1.0, 0.0
    else:
        platt = LogisticRegression(C=1e6, max_iter=1000)
        platt.fit(z_cal.reshape(-1, 1), y_cal)
        a, b = float(platt.coef_[0][0]), float(platt.intercept_[0])
    return Model(mean, scale, coef, intercept, a, b)


def expected_calibration_error(p: np.ndarray, y: np.ndarray, bins: int = 10) -> float:
    edges = np.linspace(0.0, 1.0, bins + 1)
    idx = np.clip(np.digitize(p, edges[1:-1]), 0, bins - 1)
    ece = 0.0
    for k in range(bins):
        mask = idx == k
        if mask.any():
            ece += mask.mean() * abs(p[mask].mean() - y[mask].mean())
    return float(ece)


def walk_forward(x: np.ndarray, y: np.ndarray, folds: int, embargo: int):
    """Expanding-window validation: fold k trains on blocks 0..k-1 and tests on block k."""
    n = len(y)
    block = n // (folds + 1)
    reports, oos_p, oos_y, baseline = [], [], [], []
    for k in range(1, folds + 1):
        train_end = k * block - embargo
        test_from, test_to = k * block, (k + 1) * block if k < folds else n
        if train_end < 200:
            continue
        model = fit(x[:train_end], y[:train_end], embargo)
        p = model.predict(x[test_from:test_to])
        yt = y[test_from:test_to]
        auc = float(roc_auc_score(yt, p)) if len(np.unique(yt)) == 2 else float("nan")
        reports.append({
            "n": int(len(yt)),
            "auc": auc,
            "brier": float(brier_score_loss(yt, p)),
            "log_loss": float(log_loss(yt, np.clip(p, 1e-9, 1 - 1e-9), labels=[0, 1])),
        })
        # Naive reference: always predict the base rate observed in training.
        baseline.append(float(np.mean((y[:train_end].mean() - yt) ** 2)))
        oos_p.append(p)
        oos_y.append(yt)
    return reports, np.concatenate(oos_p), np.concatenate(oos_y), baseline


def drift_reference(x: np.ndarray):
    """Deciles of each feature, binned with the same rule as the Rust monitor."""
    out = []
    for j in range(x.shape[1]):
        edges = np.quantile(x[:, j], np.linspace(0.1, 0.9, 9))
        bins = np.searchsorted(edges, x[:, j], side="left")
        fractions = np.bincount(bins, minlength=10) / len(x)
        out.append({"edges": [float(e) for e in edges], "fractions": [float(f) for f in fractions]})
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dataset", required=True, type=Path)
    ap.add_argument("--meta", type=Path, help="defaults to <dataset>.meta.json")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--folds", type=int, default=5)
    ap.add_argument("--report", type=Path, help="Markdown validation report (default: next to the model)")
    args = ap.parse_args()

    meta_path = args.meta or args.dataset.with_suffix(".meta.json")
    meta = json.loads(meta_path.read_text())
    if meta["features"] != FEATURES:
        print("ERROR: the dataset's features do not match this script", file=sys.stderr)
        return 2
    horizon = int(meta["horizon_bars"])

    df = pl.read_csv(args.dataset).sort("ts")
    x = df.select(FEATURES).to_numpy().astype(float)
    y = df["label"].to_numpy().astype(int)
    if not np.isfinite(x).all():
        print("ERROR: non-finite feature values in the dataset", file=sys.stderr)
        return 2
    if len(y) < 1000:
        print(f"ERROR: only {len(y)} rows; at least 1000 are needed", file=sys.stderr)
        return 2

    folds, oos_p, oos_y, baseline = walk_forward(x, y, args.folds, horizon)
    if not folds:
        print("ERROR: not enough data for walk-forward validation", file=sys.stderr)
        return 2
    validation = {
        "method": f"walk-forward, {len(folds)} folds, expanding window, embargo {horizon} bars, Platt calibration on the last 20 % of each training window",
        "n_samples": int(len(y)),
        "folds": folds,
        "auc_mean": float(np.nanmean([f["auc"] for f in folds])) if not all(np.isnan(f["auc"]) for f in folds) else float("nan"),
        "brier_mean": float(np.mean([f["brier"] for f in folds])),
        "baseline_brier": float(np.mean(baseline)),
        "ece": expected_calibration_error(oos_p, oos_y),
        "positive_rate": float(y.mean()),
    }

    final = fit(x, y, horizon)
    rng = np.random.default_rng(0)
    rows = rng.choice(len(x), size=8, replace=False)
    parity = [{"features": [float(v) for v in x[i]], "probability": float(final.predict(x[i:i + 1])[0])} for i in rows]

    created = int(time.time() * 1000)
    artifact = {
        "model_id": f"lr-{meta['symbol']}-{meta['timeframe']}-h{horizon}-{created}",
        "kind": "logistic_regression",
        "created_ms": created,
        "origin": meta["origin"],
        "symbol": meta["symbol"],
        "timeframe": meta["timeframe"],
        "horizon_bars": horizon,
        "target": f"P(close[t+{horizon}] > close[t])",
        "feature_names": FEATURES,
        "mean": [float(v) for v in final.mean],
        "scale": [float(v) for v in final.scale],
        "coef": [float(v) for v in final.coef],
        "intercept": final.intercept,
        "calibration_a": final.a,
        "calibration_b": final.b,
        "validation": validation,
        "parity": parity,
        "drift_reference": drift_reference(x),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(artifact, indent=1))

    improvement = validation["baseline_brier"] - validation["brier_mean"]
    lines = [
        f"# Rapport de validation — {artifact['model_id']}",
        "",
        f"- Données : {meta['symbol']} {meta['timeframe']}, origine **{meta['origin']}**, {len(y)} échantillons",
        f"- Cible : {artifact['target']}",
        f"- Méthode : {validation['method']}",
        "",
        "| Pli | Échantillons | AUC | Brier | Log-loss |",
        "|---|---|---|---|---|",
        *[f"| {i + 1} | {f['n']} | {f['auc']:.4f} | {f['brier']:.4f} | {f['log_loss']:.4f} |" for i, f in enumerate(folds)],
        "",
        f"- AUC moyenne hors échantillon : **{validation['auc_mean']:.4f}** (0,50 = hasard)",
        f"- Brier moyen : {validation['brier_mean']:.4f} — référence naïve : {validation['baseline_brier']:.4f} — amélioration : {improvement:+.4f}",
        f"- Erreur de calibration (ECE) : {validation['ece']:.4f}",
        f"- Taux de hausses : {validation['positive_rate']:.3f}",
        "",
        "Ces chiffres décrivent des données passées. Ils ne garantissent aucune précision future.",
        "Le verdict d'utilisation est rendu par AEGIS (`aegisd verify-model`), pas par ce script.",
    ]
    if meta["origin"] != "broker":
        lines += ["", "**Modèle entraîné sur des données SYNTHÉTIQUES : inutilisable pour l'exécution, quel que soit son score.**"]
    report = args.report or args.out.with_suffix(".report.md")
    report.write_text("\n".join(lines) + "\n")

    print(f"model      : {args.out}")
    print(f"report     : {report}")
    print(f"samples    : {len(y)}  folds: {len(folds)}  origin: {meta['origin']}")
    print(f"AUC (OOS)  : {validation['auc_mean']:.4f}")
    print(f"Brier      : {validation['brier_mean']:.4f}  (baseline {validation['baseline_brier']:.4f})")
    print(f"ECE        : {validation['ece']:.4f}")
    print("next step  : aegisd verify-model --file <model>   (AEGIS decides whether the model may be used)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
