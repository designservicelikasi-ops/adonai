import { useEffect, useState } from "react";
import { api, Json } from "../api";
import { Badge, Button, Empty, KV, Panel, cx, usePoll, useToast } from "../components/ui";
import { fmtDateTime, fmtNum, fmtPct, fmtSigned, tone } from "../format";

const STAGES = [
  ["research", "Recherche"],
  ["demo", "Démo"],
  ["live_approved", "Approuvée réel"],
  ["disabled", "Désactivée"],
] as const;

function StrategyCard({ st, onDone }: { st: Json; onDone: () => void }) {
  const toast = useToast();
  const [cfg, setCfg] = useState(JSON.stringify(st.config, null, 2));
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (!dirty) setCfg(JSON.stringify(st.config, null, 2));
  }, [st.config, dirty]);
  const run = async (type: string, args: Json) => {
    try {
      toast("ok", await api.command(type, args));
      setDirty(false);
      onDone();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };
  const m = st.meta;
  return (
    <Panel
      title={m.name}
      right={
        <>
          <Badge tone={st.stage === "live_approved" ? "bad" : st.stage === "demo" ? "info" : st.stage === "disabled" ? "muted" : "gold"}>{st.stage}</Badge>
          <label className="flex items-center gap-1.5 text-xs">
            <input type="checkbox" className="accent-[#C6A15B]" checked={st.enabled} onChange={(e) => run("set_strategy_enabled", { id: m.id, enabled: e.target.checked })} /> active
          </label>
        </>
      }
    >
      <p className="text-sm text-fog-300">{m.description}</p>
      <div className="mt-3 grid gap-1 text-xs text-fog-400">
        <div><span className="text-fog-500">Entrée : </span>{m.entry_rule}</div>
        <div><span className="text-fog-500">Sortie : </span>{m.exit_rule}</div>
        <div><span className="text-fog-500">Stop / cible : </span>{m.stop_rule} · {m.target_rule}</div>
        <div><span className="text-fog-500">Régimes : </span>{m.regimes.join(", ")} · <span className="text-fog-500">unités : </span>{m.timeframes.join(", ")}</div>
      </div>
      <div className="mt-4 grid gap-4 md:grid-cols-2">
        <div>
          <div className="mb-1 text-[11px] uppercase tracking-wider text-fog-500">Paramètres (JSON validé par le moteur)</div>
          <textarea rows={7} className="w-full font-mono text-xs" value={cfg} onChange={(e) => { setCfg(e.target.value); setDirty(true); }} />
          <div className="mt-2 flex gap-2">
            <Button small kind="primary" disabled={!dirty} onClick={() => { try { run("set_strategy_config", { id: m.id, config: JSON.parse(cfg) }); } catch { toast("bad", "JSON invalide"); } }}>Enregistrer</Button>
            {dirty && <Button small kind="ghost" onClick={() => { setDirty(false); setCfg(JSON.stringify(st.config, null, 2)); }}>Annuler</Button>}
          </div>
          <p className="mt-1 text-[11px] text-fog-500">Changer les paramètres renvoie la stratégie au stade Recherche : ses preuves ne s'appliquent plus.</p>
        </div>
        <div className="space-y-3">
          <div>
            <div className="mb-1 text-[11px] uppercase tracking-wider text-fog-500">Preuves hors échantillon</div>
            {(st.evidence ?? []).length ? (
              st.evidence.map((e: Json) => (
                <div key={e.run_id} className="text-xs num">
                  {e.symbol} {e.timeframe} · {e.trades} trades · {e.wins} gagnants · net {fmtSigned(e.net)} · p {fmtPct(e.win_probability)} <Badge tone={e.origin === "broker" ? "ok" : "warn"}>{e.origin}</Badge>
                </div>
              ))
            ) : (
              <div className="text-xs text-fog-500">Aucun backtest hors échantillon.</div>
            )}
          </div>
          <KV rows={[["Signaux (session)", st.signals], ["Forward réel (trades / gagnants)", `${st.forward.trades} / ${st.forward.wins}`], ["Forward net", fmtSigned(st.forward.net)]]} />
          <div className="flex flex-wrap gap-1.5">
            {STAGES.filter(([id]) => id !== st.stage).map(([id, label]) => (
              <Button key={id} small kind={id === "live_approved" ? "danger" : "default"} onClick={() => run("set_strategy_stage", { id: m.id, stage: id })}>→ {label}</Button>
            ))}
          </div>
        </div>
      </div>
    </Panel>
  );
}

function BacktestPanel({ strategies, specs, onDone }: { strategies: Json[]; specs: Json[]; onDone: () => void }) {
  const toast = useToast();
  const last = usePoll(() => api.view("last_backtest"), 4000);
  const [sid, setSid] = useState("ema_trend");
  const [sym, setSym] = useState("");
  const [tf, setTf] = useState("M15");
  const [origin, setOrigin] = useState("broker");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!sym && specs[0]) setSym(specs[0].symbol);
  }, [specs, sym]);
  const go = async (type: string, args: Json) => {
    setBusy(true);
    try {
      toast("ok", await api.command(type, args));
      onDone();
      last.refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
    }
  };
  const b = last.data;
  return (
    <Panel title="Backtest, hors échantillon et walk-forward">
      <div className="flex flex-wrap items-end gap-2">
        <select value={sid} onChange={(e) => setSid(e.target.value)}>{strategies.map((s) => <option key={s.meta.id} value={s.meta.id}>{s.meta.name}</option>)}</select>
        <select value={sym} onChange={(e) => setSym(e.target.value)}>{specs.map((s) => <option key={s.symbol}>{s.symbol}</option>)}</select>
        <select value={tf} onChange={(e) => setTf(e.target.value)}>{["M1", "M5", "M15", "M30", "H1"].map((x) => <option key={x}>{x}</option>)}</select>
        <select value={origin} onChange={(e) => setOrigin(e.target.value)}><option value="broker">données broker</option><option value="synthetic">données synthétiques</option></select>
        <Button kind="primary" disabled={busy || !sym} onClick={() => go("run_backtest", { strategy_id: sid, symbol: sym, timeframe: tf, origin })}>Lancer</Button>
        {sym.endsWith(".sim") && <Button disabled={busy} onClick={() => go("generate_synthetic", { symbol: sym, timeframe: tf, bars: 20000, seed: 42 })}>Générer 20 000 bougies synthétiques</Button>}
      </div>
      <p className="mt-2 text-[11px] text-fog-500">Un backtest décrit le passé sous des hypothèses de coûts déclarées ; ce n'est pas une prédiction. Les données synthétiques ne servent qu'à tester le logiciel.</p>
      {b ? (
        <div className="mt-4">
          <div className="mb-2 text-xs text-fog-400 num">{b.run.run_id} · {b.run.strategy_id} {b.run.symbol} {b.run.timeframe} · {fmtDateTime(b.run.ts_ms)} <Badge tone={b.run.origin === "broker" ? "ok" : "warn"}>{b.run.origin}</Badge></div>
          <table>
            <thead><tr><th>Segment</th><th>Bougies</th><th>Trades</th><th>Win rate</th><th>Net</th><th>PF</th><th>Espérance</th><th>DD max</th></tr></thead>
            <tbody>
              {[{ segment: "complet", bars: null, metrics: b.full }, ...b.segments, ...b.walk_forward].map((x: Json, i: number) => (
                <tr key={i}>
                  <td>{x.segment}</td><td className="num">{x.bars ?? "—"}</td><td className="num">{x.metrics.trades}</td><td className="num">{fmtPct(x.metrics.win_rate)}</td>
                  <td className={cx("num", tone(x.metrics.net_profit))}>{fmtSigned(x.metrics.net_profit)}</td><td className="num">{x.metrics.profit_factor === null ? "—" : fmtNum(x.metrics.profit_factor)}</td>
                  <td className={cx("num", tone(x.metrics.expectancy))}>{fmtSigned(x.metrics.expectancy, 3)}</td><td className="num">{fmtNum(x.metrics.max_drawdown_pct)} %</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <Empty>Aucun backtest dans cette session.</Empty>
      )}
    </Panel>
  );
}

export default function Strategies({ s, refresh }: { s: Json | null; refresh: () => void }) {
  const st = usePoll(() => api.view("strategies"), 4000);
  const done = () => { st.refresh(); refresh(); };
  if (!st.data) return <Empty>{st.error ?? "Chargement…"}</Empty>;
  return (
    <div className="space-y-5">
      <BacktestPanel strategies={st.data} specs={s?.specs ?? []} onDone={done} />
      <div className="grid gap-5 2xl:grid-cols-2">
        {st.data.map((x: Json) => <StrategyCard key={x.meta.id} st={x} onDone={done} />)}
      </div>
    </div>
  );
}
