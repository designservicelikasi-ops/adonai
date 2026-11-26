import { useEffect, useState } from "react";
import { Json } from "../api";
import { TimeChart } from "../components/Chart";
import { Badge, Button, Panel, cx } from "../components/ui";
import { EmptyBox, Grid, Note, PageHeader, SeriesSelect, StatCard, Tabs, parseSeriesKey, seriesKey, useCommand, useStoredSeries, useView } from "../components/kit";
import { fmtDateTime, fmtMoney, fmtNum, fmtPct, fmtPrice, fmtSigned, tone } from "../format";

const SEGMENT: Record<string, string> = { train: "apprentissage", validation: "validation", oos: "hors échantillon", test: "test" };

function MetricsRow({ name, m, bars }: { name: string; m: Json; bars?: number }) {
  return (
    <tr>
      <td className="text-xs">{SEGMENT[name] ?? name}</td>
      <td className="num text-xs">{bars ?? "—"}</td>
      <td className="num">{m.trades}</td>
      <td className="num">{fmtPct(m.win_rate, 0)}</td>
      <td className={cx("num", tone(m.net_profit))}>{fmtSigned(m.net_profit)}</td>
      <td className="num">{m.profit_factor == null ? "—" : fmtNum(m.profit_factor, 2)}</td>
      <td className="num">{fmtNum(m.max_drawdown_pct, 2)} %</td>
      <td className={cx("num", tone(m.expectancy))}>{fmtSigned(m.expectancy)}</td>
    </tr>
  );
}

const HEAD = <thead><tr><th>Segment</th><th>Bougies</th><th>Transactions</th><th>Gagnantes</th><th>Net</th><th>Facteur de profit</th><th>DD max</th><th>Espérance</th></tr></thead>;

export default function Backtesting({ s, refresh }: { s: Json; refresh: () => void }) {
  const last = useView("last_backtest", 5000);
  const runs = useView("backtests", 8000);
  const series = useStoredSeries();
  const [strategy, setStrategy] = useState("");
  const [target, setTarget] = useState("");
  const [tab, setTab] = useState<"segments" | "trades">("segments");
  const { run, busy } = useCommand(() => { last.refresh(); runs.refresh(); refresh(); });
  const strategies: Json[] = s?.strategies ?? [];
  useEffect(() => { if (!strategy && strategies.length) setStrategy(strategies[0].id); }, [strategies.length]);
  useEffect(() => { if (!target && series.length) setTarget(seriesKey(series.reduce((a, b) => (b.bars > a.bars ? b : a)))); }, [series.length]);
  const b = last.data;
  const synthetic = b?.run?.origin === "synthetic";
  const ccy = s?.account?.currency ?? "";
  return (
    <div>
      <PageHeader
        title="Backtesting"
        subtitle="Rejoue une stratégie sur un historique stocké, avec spread, commission, slippage et swaps. Découpage apprentissage / validation / hors échantillon, puis walk-forward. Reproductible : mêmes données, mêmes paramètres, même résultat."
      />
      <Panel title="Lancer un backtest" className="mb-4">
        <div className="flex flex-wrap items-center gap-2">
          <select value={strategy} onChange={(e) => setStrategy(e.target.value)} aria-label="stratégie">
            {strategies.map((x: Json) => <option key={x.id} value={x.id}>{x.name}</option>)}
          </select>
          <SeriesSelect series={series} value={target} onChange={setTarget} minBars={200} />
          <Button kind="primary" disabled={busy || !strategy || !target} onClick={() => run("run_backtest", { strategy_id: strategy, ...parseSeriesKey(target) })}>{busy ? "Calcul…" : "Lancer"}</Button>
        </div>
        <p className="mt-2 text-xs text-fog-400">Paramètres actuels de la stratégie et coûts par défaut du backtest (rappelés au-dessus des résultats). Un résultat sur données SYNTHÉTIQUES vérifie le logiciel ; il ne dit rien d'un marché réel.</p>
      </Panel>

      {!b ? <EmptyBox>Aucun backtest dans cette session du moteur.</EmptyBox> : (
        <>
          <div className="mb-3 flex flex-wrap items-center gap-2 text-sm">
            <span className="font-medium">{b.run.strategy_id}</span>
            <span className="num text-fog-400">{b.run.symbol} {b.run.timeframe} · {b.run.params?.bars ?? "—"} bougies · {fmtDateTime(b.run.ts_ms)}</span>
            {synthetic ? <Badge tone="warn">DONNÉES SYNTHÉTIQUES</Badge> : <Badge tone="info">{b.run.origin}</Badge>}
            <span className="font-mono text-xs text-fog-500">{b.run.run_id}</span>
          </div>
          {b.run.params?.backtest && (
            <p className="mb-3 text-xs text-fog-400">
              Capital {fmtMoney(b.run.params.backtest.initial_equity, ccy)} · risque {fmtNum(b.run.params.backtest.risk_per_trade_pct, 2)} % par transaction · commission {fmtNum(b.run.params.backtest.commission_per_lot_round_turn, 2)} par lot aller-retour · slippage {fmtNum(b.run.params.backtest.slippage_points, 0)} points · spread {b.run.params.backtest.spread_points == null ? "des bougies" : `${b.run.params.backtest.spread_points} points`} · swaps {b.run.params.backtest.apply_swaps ? "appliqués" : "ignorés"}
            </p>
          )}
          <Grid cols={5}>
            <StatCard label="Résultat net" value={fmtSigned(b.full.net_profit)} tone={tone(b.full.net_profit)} sub={`final ${fmtMoney(b.full.final_equity, ccy)}`} accent />
            <StatCard label="Transactions" value={b.full.trades} sub={`${fmtPct(b.full.win_rate, 0)} gagnantes`} />
            <StatCard label="Facteur de profit" value={b.full.profit_factor == null ? "—" : fmtNum(b.full.profit_factor, 2)} sub={`espérance ${fmtSigned(b.full.expectancy)}`} />
            <StatCard label="Drawdown max" value={`${fmtNum(b.full.max_drawdown_pct, 2)} %`} sub={fmtMoney(b.full.max_drawdown, ccy)} />
            <StatCard label="Frais" value={fmtMoney((b.full.total_commission ?? 0) + (b.full.total_swap ?? 0), ccy)} sub={`commission ${fmtSigned(b.full.total_commission)} · swap ${fmtSigned(b.full.total_swap)}`} />
          </Grid>
          <Panel title="Courbe d'équité" className="mt-4">
            {(b.equity_curve ?? []).length < 2 ? <EmptyBox>Pas assez de transactions pour une courbe.</EmptyBox> : (
              <TimeChart points={b.equity_curve.map(([t, v]: [number, number]) => ({ t, v }))} priceDigits={2} baseline={b.equity_curve[0][1]} height={240} />
            )}
          </Panel>
          <div className="mt-4">
            <Tabs value={tab} onChange={setTab} items={[["segments", "Segments et walk-forward"], ["trades", `Transactions (${(b.trades ?? []).length})`]]} />
            {tab === "segments" ? (
              <div className="grid gap-4 xl:grid-cols-2">
                <Panel title="Découpage temporel" pad={false}>
                  <table>{HEAD}<tbody>{(b.segments ?? []).map((x: Json) => <MetricsRow key={x.segment} name={x.segment} m={x.metrics} bars={x.bars} />)}</tbody></table>
                </Panel>
                <Panel title="Walk-forward" pad={false}>
                  {(b.walk_forward ?? []).length === 0 ? <EmptyBox>Historique trop court pour un walk-forward.</EmptyBox> : (
                    <table>{HEAD}<tbody>{b.walk_forward.map((x: Json) => <MetricsRow key={x.segment} name={x.segment} m={x.metrics} bars={x.bars} />)}</tbody></table>
                  )}
                </Panel>
              </div>
            ) : (
              <Panel pad={false}>
                <div className="max-h-[480px] overflow-y-auto">
                  <table>
                    <thead><tr><th>Entrée</th><th>Sortie</th><th>Sens</th><th>Volume</th><th>Prix</th><th>Net</th><th>Frais</th><th>Motif de sortie</th><th>Raison d'entrée</th></tr></thead>
                    <tbody>
                      {(b.trades ?? []).map((t: Json, i: number) => (
                        <tr key={i}>
                          <td className="num text-xs">{fmtDateTime(t.entry_ts)}</td>
                          <td className="num text-xs">{fmtDateTime(t.exit_ts)}</td>
                          <td><Badge tone={t.side === "buy" ? "ok" : "bad"}>{String(t.side).toUpperCase()}</Badge></td>
                          <td className="num">{t.volume}</td>
                          <td className="num text-xs">{fmtPrice(t.entry)} → {fmtPrice(t.exit)}</td>
                          <td className={cx("num", tone(t.net))}>{fmtSigned(t.net)}</td>
                          <td className="num text-xs">{fmtSigned((t.commission ?? 0) + (t.swap ?? 0))}</td>
                          <td className="font-mono text-xs">{t.exit_reason}</td>
                          <td className="text-xs text-fog-400">{t.reason}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Panel>
            )}
          </div>
        </>
      )}

      <Panel title="Backtests enregistrés" className="mt-4" pad={false}>
        {(runs.data ?? []).length === 0 ? <EmptyBox>Aucun backtest enregistré.</EmptyBox> : (
          <div className="max-h-80 overflow-y-auto">
            <table>
              <thead><tr><th>Date</th><th>Stratégie</th><th>Données</th><th>Transactions</th><th>Net</th><th>Hors échantillon</th></tr></thead>
              <tbody>
                {(runs.data as Json[]).map((r) => {
                  const full = r.metrics?.full ?? {};
                  const oos = (r.metrics?.segments ?? []).find((x: Json) => x.segment === "oos")?.metrics;
                  return (
                    <tr key={r.run_id}>
                      <td className="num text-xs">{fmtDateTime(r.ts_ms)}</td>
                      <td className="font-mono text-xs">{r.strategy_id}</td>
                      <td className="text-xs">{r.symbol} {r.timeframe} {r.origin === "synthetic" ? <Badge tone="warn">synthétique</Badge> : <Badge tone="info">{r.origin}</Badge>}</td>
                      <td className="num">{full.trades ?? "—"}</td>
                      <td className={cx("num", tone(full.net_profit))}>{fmtSigned(full.net_profit)}</td>
                      <td className={cx("num", tone(oos?.net_profit))}>{oos ? `${fmtSigned(oos.net_profit)} (${oos.trades})` : "—"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      <div className="mt-4"><Note>Une stratégie ne peut être promue qu'avec au moins 30 transactions hors échantillon ; un backtest sur données synthétiques ne la rend jamais éligible au réel.</Note></div>
    </div>
  );
}
