import { Json } from "../api";
import { Badge, Bar, Panel, cx } from "../components/ui";
import { EmptyBox, Grid, Loaded, Note, PageHeader, StatCard, useView } from "../components/kit";
import { fmtMoney, fmtNum, fmtPct, fmtSigned, fmtTime, tone } from "../format";

export default function Micro({ s }: { s: Json }) {
  const q = useView("micro_targets", 3000);
  const ccy = s?.account?.currency ?? "";
  const g = s?.goal;
  const day = s?.day;
  return (
    <div>
      <PageHeader
        title="Micro-Profit Engine"
        subtitle="Pour chaque décision, chaque objectif net est résolu après spread, commission, slippage, swap et conversion. Une cible sans probabilité calibrée ou d'espérance insuffisante est refusée."
      />
      <Grid cols={4}>
        <StatCard label="Objectif du jour" value={g ? fmtMoney(g.amount, ccy) : "—"} sub={g ? (g.basis === "gross" ? "brut" : "net") : "aucun compte"} accent />
        <StatCard label="Résultat actuel" value={g ? fmtMoney(g.achieved, ccy) : "—"} tone={tone(g?.achieved)} sub={g ? `${fmtNum(g.progress_pct, 0)} %` : undefined} />
        <StatCard label="Montant restant" value={g ? fmtMoney(g.remaining, ccy) : "—"} />
        <StatCard label="Coûts du jour" value={day ? fmtMoney(-(day.commissions + day.swaps + day.fees) + day.spread_cost_est + day.slippage_cost, ccy) : "—"} sub={day ? `${day.closed_trades} clôture(s), ${day.entries_sent} entrée(s)` : undefined} />
      </Grid>
      {g && <div className="mt-3"><Bar value={Math.max(0, g.achieved)} max={g.amount} /></div>}
      <Note>Le moteur n'augmente jamais le risque pour atteindre l'objectif : l'objectif ne peut qu'arrêter les entrées (politique « {g?.policy ?? "—"} »).</Note>
      <div className="mt-4 space-y-4">
        <Loaded q={q}>
          {(tables) =>
            (tables ?? []).length === 0 ? <EmptyBox>Aucune décision encore : la table apparaît au premier signal.</EmptyBox> : tables.map((t: Json) => (
              <Panel key={t.symbol} title={`${t.symbol} — ${String(t.side).toUpperCase()} ${t.volume} lot(s), SL ${t.sl}`} right={<span>{t.timeframe} · {fmtTime(t.ts_ms)}</span>}>
                <table>
                  <thead><tr><th>Objectif net</th><th>TP (points)</th><th>Gain brut</th><th>Spread</th><th>Commission</th><th>Slippage</th><th>Swap</th><th>Perte si SL</th><th>P(TP)</th><th>Point mort</th><th>Espérance</th><th>Verdict</th></tr></thead>
                  <tbody>
                    {t.rows.map((r: Json) => (
                      <tr key={r.target_net}>
                        <td className="num font-medium">{fmtMoney(r.target_net, ccy)}</td>
                        <td className="num">{fmtNum(r.tp_points, 1)}</td>
                        <td className="num">{fmtNum(r.gross_needed)}</td>
                        <td className="num">{fmtNum(r.costs.spread)}</td>
                        <td className="num">{fmtNum(r.costs.commission)}</td>
                        <td className="num">{fmtNum(r.costs.slippage)}</td>
                        <td className="num">{fmtNum(r.costs.swap)}</td>
                        <td className="num text-down-400">{fmtSigned(r.net_if_loss)}</td>
                        <td className="num">{r.probability == null ? "—" : fmtPct(r.probability, 0)}</td>
                        <td className="num">{fmtPct(r.breakeven_probability, 0)}</td>
                        <td className={cx("num", tone(r.expected_net))}>{r.expected_net == null ? "—" : fmtSigned(r.expected_net)}</td>
                        <td className="text-xs"><Badge tone={r.feasible ? "ok" : "muted"}>{r.feasible ? "faisable" : "refusé"}</Badge> <span className="text-fog-400">{r.reason}</span></td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </Panel>
            ))
          }
        </Loaded>
      </div>
    </div>
  );
}
