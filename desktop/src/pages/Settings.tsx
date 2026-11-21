import { useEffect, useState } from "react";
import { api, Json } from "../api";
import { Badge, Button, Empty, Modal, Panel, usePoll, useToast } from "../components/ui";
import { modeLabel, modeTone } from "../App";

type Field = { path: string; label: string; help?: string; kind?: "num" | "int" | "bool" | "select"; options?: [string, string][]; step?: number; pct?: boolean };

const SECTIONS: { title: string; fields: Field[] }[] = [
  {
    title: "Objectif de gain (journalier ou mensuel)",
    fields: [
      { path: "daily_goal.enabled", label: "Objectif actif", kind: "bool" },
      { path: "daily_goal.period", label: "Période", kind: "select", options: [["day", "par JOUR (repart chaque jour à 0 h UTC)"], ["month", "par MOIS (repart le 1er du mois, UTC)"]] },
      { path: "daily_goal.amount", label: "Montant (devise du compte)", step: 0.5 },
      { path: "daily_goal.basis", label: "Nature", kind: "select", options: [["net", "NET (après commission, swap, frais)"], ["gross", "BRUT (avant coûts)"]] },
      { path: "daily_goal.policy", label: "Une fois atteint", kind: "select", options: [["stop_entries", "arrêter les nouvelles entrées"], ["protect_gains", "continuer en protégeant le gain"], ["track_only", "suivre seulement"]] },
      { path: "daily_goal.protect_fraction", label: "Part du gain protégée", step: 0.05, help: "0,5 à 1" },
    ],
  },
  {
    title: "Probabilités et espérance",
    fields: [
      { path: "buy_min_probability", label: "Seuil BUY minimum (P(TP avant SL))", step: 0.01, pct: true },
      { path: "sell_min_probability", label: "Seuil SELL minimum", step: 0.01, pct: true },
      { path: "min_net_expected_profit", label: "Espérance nette minimale / trade", step: 0.01 },
    ],
  },
  {
    title: "Limites de risque",
    fields: [
      { path: "risk_per_trade_pct", label: "Risque par transaction (% équité)", step: 0.05 },
      { path: "max_lots_per_trade", label: "Plafond de lots par transaction (0 = off)", step: 0.01, help: "micro-trading : ne peut que réduire la taille" },
      { path: "max_daily_loss_pct", label: "Perte journalière maximale (%)", step: 0.1 },
      { path: "max_drawdown_pct", label: "Drawdown maximal → arrêt d'urgence (%)", step: 0.5 },
      { path: "max_open_positions", label: "Positions ouvertes max", kind: "int" },
      { path: "max_positions_per_symbol", label: "Positions par instrument max", kind: "int" },
      { path: "max_notional_leverage", label: "Exposition globale max (× équité)", step: 0.5 },
      { path: "max_currency_exposure", label: "Exposition par devise max (× équité)", step: 0.5 },
      { path: "max_spread_points", label: "Spread maximal (points)", step: 1 },
      { path: "max_slippage_points", label: "Slippage maximal accepté (points)", step: 1 },
    ],
  },
  {
    title: "Volume et rythme",
    fields: [
      { path: "max_trades_per_day", label: "Transactions max par jour", kind: "int", help: "jusqu'à 10 000" },
      { path: "max_orders_per_minute", label: "Entrées max par minute", kind: "int" },
      { path: "max_consecutive_losses", label: "Pertes consécutives avant pause (0 = off)", kind: "int" },
      { path: "loss_cooldown_minutes", label: "Durée de la pause (min)", kind: "int" },
      { path: "max_cost_to_gross_ratio", label: "Coûts / gains bruts max (0 = off)", step: 0.05, pct: true },
      { path: "cost_check_min_trades", label: "… vérifié à partir de N trades", kind: "int" },
    ],
  },
  {
    title: "Coûts (estimation avant chaque ordre)",
    fields: [
      { path: "commission_per_lot_round_turn", label: "Commission aller-retour par lot", step: 0.1, help: "MT5 ne la fournit pas : à saisir selon votre broker" },
      { path: "expected_slippage_points", label: "Slippage attendu (points)", step: 0.5 },
      { path: "conversion_fee_pct", label: "Frais de conversion (%)", step: 0.05 },
      { path: "expected_holding_nights", label: "Nuits de détention attendues", step: 1 },
    ],
  },
  {
    title: "Position Intelligence",
    fields: [
      { path: "position.enabled", label: "Gestion automatique des positions", kind: "bool" },
      { path: "position.secure_at_r", label: "SECURE à partir de (× risque initial)", step: 0.1 },
      { path: "position.secure_buffer_points", label: "Marge au-delà du point mort + coûts (points)", step: 1 },
      { path: "position.trail_from_r", label: "Stop suiveur à partir de (× R, 0 = off)", step: 0.1 },
      { path: "position.trail_atr", label: "Distance du stop suiveur (ATR)", step: 0.1 },
      { path: "position.tp_decay_bars", label: "Rapprocher le TP après N bougies (0 = off)", kind: "int" },
      { path: "position.tp_decay_atr", label: "Nouveau TP à (ATR)", step: 0.05 },
      { path: "position.max_bars_held", label: "Clôture après N bougies (0 = off)", kind: "int" },
      { path: "position.close_at_net", label: "Clôture si net réalisable ≥ (0 = off)", step: 0.01 },
    ],
  },
  {
    title: "Arrêt d'urgence et contrôles",
    fields: [
      { path: "kill_position_policy", label: "Positions à l'arrêt d'urgence", kind: "select", options: [["keep", "conserver avec leurs SL/TP"], ["close", "clôturer au marché"]] },
      { path: "kill_cancel_pending", label: "Annuler les ordres en attente", kind: "bool" },
      { path: "max_tick_age_ms", label: "Âge maximal d'un prix (ms)", kind: "int" },
      { path: "max_account_age_ms", label: "Âge maximal des données de compte (ms)", kind: "int" },
      { path: "authorization_ttl_ms", label: "Validité d'une autorisation (ms)", kind: "int" },
    ],
  },
];

const get = (o: Json, path: string) => path.split(".").reduce((a, k) => (a == null ? a : a[k]), o);
function set(o: Json, path: string, v: unknown): Json {
  const c = structuredClone(o);
  const keys = path.split(".");
  let cur = c;
  for (const k of keys.slice(0, -1)) cur = cur[k];
  cur[keys[keys.length - 1]] = v;
  return c;
}

function FieldInput({ f, value, onChange }: { f: Field; value: Json; onChange: (v: Json) => void }) {
  if (f.kind === "bool") return <input type="checkbox" className="h-4 w-4 accent-[#C9A227]" checked={!!value} onChange={(e) => onChange(e.target.checked)} />;
  if (f.kind === "select") return <select value={value} onChange={(e) => onChange(e.target.value)}>{f.options!.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>;
  return <input className="w-32 num text-right" type="number" step={f.kind === "int" ? 1 : f.step ?? 0.01} value={value ?? ""} onChange={(e) => onChange(f.kind === "int" ? parseInt(e.target.value || "0", 10) : parseFloat(e.target.value))} />;
}

export default function Settings({ s, refresh }: { s: Json | null; refresh: () => void }) {
  const toast = useToast();
  const [form, setForm] = useState<Json | null>(null);
  const [dirty, setDirty] = useState(false);
  const [windows, setWindows] = useState("");
  const [liveOpen, setLiveOpen] = useState(false);
  const [phrase, setPhrase] = useState("");
  const [resetPhrase, setResetPhrase] = useState("");
  const kill = s?.kill;
  const cfgSource = usePoll(() => api.view("risk_config"), 10000);

  useEffect(() => {
    if (cfgSource.data && !dirty) {
      setForm(cfgSource.data);
      setWindows((cfgSource.data.trading_windows ?? []).map((w: Json) => `${w.start}-${w.end}`).join(", "));
    }
  }, [cfgSource.data, dirty]);

  const save = async () => {
    try {
      const tw = windows.split(",").map((x) => x.trim()).filter(Boolean).map((x) => { const [start, end] = x.split("-").map((y) => y.trim()); return { start, end }; });
      toast("ok", await api.command("set_risk_config", { ...form, trading_windows: tw }));
      setDirty(false);
      cfgSource.refresh();
      refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };
  const setMode = async (mode: string, confirmation = "") => {
    try {
      toast("ok", await api.command("set_mode", { mode, confirmation }));
      setLiveOpen(false);
      setPhrase("");
      refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };

  if (!form) return <Empty>{cfgSource.error ?? "Chargement de la configuration…"}</Empty>;
  return (
    <div className="space-y-5">
      <Panel title="Mode de fonctionnement" right={<Badge tone={modeTone(s?.mode)}>{modeLabel(s?.mode)}</Badge>}>
        <p className="mb-3 text-sm text-fog-300">
          Le moteur démarre toujours en MONITOR (aucun ordre). DEMO AUTO exige un compte de démonstration. LIVE AUTO est désactivé par défaut : il doit être autorisé dans le fichier de configuration du serveur, puis armé ici avec le numéro du compte et la phrase de confirmation.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button disabled={s?.mode === "monitor"} onClick={() => setMode("monitor")}>MONITOR</Button>
          <Button kind="primary" disabled={s?.mode === "demo_auto"} onClick={() => setMode("demo_auto")}>DEMO AUTO</Button>
          <Button kind="danger" disabled={!s?.live_trading_enabled_in_config || s?.mode === "live_auto"} onClick={() => setLiveOpen(true)} title={s?.live_trading_enabled_in_config ? "" : "désactivé dans la configuration du serveur"}>LIVE AUTO…</Button>
        </div>
      </Panel>
      {kill?.active && (
        <Panel title="Arrêt d'urgence actif">
          <p className="mb-2 text-sm text-down-400">Engagé par {kill.actor} : {kill.reason}</p>
          <div className="flex gap-2">
            <input className="num" placeholder="RESET AEGIS" value={resetPhrase} onChange={(e) => setResetPhrase(e.target.value)} />
            <Button kind="danger" onClick={async () => { try { toast("ok", await api.command("reset_kill", { phrase: resetPhrase })); setResetPhrase(""); refresh(); } catch (e) { toast("bad", String((e as Error).message)); } }}>Réinitialiser (le moteur reste en MONITOR)</Button>
          </div>
        </Panel>
      )}
      <Panel title="Configuration du risque" right={<><Button small kind="ghost" disabled={!dirty} onClick={() => setDirty(false)}>Annuler</Button><Button small kind="primary" disabled={!dirty} onClick={save}>Valider et enregistrer</Button></>}>
        <p className="mb-4 text-xs text-fog-500">Chaque modification est validée par le moteur (bornes de sécurité), enregistrée en base avec un numéro de version et écrite au journal d'audit. Une valeur hors bornes est refusée, jamais corrigée en silence.</p>
        <div className="grid gap-6 xl:grid-cols-2">
          {SECTIONS.map((sec) => (
            <div key={sec.title}>
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-[.14em] text-aegis-400">{sec.title}</div>
              <div className="divide-y divide-ink-750 rounded-lg border border-ink-700">
                {sec.fields.map((f) => (
                  <div key={f.path} className="flex items-center justify-between gap-4 px-3 py-2">
                    <div className="text-sm">{f.label}{f.help && <div className="text-[11px] text-fog-500">{f.help}</div>}{f.pct && <div className="text-[11px] text-fog-500">{((get(form, f.path) ?? 0) * 100).toFixed(0)} %</div>}</div>
                    <FieldInput f={f} value={get(form, f.path)} onChange={(v) => { setForm(set(form, f.path, v)); setDirty(true); }} />
                  </div>
                ))}
              </div>
            </div>
          ))}
          <div>
            <div className="mb-2 text-[11px] font-semibold uppercase tracking-[.14em] text-aegis-400">Horaires de trading (UTC)</div>
            <input className="w-full num" placeholder="07:00-20:00, 22:00-02:00 (vide = toujours)" value={windows} onChange={(e) => { setWindows(e.target.value); setDirty(true); }} />
          </div>
        </div>
      </Panel>
      <Modal open={liveOpen} title="Armer LIVE AUTO (argent réel)" onClose={() => setLiveOpen(false)}>
        <p className="mb-3 text-sm text-fog-300">Des ordres réels seront envoyés automatiquement sur le compte <b className="num">{s?.connection?.login}</b>. Aucune performance passée ne garantit un résultat. Tapez exactement :</p>
        <div className="mb-3 rounded bg-ink-950 px-3 py-2 font-mono text-xs text-aegis-300">{s?.connection?.login} J'ACCEPTE LE RISQUE REEL</div>
        <input className="mb-4 w-full font-mono" value={phrase} onChange={(e) => setPhrase(e.target.value)} />
        <div className="flex justify-end gap-2">
          <Button kind="ghost" onClick={() => setLiveOpen(false)}>Annuler</Button>
          <Button kind="danger" disabled={!phrase} onClick={() => setMode("live_auto", phrase)}>Armer LIVE AUTO</Button>
        </div>
      </Modal>
    </div>
  );
}
