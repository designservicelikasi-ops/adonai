import { useState } from "react";
import { api, Json } from "../api";
import { Badge, Button, Empty, KV, Panel, cx, usePoll, useToast } from "../components/ui";
import { ago, fmtDateTime, fmtNum, fmtPct, fmtSigned, tone } from "../format";

/** Reliability diagram: predicted probability (x) vs observed frequency (y). */
function Reliability({ report, size = 180 }: { report: Json; size?: number }) {
  const bins: Json[] = (report?.bins ?? []).filter((b: Json) => b.count > 0);
  const max = Math.max(1, ...bins.map((b) => b.count));
  const p = (v: number) => 12 + v * (size - 24);
  return (
    <svg width={size} height={size} className="rounded-md bg-ink-950">
      <line x1={p(0)} y1={p(1)} x2={p(1)} y2={p(0)} stroke="#BFB39A" strokeDasharray="4 4" />
      <rect x={p(0)} y={p(0)} width={size - 24} height={size - 24} fill="none" stroke="#E8E3D8" />
      {bins.map((b, i) => (
        <circle key={i} cx={p(b.mean_predicted)} cy={p(1 - b.observed_rate)} r={3 + 6 * Math.sqrt(b.count / max)} fill="#C6A15B" fillOpacity={0.75} stroke="#9A7839" strokeWidth={0.5}>
          <title>{`prévu ${(b.mean_predicted * 100).toFixed(1)} % · observé ${(b.observed_rate * 100).toFixed(1)} % · ${b.count} obs.`}</title>
        </circle>
      ))}
      <text x={p(0)} y={size - 2} fontSize="9" fill="#66768A">prévu →</text>
      <text x={2} y={10} fontSize="9" fill="#66768A">observé</text>
    </svg>
  );
}

function Report({ title, r, selected, reason }: { title: string; r: Json; selected?: string; reason?: string }) {
  return (
    <div className="flex gap-4">
      <Reliability report={r} />
      <div className="min-w-0 flex-1">
        <div className="mb-2 flex items-center gap-2 text-sm font-medium">
          {title}
          {selected && <Badge tone={selected === "full" ? "ok" : selected === "scenario_only" ? "info" : "bad"}>{selected === "full" ? "modèle complet" : selected === "scenario_only" ? "scénario seul" : "inutilisable"}</Badge>}
        </div>
        <KV rows={[["Observations hors échantillon", r?.n ?? "—"], ["Brier", fmtNum(r?.brier, 4)], ["Log loss", fmtNum(r?.log_loss, 4)], ["ECE", fmtNum(r?.ece, 3)], ["Prévu moyen / observé", `${fmtPct(r?.mean_predicted)} / ${fmtPct(r?.observed_rate)}`], ["Brier par pli", (r?.fold_brier ?? []).map((x: number) => x.toFixed(4)).join(" · ") || "—"]]} />
        {reason && <p className="mt-2 text-[11px] text-fog-500">{reason}</p>}
      </div>
    </div>
  );
}

function ProbabilityPanel({ s }: { s: Json | null }) {
  const toast = useToast();
  const models = usePoll(() => api.view("probability_models"), 5000);
  const live = usePoll(() => api.view("probability_live"), 5000);
  const [sym, setSym] = useState("");
  const [tf, setTf] = useState("M5");
  const [origin, setOrigin] = useState("broker");
  const [rollback, setRollback] = useState("");
  const [busy, setBusy] = useState(false);
  const go = async (type: string, args: Json) => {
    setBusy(true);
    try {
      toast("ok", await api.command(type, args));
      models.refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
    }
  };
  const specs: Json[] = s?.specs ?? [];
  return (
    <div className="space-y-5">
      <Panel title="Probability Engine — P(TP avant SL), pas une probabilité directionnelle">
        <p className="mb-3 text-xs text-fog-400">
          Étiquettes « triple barrière » sur l'historique (TP et SL dans la même bougie = perte), régression logistique validée en walk-forward, calibration croisée. Le modèle complet n'est utilisé que s'il bat le modèle de référence (scénario seul) ; sinon le scénario calibré ; sinon aucune probabilité et aucune entrée.
        </p>
        <div className="flex flex-wrap items-end gap-2">
          <select value={sym} onChange={(e) => setSym(e.target.value)}><option value="">instrument…</option>{specs.map((x) => <option key={x.symbol}>{x.symbol}</option>)}</select>
          <select value={tf} onChange={(e) => setTf(e.target.value)}>{["M1", "M5", "M15", "M30", "H1"].map((x) => <option key={x}>{x}</option>)}</select>
          <select value={origin} onChange={(e) => setOrigin(e.target.value)}><option value="broker">données broker</option><option value="synthetic">données synthétiques</option></select>
          <Button kind="primary" disabled={busy || !sym} onClick={() => go("train_probability", { symbol: sym, timeframe: tf, origin })}>Entraîner une nouvelle version</Button>
          <span className="flex-1" />
          <input placeholder="identifiant de version" className="num w-56 text-xs" value={rollback} onChange={(e) => setRollback(e.target.value)} />
          <Button disabled={busy || !rollback} onClick={() => go("activate_probability_model", { model_id: rollback })}>Activer (retour arrière)</Button>
        </div>
      </Panel>
      {(models.data ?? []).length === 0 && <Panel><Empty>Aucun modèle de probabilité : aucune entrée n'est possible sans probabilité validée (ou preuve hors échantillon de stratégie).</Empty></Panel>}
      {(models.data ?? []).map((m: Json) => (
        <Panel key={m.model_id} title={`${m.symbol} ${m.timeframe} — ${m.model_id}`} right={<><Badge tone={m.origin === "broker" ? "ok" : "warn"}>{m.origin}</Badge><Badge tone={m.usable ? "ok" : "bad"}>{m.usable ? "utilisable" : "inutilisable ici"}</Badge></>}>
          <div className="mb-3 text-[11px] text-fog-500 num">créé {fmtDateTime(m.created_ms)} · {m.bars} bougies · jeu de données {m.dataset_hash.slice(0, 16)}…</div>
          <div className="grid gap-6 xl:grid-cols-2">
            <Report title="ACHAT" r={m.buy_report} selected={m.buy_selected} reason={m.buy_reason} />
            <Report title="VENTE" r={m.sell_report} selected={m.sell_selected} reason={m.sell_reason} />
          </div>
        </Panel>
      ))}
      <Panel title="Calibration en conditions réelles (probabilités utilisées pour les transactions exécutées, 90 jours)">
        {live.data ? <Report title="Transactions réelles" r={live.data} /> : <Empty>Pas encore de transaction clôturée avec une probabilité enregistrée.</Empty>}
      </Panel>
    </div>
  );
}

function ShadowPanel({ s }: { s: Json | null }) {
  const toast = useToast();
  const sh = usePoll(() => api.view("shadow"), 4000);
  const [sym, setSym] = useState("");
  const [tf, setTf] = useState("M5");
  const [origin, setOrigin] = useState("broker");
  const [onlyStrategy, setOnlyStrategy] = useState("all");
  const go = async (type: string, args: Json) => {
    try {
      toast("ok", await api.command(type, args));
      sh.refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };
  const r = sh.data;
  if (!r) return <Panel title="Shadow Intelligence"><Empty>{sh.error ?? "Shadow désactivé ou en démarrage."}</Empty></Panel>;
  const cands: Json[] = (r.candidates ?? []).filter((c: Json) => onlyStrategy === "all" || c.strategy_id === onlyStrategy);
  const ids = Array.from(new Set((r.candidates ?? []).map((c: Json) => c.strategy_id))) as string[];
  const ta = r.trade_analysis;
  return (
    <div className="space-y-5">
      <Panel title="Shadow Intelligence — moteur parallèle sans accès à l'exécution" right={<span>mis à jour {ago(r.updated_ms)}</span>}>
        <div className="grid gap-4 md:grid-cols-4">
          <KV rows={[["Bougies observées", r.bars_processed], ["Décisions suivies", r.decisions_seen]]} />
          <KV rows={[["Signaux refusés en suivi", r.pending_hypotheticals], ["Candidats", r.candidates.length]]} />
          <div className="md:col-span-2 flex flex-wrap items-end gap-2">
            <select value={sym} onChange={(e) => setSym(e.target.value)}><option value="">instrument…</option>{(s?.specs ?? []).map((x: Json) => <option key={x.symbol}>{x.symbol}</option>)}</select>
            <select value={tf} onChange={(e) => setTf(e.target.value)}>{["M1", "M5", "M15", "M30", "H1"].map((x) => <option key={x}>{x}</option>)}</select>
            <select value={origin} onChange={(e) => setOrigin(e.target.value)}><option value="broker">broker</option><option value="synthetic">synthétique</option></select>
            <Button disabled={!sym} onClick={() => go("evaluate_shadow_history", { symbol: sym, timeframe: tf, origin })}>Évaluer l'historique (backtest + hors échantillon)</Button>
          </div>
        </div>
        <p className="mt-3 text-[11px] text-fog-500">Pipeline : BACKTEST → HORS ÉCHANTILLON → FORWARD (papier, bougies réelles) → DÉMO → VALIDATION. Une promotion n'a lieu que sur votre commande, vers la démo uniquement ; le réel exige ensuite les preuves forward du compte démo.</p>
      </Panel>
      <Panel title="Champions et challengers" pad={false} right={<select value={onlyStrategy} onChange={(e) => setOnlyStrategy(e.target.value)}><option value="all">toutes les stratégies</option>{ids.map((i) => <option key={i}>{i}</option>)}</select>}>
        <div className="max-h-[60vh] overflow-auto">
          <table>
            <thead><tr><th>Candidat</th><th>Rôle</th><th>Étape</th><th>Forward (trades)</th><th>Net / lot</th><th>Espérance</th><th>PF</th><th>DD</th><th>Hors échantillon</th><th>Verdict</th><th></th></tr></thead>
            <tbody>
              {cands.map((c) => (
                <tr key={c.id}>
                  <td className="text-xs font-mono">{c.id}{c.live_enabled && <div className="text-[10px] text-aegis-300">en service</div>}</td>
                  <td><Badge tone={c.role === "champion" ? "gold" : "muted"}>{c.role}</Badge></td>
                  <td><Badge tone={c.stage === "forward" ? "ok" : c.stage === "out_of_sample" ? "info" : "muted"}>{c.stage}</Badge></td>
                  <td className="num">{c.forward.trades}{c.open_positions ? <span className="text-fog-500"> (+{c.open_positions})</span> : null}</td>
                  <td className={cx("num", tone(c.forward.net))}>{fmtSigned(c.forward.net)}</td>
                  <td className={cx("num", tone(c.forward.expectancy))}>{fmtSigned(c.forward.expectancy, 3)}</td>
                  <td className="num">{c.forward.profit_factor === null ? "—" : fmtNum(c.forward.profit_factor)}</td>
                  <td className="num">{fmtNum(c.forward.max_drawdown)}</td>
                  <td className="text-[11px] num">{(c.oos ?? []).map((o: Json) => `${o.symbol} ${o.timeframe} ${o.origin}: ${o.oos_trades} tr, ${fmtSigned(o.oos_net)}`).join(" | ") || "—"}</td>
                  <td className="max-w-[260px] text-xs text-fog-400">{c.verdict}</td>
                  <td>{c.role === "challenger" && <Button small kind="primary" disabled={c.stage !== "forward"} onClick={() => go("promote_challenger", { candidate_id: c.id })}>Promouvoir en démo</Button>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
      <div className="grid gap-5 xl:grid-cols-2">
        <Panel title="Opportunités manquées (signaux refusés suivis sans regarder le futur)" pad={false}>
          {(r.missed ?? []).length === 0 ? <Empty>Aucun signal refusé résolu pour l'instant.</Empty> : (
            <table>
              <thead><tr><th>Contrôle bloquant</th><th>Auraient gagné</th><th>Auraient perdu</th><th>Indécis</th></tr></thead>
              <tbody>{r.missed.map((m: Json) => <tr key={m.check}><td className="font-mono text-xs">{m.check}</td><td className="num text-up-400">{m.would_have_won}</td><td className="num text-down-400">{m.would_have_lost}</td><td className="num">{m.undecided}</td></tr>)}</tbody>
            </table>
          )}
        </Panel>
        <Panel title="Analyse des transactions réelles">
          <KV rows={[["Transactions analysées", ta.trades], ["Entrées prématurées", ta.premature_entries], ["Sorties prématurées / tardives", ta.premature_exits], ["Efficacité moyenne des sorties", fmtPct(ta.avg_exit_efficiency)]]} />
          <div className="mt-3 max-h-48 space-y-1 overflow-auto">{(ta.notes ?? []).slice().reverse().map((n: string, i: number) => <div key={i} className="text-xs text-fog-400">{n}</div>)}</div>
        </Panel>
      </div>
    </div>
  );
}

export default function Intelligence({ s }: { s: Json | null }) {
  const [tab, setTab] = useState<"prob" | "shadow">("prob");
  return (
    <div className="space-y-5">
      <div className="flex gap-1">
        {[["prob", "Probabilités"], ["shadow", "Shadow (champion / challenger)"]].map(([id, label]) => (
          <button key={id} onClick={() => setTab(id as "prob" | "shadow")} className={cx("rounded-md px-3 py-1.5 text-sm", tab === id ? "bg-aegis-500/15 text-aegis-300" : "text-fog-400 hover:bg-ink-800")}>{label}</button>
        ))}
      </div>
      {tab === "prob" ? <ProbabilityPanel s={s} /> : <ShadowPanel s={s} />}
    </div>
  );
}
