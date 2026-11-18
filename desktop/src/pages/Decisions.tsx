import { useState } from "react";
import { api, Json } from "../api";
import { Badge, Empty, KV, Panel, cx, usePoll } from "../components/ui";
import { fmtDateTime, fmtNum, fmtPct, fmtSigned, tone } from "../format";

const outcomeTone = (o: string) => (o === "approved" ? "ok" : o === "rejected" ? "bad" : "muted");
const outcomeLabel = (o: string) => (o === "approved" ? "AUTORISÉ" : o === "rejected" ? "REJETÉ" : "OBSERVÉ");

function Detail({ d, ccy }: { d: Json; ccy: string }) {
  const m = d.assessment?.micro;
  const checks: Json[] = [...(d.engine_checks ?? []), ...(d.assessment?.checks ?? [])];
  return (
    <div className="grid gap-5 bg-ink-900/60 p-4 lg:grid-cols-2">
      <div>
        <div className="mb-2 text-[11px] uppercase tracking-wider text-fog-500">Contrôles ({checks.filter((c) => !c.passed).length} échec(s))</div>
        <div className="space-y-1">
          {checks.map((c, i) => (
            <div key={i} className="flex gap-2 text-xs">
              <span className={c.passed ? "text-up-400" : "text-down-400"}>{c.passed ? "✓" : "✗"}</span>
              <span className="w-40 shrink-0 font-mono text-fog-300">{c.name}</span>
              <span className="text-fog-400">{c.detail}</span>
            </div>
          ))}
        </div>
      </div>
      <div className="space-y-4">
        <div>
          <div className="mb-2 text-[11px] uppercase tracking-wider text-fog-500">Signal</div>
          <p className="text-sm">{d.reason}</p>
          <div className="mt-1 text-xs text-fog-500 num">{(d.evidence ?? []).map(([k, v]: [string, number]) => `${k}=${Number(v).toPrecision(5)}`).join(" · ")}</div>
          <div className="mt-1 text-xs num">entrée {d.entry} · SL {d.sl} · TP {d.tp}</div>
        </div>
        {m ? (
          <div>
            <div className="mb-2 text-[11px] uppercase tracking-wider text-fog-500">Micro-profit : brut, coûts, net ({m.volume} lot)</div>
            <KV
              rows={[
                ["Gain brut si TP", fmtSigned(m.gross_win)],
                ["Perte brute si SL", fmtSigned(-m.gross_loss)],
                ["Spread", fmtNum(m.costs.spread)],
                ["Commission", fmtNum(m.costs.commission)],
                ["Slippage estimé", fmtNum(m.costs.slippage)],
                ["Swap", fmtNum(m.costs.swap)],
                ["Conversion", fmtNum(m.costs.conversion)],
                ["Net si TP / si SL", `${fmtSigned(m.net_if_win)} / ${fmtSigned(m.net_if_loss)}`],
                ["P(TP avant SL)", fmtPct(m.win_probability)],
                ["Probabilité d'équilibre", fmtPct(m.breakeven_probability)],
                ["Espérance nette", <span className={tone(m.expected_net)}>{fmtSigned(m.expected_net)} {ccy}</span>],
              ]}
            />
            <div className="mt-2 text-[11px] text-fog-500">Source : {m.probability_source}</div>
          </div>
        ) : (
          <div className="text-xs text-fog-500">Évaluation économique non calculée (contrôles préalables en échec).</div>
        )}
      </div>
    </div>
  );
}

export default function Decisions({ s }: { s: Json | null }) {
  const dec = usePoll(() => api.view("decisions"), 3000);
  const [open, setOpen] = useState<string | null>(null);
  const [filter, setFilter] = useState("all");
  const rows: Json[] = (dec.data ?? []).filter((d: Json) => filter === "all" || d.outcome === filter);
  return (
    <Panel
      title="Décisions (BUY / SELL / WAIT expliqués)"
      pad={false}
      right={
        <select value={filter} onChange={(e) => setFilter(e.target.value)}>
          <option value="all">Toutes</option>
          <option value="approved">Autorisées</option>
          <option value="rejected">Rejetées</option>
          <option value="observed">Observées (MONITOR)</option>
        </select>
      }
    >
      {!rows.length ? (
        <Empty>{dec.error ?? "Aucune décision : les stratégies n'ont pas encore émis de signal."}</Empty>
      ) : (
        <div className="max-h-[78vh] overflow-auto">
          <table>
            <thead><tr><th>Heure</th><th>Instrument</th><th>Stratégie</th><th>Sens</th><th>Mode</th><th>Décision</th><th>P(TP&lt;SL)</th><th>Espérance nette</th><th>Volume</th><th>Premier blocage</th></tr></thead>
            <tbody>
              {rows.map((d) => {
                const fail = [...(d.engine_checks ?? []), ...(d.assessment?.checks ?? [])].find((c: Json) => !c.passed);
                const m = d.assessment?.micro;
                return [
                  <tr key={d.decision_id} className="cursor-pointer" onClick={() => setOpen(open === d.decision_id ? null : d.decision_id)}>
                    <td className="num text-xs">{fmtDateTime(d.ts_ms)}</td>
                    <td>{d.symbol} <span className="text-xs text-fog-500">{d.timeframe}</span></td>
                    <td className="text-xs">{d.strategy_id}</td>
                    <td><Badge tone={d.side === "buy" ? "ok" : "bad"}>{d.side.toUpperCase()}</Badge></td>
                    <td className="text-xs">{d.mode}</td>
                    <td><Badge tone={outcomeTone(d.outcome)}>{outcomeLabel(d.outcome)}</Badge></td>
                    <td className="num">{fmtPct(m?.win_probability)}</td>
                    <td className={cx("num", tone(m?.expected_net))}>{fmtSigned(m?.expected_net)}</td>
                    <td className="num">{d.assessment?.volume || "—"}</td>
                    <td className="max-w-[360px] truncate text-xs text-fog-400">{fail ? `${fail.name}: ${fail.detail}` : "—"}</td>
                  </tr>,
                  open === d.decision_id && (
                    <tr key={d.decision_id + "-d"}>
                      <td colSpan={10} className="p-0"><Detail d={d} ccy={s?.account?.currency ?? ""} /></td>
                    </tr>
                  ),
                ];
              })}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  );
}
