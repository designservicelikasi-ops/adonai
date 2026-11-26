import { useMemo, useState } from "react";
import { api, Json } from "../api";
import { TimeChart } from "../components/Chart";
import { Badge, Empty, Panel, Stat, cx, usePoll } from "../components/ui";
import { fmtNum, fmtPct, fmtSigned, tone } from "../format";

function StatsGrid({ st }: { st: Json }) {
  if (!st || !st.trades) return <Empty>Aucune transaction clôturée sur cette période.</Empty>;
  return (
    <div className="grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-4 xl:grid-cols-7">
      <Stat label="Profit brut" value={fmtSigned(st.gross_profit)} valueClass="text-up-400" />
      <Stat label="Perte brute" value={fmtSigned(st.gross_loss)} valueClass="text-down-400" />
      <Stat label="Résultat net" value={fmtSigned(st.net)} valueClass={tone(st.net)} />
      <Stat label="Transactions" value={st.trades} sub={`${st.wins} gagnantes`} />
      <Stat label="Win rate" value={fmtPct(st.win_rate)} />
      <Stat label="Profit factor (net)" value={st.profit_factor === null ? "—" : fmtNum(st.profit_factor)} valueClass={st.profit_factor !== null && st.profit_factor < 1 ? "text-down-400" : ""} />
      <Stat label="Espérance / trade" value={fmtSigned(st.expectancy, 3)} valueClass={tone(st.expectancy)} />
      <Stat label="Gain moyen" value={fmtSigned(st.avg_win, 3)} valueClass="text-up-400" />
      <Stat label="Perte moyenne" value={fmtSigned(st.avg_loss, 3)} valueClass="text-down-400" />
      <Stat label="Coût moyen / trade" value={fmtNum(st.avg_cost, 3)} valueClass="text-warn-400" />
      <Stat label="Coûts / gains bruts" value={fmtPct(st.cost_share)} valueClass={(st.cost_share ?? 0) > 0.5 ? "text-down-400" : ""} />
      <Stat label="Commissions" value={fmtSigned(st.commission)} />
      <Stat label="Spread (estimé)" value={fmtSigned(-st.est_spread_cost)} />
      <Stat label="Efficacité des sorties" value={fmtPct(st.exit_efficiency)} sub="part du meilleur mouvement conservée" />
    </div>
  );
}

function StatsTable({ rows, keyLabel }: { rows: Json[]; keyLabel: string }) {
  if (!rows?.length) return <Empty>Aucune donnée.</Empty>;
  return (
    <div className="max-h-[420px] overflow-auto">
      <table>
        <thead>
          <tr><th>{keyLabel}</th><th>Trades</th><th>Win rate</th><th>Brut</th><th>Coûts</th><th>Net</th><th>PF</th><th>Espérance</th></tr>
        </thead>
        <tbody>
          {[...rows].reverse().map((r) => (
            <tr key={r.key}>
              <td className="num">{r.key}</td>
              <td className="num">{r.trades}</td>
              <td className="num">{fmtPct(r.win_rate)}</td>
              <td className={cx("num", tone(r.gross))}>{fmtSigned(r.gross)}</td>
              <td className="num text-warn-400">{fmtNum((r.avg_cost ?? 0) * r.trades)}</td>
              <td className={cx("num font-medium", tone(r.net))}>{fmtSigned(r.net)}</td>
              <td className="num">{r.profit_factor === null ? "—" : fmtNum(r.profit_factor)}</td>
              <td className={cx("num", tone(r.expectancy))}>{fmtSigned(r.expectancy, 3)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const HORIZONS = [
  ["today", "Aujourd'hui"],
  ["last_7_days", "7 jours"],
  ["last_30_days", "30 jours"],
  ["all_time", "Historique"],
] as const;
const GROUPS = [
  ["hourly", "Par heure", "Heure (UTC)"],
  ["daily", "Par jour", "Jour"],
  ["weekly", "Par semaine", "Semaine du"],
  ["monthly", "Par mois", "Mois"],
  ["by_strategy", "Par stratégie", "Stratégie"],
  ["by_symbol", "Par instrument", "Instrument"],
  ["by_hour", "Par heure d'ouverture", "Heure (UTC)"],
  ["by_regime", "Par régime", "Régime"],
] as const;

export default function Performance({ s }: { s: Json | null }) {
  const p = usePoll(() => api.view("performance"), 5000);
  const [h, setH] = useState<(typeof HORIZONS)[number][0]>("today");
  const [g, setG] = useState<(typeof GROUPS)[number][0]>("daily");
  const perf = p.data;
  const curve = useMemo(() => (perf?.equity_curve ?? []).map((x: [number, number]) => ({ t: x[0], v: x[1] })), [perf]);
  if (!perf) return <Empty>{p.error ?? "Chargement des performances…"}</Empty>;
  const group = GROUPS.find((x) => x[0] === g)!;
  return (
    <div className="space-y-5">
      <Panel
        title="Synthèse (transactions AEGIS clôturées, net = après commission, swap et frais)"
        right={
          <div className="flex gap-1">
            {HORIZONS.map(([id, label]) => (
              <button key={id} onClick={() => setH(id)} className={cx("rounded-md px-2.5 py-1 text-xs", h === id ? "bg-aegis-500/15 text-aegis-300" : "text-fog-400 hover:bg-ink-750")}>{label}</button>
            ))}
          </div>
        }
      >
        <StatsGrid st={perf[h]} />
      </Panel>
      <div className="grid gap-5 xl:grid-cols-3">
        <Panel title="Résultat net cumulé" className="xl:col-span-2" right={<span className="num">drawdown max {fmtNum(perf.max_drawdown)} {s?.account?.currency ?? ""}</span>}>
          {curve.length > 1 ? <TimeChart points={curve} kind="area" color="#C6A15B" baseline={0} priceDigits={2} /> : <Empty>La courbe apparaît après deux transactions clôturées.</Empty>}
        </Panel>
        <Panel title="Long terme">
          <div className="space-y-4">
            <Stat label="Rendement ajusté au risque (Sharpe journalier annualisé)" value={perf.daily_sharpe === null ? "—" : fmtNum(perf.daily_sharpe)} sub="calculé à partir de 10 jours d'activité" />
            <Stat label="Transactions par jour actif" value={perf.avg_trades_per_day === null ? "—" : fmtNum(perf.avg_trades_per_day, 1)} />
            <div>
              <div className="mb-1 text-[11px] uppercase tracking-wider text-fog-500">Dégradation détectée</div>
              {(perf.degradation ?? []).length ? perf.degradation.map((d: string) => <div key={d} className="mb-1 text-xs text-down-400">{d}</div>) : <Badge tone="ok">aucune</Badge>}
            </div>
          </div>
        </Panel>
      </div>
      <Panel
        title={group[1]}
        pad={false}
        right={
          <select value={g} onChange={(e) => setG(e.target.value as typeof g)}>
            {GROUPS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select>
        }
      >
        <StatsTable rows={perf[g]} keyLabel={group[2]} />
      </Panel>
    </div>
  );
}
