import { useEffect, useState } from "react";
import { Json } from "../api";
import { Badge, Button, KV, Panel, cx } from "../components/ui";
import { EmptyBox, Loaded, Note, PageHeader, SeriesSelect, parseSeriesKey, seriesKey, useCommand, useStoredSeries, useView } from "../components/kit";
import { fmtDateTime, fmtNum, fmtSigned, tone } from "../format";

/** "fast": "10, 20" → { fast: [10, 20] }; values are parsed as JSON when possible. */
function parseGrid(rows: { key: string; values: string }[]): Record<string, Json[]> | string {
  const grid: Record<string, Json[]> = {};
  for (const r of rows) {
    if (!r.key.trim()) continue;
    const vals = r.values.split(",").map((v) => v.trim()).filter(Boolean).map((v) => { try { return JSON.parse(v); } catch { return v; } });
    if (!vals.length) return `aucune valeur pour « ${r.key} »`;
    grid[r.key.trim()] = vals;
  }
  return Object.keys(grid).length ? grid : "ajoutez au moins un paramètre";
}

const combos = (g: Record<string, Json[]> | string) => (typeof g === "string" ? 0 : Object.values(g).reduce((n, v) => n * v.length, 1));

function Sweep({ r }: { r: Json }) {
  const pts: Json[] = [...(r.points ?? [])].sort((a, b) => (b.holdout?.net_profit ?? -Infinity) - (a.holdout?.net_profit ?? -Infinity));
  return (
    <Panel title={`${r.strategy_id} — ${r.evaluated}/${r.combinations} combinaison(s)`} right={<span className="num">{r.in_sample_bars} + {r.holdout_bars} bougies · {r.threads} fil(s) · {r.elapsed_ms} ms</span>} className="mb-4">
      {(r.warnings ?? []).map((w: string, i: number) => <div key={i} className="mb-2"><Note tone="warn">{w}</Note></div>)}
      <div className="max-h-80 overflow-y-auto">
        <table>
          <thead><tr><th>Paramètres</th><th>Apprentissage</th><th>Plis positifs</th><th>Écart des plis</th><th>Contrôle (holdout)</th><th>Diagnostic</th></tr></thead>
          <tbody>
            {pts.map((p, i) => (
              <tr key={i}>
                <td className="font-mono text-[11px]">{Object.entries(p.config ?? {}).map(([k, v]) => `${k}=${v}`).join(" ")}</td>
                <td className={cx("num", tone(p.in_sample?.net_profit))}>{fmtSigned(p.in_sample?.net_profit)} <span className="text-fog-500">({p.in_sample?.trades})</span></td>
                <td className="num">{p.positive_folds}/{(p.fold_nets ?? []).length}</td>
                <td className="num">{fmtNum(p.fold_spread, 1)}</td>
                <td className={cx("num font-medium", tone(p.holdout?.net_profit))}>{fmtSigned(p.holdout?.net_profit)} <span className="font-normal text-fog-500">({p.holdout?.trades})</span></td>
                <td className="space-x-1">
                  {p.overfit && <Badge tone="bad">sur-appris</Badge>}
                  {p.unstable && <Badge tone="warn">instable</Badge>}
                  {!p.overfit && !p.unstable && <Badge tone="ok">stable</Badge>}
                  {(p.notes ?? []).length > 0 && <span className="text-[11px] text-fog-400">{p.notes.join(" ; ")}</span>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

export default function Learning({ s, refresh }: { s: Json; refresh: () => void }) {
  const q = useView("learning", 3000);
  const models = useView("probability_models", 10000);
  const full = useView("strategies", 15000); // the summary has no parameters
  const series = useStoredSeries();
  const strategies: Json[] = s?.strategies ?? [];
  const [strategy, setStrategy] = useState("");
  const [target, setTarget] = useState("");
  const [rows, setRows] = useState<{ key: string; values: string }[]>([]);
  const { run, busy } = useCommand(() => { q.refresh(); models.refresh(); refresh(); });
  useEffect(() => { if (!strategy && strategies.length) setStrategy(strategies[0].id); }, [strategies.length]);
  useEffect(() => { if (!target && series.length) setTarget(seriesKey(series.reduce((a, b) => (b.bars > a.bars ? b : a)))); }, [series.length]);
  // Suggest the strategy's own parameters (current value only) when it changes.
  useEffect(() => {
    const cfg = ((full.data ?? []) as Json[]).find((x) => x.meta?.id === strategy)?.config ?? {};
    setRows(Object.entries(cfg).filter(([, v]) => typeof v === "number").slice(0, 3).map(([k, v]) => ({ key: k, values: String(v) })));
  }, [strategy, !!full.data]);
  const grid = parseGrid(rows);
  const max = q.data?.settings?.max_sweep_combinations ?? 0;
  const n = combos(grid);
  return (
    <div>
      <PageHeader
        title="Fast Learning"
        subtitle="Recherche de paramètres en parallèle (walk-forward sur la partie d'apprentissage, contrôle sur une partie jamais vue), audit de la qualité des données, surveillance de la dérive des modèles. La recherche PROPOSE ; elle ne change jamais une stratégie en service."
        right={q.data?.sweep_running ? <Badge tone="gold" dot>recherche en cours</Badge> : undefined}
      />
      <div className="grid gap-4 xl:grid-cols-3">
        <Panel title="Recherche de paramètres" className="xl:col-span-2">
          <div className="mb-3 flex flex-wrap gap-2">
            <select value={strategy} onChange={(e) => setStrategy(e.target.value)} aria-label="stratégie">
              {strategies.map((x: Json) => <option key={x.id} value={x.id}>{x.name}</option>)}
            </select>
            <SeriesSelect series={series} value={target} onChange={setTarget} minBars={500} />
          </div>
          <div className="space-y-2">
            {rows.map((r, i) => (
              <div key={i} className="flex gap-2">
                <input className="w-40 font-mono" value={r.key} placeholder="paramètre" onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, key: e.target.value } : x)))} />
                <input className="flex-1 num" value={r.values} placeholder="valeurs séparées par des virgules, ex. 10, 20, 30" onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, values: e.target.value } : x)))} />
                <Button small kind="ghost" onClick={() => setRows(rows.filter((_, j) => j !== i))} title="retirer">✕</Button>
              </div>
            ))}
            <Button small onClick={() => setRows([...rows, { key: "", values: "" }])}>+ paramètre</Button>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button kind="primary" disabled={busy || q.data?.sweep_running || typeof grid === "string" || !target || (max > 0 && n > max)} onClick={() => typeof grid !== "string" && run("run_parameter_sweep", { strategy_id: strategy, ...parseSeriesKey(target), grid })}>
              Lancer la recherche
            </Button>
            <span className={cx("text-xs", max > 0 && n > max ? "text-down-400" : "text-fog-400")}>
              {typeof grid === "string" ? grid : `${n} combinaison(s)${max ? ` — maximum ${max}` : ""}`}
            </span>
          </div>
          <p className="mt-2 text-xs text-fog-400">Les paramètres non listés gardent leur valeur actuelle. 70 % des bougies servent à l'apprentissage (4 plis), les 30 % suivants au contrôle.</p>
        </Panel>
        <Panel title="Réglages et machine">
          <Loaded q={q}>
            {(l: Json) => (
              <KV rows={[
                ["Fils de recherche", `${l.settings.research_threads} / ${l.machine_threads} disponibles`],
                ["Combinaisons max", l.settings.max_sweep_combinations],
                ["Retour arrière automatique", l.settings.auto_rollback ? "oui" : "non"],
                ["Dérive : transactions min", l.settings.drift_min_trades],
                ["Dérive : tolérance Brier", `+ ${fmtNum(l.settings.drift_brier_tolerance, 2)}`],
              ]} />
            )}
          </Loaded>
        </Panel>
      </div>

      <h2 className="mb-2 mt-6 text-sm font-semibold text-fog-300">Résultats des recherches</h2>
      <Loaded q={q}>
        {(l: Json) => ((l.sweeps ?? []).length === 0 ? <EmptyBox>Aucune recherche dans cette session du moteur.</EmptyBox> : <>{l.sweeps.map((r: Json, i: number) => <Sweep key={i} r={r} />)}</>)}
      </Loaded>

      <div className="mt-4 grid gap-4 xl:grid-cols-2">
        <Panel title="Qualité des données">
          <div className="mb-3 flex gap-2">
            <SeriesSelect series={series} value={target} onChange={setTarget} />
            <Button disabled={busy || !target} onClick={() => run("audit_data", parseSeriesKey(target))}>Auditer</Button>
          </div>
          <Loaded q={q}>
            {(l: Json) => ((l.audits ?? []).length === 0 ? <EmptyBox>Aucun audit.</EmptyBox> : (
              <table>
                <thead><tr><th>Historique</th><th>Bougies</th><th>Manquantes</th><th>Doublons</th><th>Aberrantes</th><th>Verdict</th></tr></thead>
                <tbody>
                  {l.audits.map(([sym, tf, origin, a]: [string, string, string, Json], i: number) => (
                    <tr key={i} title={(a.outliers ?? []).map(([t, d]: [number, string]) => `${fmtDateTime(t)} : ${d}`).join("\n")}>
                      <td className="text-xs">{sym} {tf} <span className="text-fog-500">{origin}</span></td>
                      <td className="num">{a.bars}</td>
                      <td className="num">{a.missing_bars}</td>
                      <td className="num">{a.duplicates}</td>
                      <td className="num">{(a.outliers ?? []).length}</td>
                      <td><Badge tone={a.verdict === "good" ? "ok" : a.verdict === "usable" ? "warn" : "bad"}>{({ good: "bonne", usable: "utilisable", poor: "insuffisante" } as Record<string, string>)[a.verdict] ?? a.verdict}</Badge></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ))}
          </Loaded>
        </Panel>
        <Panel title="Dérive et retours arrière">
          <Loaded q={q}>
            {(l: Json) => (
              <>
                {(l.drift ?? []).length === 0 ? <EmptyBox>Pas encore assez de transactions réelles pour mesurer une dérive.</EmptyBox> : (
                  <ul className="space-y-1.5 text-sm">
                    {l.drift.map((d: Json) => (
                      <li key={d.model_id} className="flex flex-wrap items-center gap-2">
                        <Badge tone={d.drifting ? "bad" : "ok"}>{d.drifting ? "dérive" : "stable"}</Badge>
                        <span className="font-mono text-xs">{d.model_id}</span>
                        <span className="num text-xs text-fog-400">Brier {fmtNum(d.live_brier, 4)} contre {fmtNum(d.reference_brier, 4)} · {d.trades} transactions</span>
                        <span className="text-xs text-fog-400">{d.detail}</span>
                      </li>
                    ))}
                  </ul>
                )}
                {(l.rollbacks ?? []).length > 0 && (
                  <div className="mt-3 space-y-1">{l.rollbacks.map((r: string, i: number) => <Note key={i} tone="warn">{r}</Note>)}</div>
                )}
              </>
            )}
          </Loaded>
        </Panel>
      </div>

      <Panel title="Modèles de probabilité" className="mt-4" pad={false}>
        {(models.data ?? []).length === 0 ? <EmptyBox>Aucun modèle entraîné.</EmptyBox> : (
          <table>
            <thead><tr><th>Modèle</th><th>Données</th><th>Achat</th><th>Vente</th><th>Utilisable</th></tr></thead>
            <tbody>
              {(models.data as Json[]).map((m) => (
                <tr key={m.model_id}>
                  <td><div className="font-mono text-xs">{m.model_id}</div><div className="num text-[11px] text-fog-500">{fmtDateTime(m.created_ms)}</div></td>
                  <td className="text-xs">{m.symbol} {m.timeframe} · {m.bars} bougies {m.origin === "synthetic" && <Badge tone="warn">synthétique</Badge>}</td>
                  <td className="num text-xs">Brier {fmtNum(m.buy_report?.brier, 4)} · ECE {fmtNum(m.buy_report?.ece, 3)}</td>
                  <td className="num text-xs">Brier {fmtNum(m.sell_report?.brier, 4)} · ECE {fmtNum(m.sell_report?.ece, 3)}</td>
                  <td>{m.usable ? <Badge tone="ok">oui</Badge> : <Badge tone="muted">non</Badge>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
    </div>
  );
}
