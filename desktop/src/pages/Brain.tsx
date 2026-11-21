import { useState } from "react";
import { api, Json } from "../api";
import { Badge, Button, Empty, KV, Panel, Stat, cx, usePoll, useToast } from "../components/ui";
import { ago, fmtDateTime, fmtNum, fmtPct, fmtSigned, fmtTime, tone } from "../format";

const SESSIONS: Record<string, string> = {
  asie: "Asie (0 h – 7 h UTC)",
  londres: "Londres (7 h – 12 h)",
  londres_new_york: "Londres + New York (12 h – 16 h)",
  new_york: "New York (16 h – 21 h)",
  fermeture: "Fermeture (21 h – 24 h)",
};

function Lessons({ l }: { l: Json }) {
  const cut = l.calibration?.haircut ?? 1;
  return (
    <Panel title={`Ce que le cerveau a appris — ${l.symbol}`} right={<span>mis à jour {ago(l.updated_ms)}</span>}>
      <div className="grid gap-4 md:grid-cols-4">
        <Stat label="Transactions étudiées" value={l.trades} sub={<span className={tone(l.net)}>résultat net {fmtSigned(l.net)}</span>} />
        <Stat
          label="Frais réels par lot"
          value={l.cost?.observed_per_lot == null ? "—" : fmtNum(l.cost.observed_per_lot)}
          sub={`prévu ${fmtNum(l.cost?.configured_per_lot)} · ajouté +${fmtNum(l.cost?.extra_per_lot)}`}
        />
        <Stat
          label="Correction des probabilités"
          value={`× ${fmtNum(cut, 3)}`}
          valueClass={cut < 0.95 ? "text-down-400" : undefined}
          sub={`${l.calibration?.actual_wins ?? 0} gains obtenus pour ${fmtNum(l.calibration?.promised_wins, 1)} promis`}
        />
        <Stat label="Contextes bloqués" value={(l.contexts ?? []).filter((c: Json) => c.blocked).length} sub={`${(l.contexts ?? []).length} contextes suivis`} />
      </div>
      <p className="mt-3 text-[11px] text-fog-500">
        Le cerveau ne devient jamais plus audacieux tout seul : un broker moins cher que prévu ne réduit pas les frais estimés, et la correction ne monte jamais au-dessus de × 1. Seuls les résultats du même type de terminal comptent (le simulateur n'apprend rien au compte réel).
      </p>
      {(l.contexts ?? []).length > 0 && (
        <table className="mt-3">
          <thead><tr><th>Stratégie</th><th>Sens</th><th>Session</th><th>Transactions</th><th>Gagnantes</th><th>Net</th><th>Moyenne</th><th>Verdict</th></tr></thead>
          <tbody>
            {l.contexts.map((c: Json, i: number) => (
              <tr key={i}>
                <td className="font-mono text-xs">{c.strategy_id}</td>
                <td>{String(c.side).toUpperCase()}</td>
                <td className="text-xs">{SESSIONS[c.session] ?? c.session}</td>
                <td className="num">{c.trades}</td>
                <td className="num">{c.wins}</td>
                <td className={cx("num", tone(c.net))}>{fmtSigned(c.net)}</td>
                <td className={cx("num", tone(c.mean_net))}>{fmtSigned(c.mean_net, 3)}</td>
                <td className="text-xs">{c.blocked ? <Badge tone="bad">bloqué</Badge> : null} <span className="text-fog-400">{c.verdict}</span></td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}

const FIELDS: [string, string, string][] = [
  ["min_net_win", "Gain net minimal par transaction", "en devise du compte, après tous les frais"],
  ["max_net_win", "Gain net maximal par transaction", "le volume est réduit si besoin, jamais augmenté"],
  ["sl_pips", "Stop Loss (pips)", "0 = le stop de chaque stratégie"],
  ["tp_pips", "Take Profit fixe (pips)", "0 = le cerveau choisit le TP"],
  ["max_tp_pips", "Take Profit maximum (pips)", "0 = sans limite ; ignoré si le TP est fixe"],
  ["retrain_every_hours", "Réentraînement toutes les (heures)", "0 = jamais ; une nouvelle version est toujours proposée, jamais activée seule"],
];

function BrainSettings({ cfg, busy, onSave }: { cfg: Json; busy: boolean; onSave: (c: Json) => void }) {
  const [draft, setDraft] = useState<Json>(cfg);
  const [open, setOpen] = useState(false);
  const changed = JSON.stringify(draft) !== JSON.stringify(cfg);
  return (
    <Panel title="Réglages du cerveau" right={<Button small kind="ghost" onClick={() => { setDraft(cfg); setOpen(!open); }}>{open ? "Fermer" : "Modifier"}</Button>}>
      {!open ? (
        <KV rows={[
          ["Cerveau", cfg.enabled ? "actif" : "désactivé"],
          ["Gain net visé", `${fmtNum(cfg.min_net_win)} à ${fmtNum(cfg.max_net_win)}`],
          ["Stop Loss", cfg.sl_pips > 0 ? `${fmtNum(cfg.sl_pips, 1)} pips` : "celui de la stratégie"],
          ["Take Profit", cfg.tp_pips > 0 ? `fixe : ${fmtNum(cfg.tp_pips, 1)} pips` : cfg.max_tp_pips > 0 ? `choisi par le cerveau, au plus ${fmtNum(cfg.max_tp_pips, 1)} pips` : "choisi par le cerveau"],
        ]} />
      ) : (
        <div className="space-y-3">
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={!!draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
            Cerveau actif (il choisit la cible des trades sur {(cfg.symbols ?? []).join(", ")})
          </label>
          <div className="grid gap-3 md:grid-cols-3">
            {FIELDS.map(([key, label, help]) => (
              <label key={key} className="block text-xs text-fog-400">
                {label}
                <input type="number" step="any" min={0} className="num mt-1 w-full" value={draft[key] ?? 0} onChange={(e) => setDraft({ ...draft, [key]: e.target.value === "" ? 0 : Number(e.target.value) })} />
                <span className="text-[10px] text-fog-500">{help}</span>
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <Button kind="primary" disabled={busy || !changed} onClick={() => onSave(draft)}>Enregistrer</Button>
            <Button disabled={busy || !changed} onClick={() => setDraft(cfg)}>Annuler</Button>
          </div>
          <p className="text-[11px] text-fog-500">Appliqué immédiatement et conservé après un redémarrage. Le SL en pips remplace celui des stratégies ; les limites de risque (perte max par trade, etc.) restent vérifiées après le cerveau.</p>
        </div>
      )}
    </Panel>
  );
}

function scoreText(s: Json) {
  if (!s) return "—";
  const side = (x: Json) => (x?.brier == null ? "aucune probabilité" : `Brier ${fmtNum(x.brier, 4)} (${x.predicted}/${x.rows} scénarios)`);
  return `achat ${side(s.buy)} · vente ${side(s.sell)}`;
}

export default function Brain({ s }: { s: Json | null }) {
  const toast = useToast();
  const b = usePoll(() => api.view("brain"), 3000);
  const [busy, setBusy] = useState(false);
  const go = async (type: string, args?: Json) => {
    setBusy(true);
    try {
      toast("ok", await api.command(type, args));
      b.refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    } finally {
      setBusy(false);
    }
  };
  const v = b.data;
  if (!v) return <Panel title="Cerveau micro-profits"><Empty>{b.error ?? "Chargement…"}</Empty></Panel>;
  const cfg = v.config ?? {};
  const ccy = s?.account?.currency ?? "";
  return (
    <div className="space-y-5">
      <Panel
        title="Cerveau micro-profits — vise les petits gains nets et apprend de chaque transaction"
        right={<Badge tone={v.enabled ? "ok" : "muted"} dot={v.enabled}>{v.enabled ? "actif" : "désactivé"}</Badge>}
      >
        {!v.enabled && (
          <p className="mb-3 text-sm text-fog-300">
            Désactivé : les stratégies gardent leurs propres cibles et leurs propres stops. Pour l'activer : « Réglages du cerveau » → Modifier → Cerveau actif.
          </p>
        )}
        <div className="grid gap-4 md:grid-cols-4">
          <Stat label="Gain net visé par transaction" value={`${fmtNum(cfg.min_net_win)} à ${fmtNum(cfg.max_net_win)} ${ccy}`} sub="après spread, commission et glissement" />
          <Stat label="Instruments" value={(v.symbols ?? []).join(", ") || (cfg.symbols ?? []).join(", ")} sub="variantes du broker incluses" />
          <Stat label="Prochain réentraînement" value={v.retraining ? "en cours…" : v.next_retrain_ms ? fmtDateTime(v.next_retrain_ms) : "désactivé"} sub={v.last_retrain_ms ? `dernier ${ago(v.last_retrain_ms)}` : "jamais"} />
          <div className="flex items-end">
            <Button kind="primary" disabled={busy || !v.enabled || v.retraining} onClick={() => go("brain_retrain")} title="Entraîne une nouvelle version sur les dernières bougies et la compare à l'actuelle sur des données qu'elle n'a jamais vues">
              Réentraîner maintenant
            </Button>
          </div>
        </div>
        <p className="mt-3 text-[11px] text-fog-500">
          Pour chaque signal, le cerveau essaie plusieurs cibles (et des volumes plus petits si besoin) dont le gain net tombe dans la fourchette, et garde celle qui rapporte le plus en moyenne compte tenu de la probabilité de l'atteindre avant le stop. Il ne déplace jamais le stop, ne demande jamais plus de volume que le gestionnaire de risque, et toutes les limites de risque restent vérifiées après lui.
        </p>
        {(v.warnings ?? []).map((w: string, i: number) => (
          <div key={i} className="mt-2 rounded-md border border-warn-400/40 bg-warn-400/10 px-3 py-2 text-xs text-warn-400">{w}</div>
        ))}
      </Panel>

      <BrainSettings key={JSON.stringify(cfg)} cfg={cfg} busy={busy} onSave={(c) => go("set_brain_config", c)} />

      {(v.candidates ?? []).length > 0 && (
        <Panel title="Nouvelles versions proposées — à vous de décider">
          <div className="space-y-3">
            {v.candidates.map((c: Json) => (
              <div key={c.model_id} className="rounded-md border border-aegis-500/40 bg-aegis-500/5 p-3">
                <div className="mb-2 flex flex-wrap items-center gap-2 text-sm">
                  <span className="font-medium">{c.symbol} {c.timeframe}</span>
                  <span className="font-mono text-xs text-fog-400">{c.model_id}</span>
                  <span className="text-[11px] text-fog-500">créée {fmtDateTime(c.created_ms)} · entraînée sur {c.bars_trained} bougies</span>
                </div>
                <p className="mb-2 text-xs text-fog-300">{c.verdict}</p>
                <KV rows={[["Nouvelle version (période de contrôle)", scoreText(c.score)], [`Version active${c.active_model_id ? ` ${c.active_model_id}` : ""}`, c.active_score ? scoreText(c.active_score) : "aucune"]]} />
                <div className="mt-3 flex gap-2">
                  <Button kind="primary" small disabled={busy} onClick={() => go("brain_accept_candidate", { model_id: c.model_id })}>Activer cette version</Button>
                  <Button small disabled={busy} onClick={() => go("brain_dismiss_candidate", { model_id: c.model_id })}>Écarter</Button>
                </div>
              </div>
            ))}
          </div>
          <p className="mt-3 text-[11px] text-fog-500">Un Brier plus bas veut dire des probabilités plus justes. La période de contrôle est la partie la plus récente de l'historique, que la nouvelle version n'a jamais vue. Une version écartée reste enregistrée : elle peut être activée plus tard depuis « Probabilités & Shadow ».</p>
        </Panel>
      )}

      {(v.lessons ?? []).length === 0 ? (
        <Panel title="Ce que le cerveau a appris"><Empty>Pas encore de transaction clôturée sur les instruments du cerveau.</Empty></Panel>
      ) : (
        v.lessons.map((l: Json) => <Lessons key={l.symbol} l={l} />)
      )}

      <Panel title="Derniers choix de cible" pad={false}>
        {(v.choices ?? []).length === 0 ? <Empty>Aucun signal sur les instruments du cerveau pour l'instant.</Empty> : (
          <div className="max-h-[50vh] overflow-auto">
            <table>
              <thead><tr><th>Heure</th><th>Signal</th><th>Choix</th><th>Gain net si gagné</th><th>Perte si stop</th><th>Probabilité</th><th>Espérance</th><th>Volume</th><th>Explication</th></tr></thead>
              <tbody>
                {v.choices.map((c: Json, i: number) => {
                  const x = c.chosen ?? c.best;
                  return (
                    <tr key={i}>
                      <td className="num text-xs">{fmtTime(c.ts_ms)}</td>
                      <td className="text-xs">{c.symbol} {c.timeframe} · {String(c.side).toUpperCase()} · <span className="font-mono">{c.strategy_id}</span></td>
                      <td>{c.traded ? <Badge tone="ok">tradé</Badge> : c.chosen ? <Badge tone="info">choisi, refusé ensuite</Badge> : <Badge tone="muted">attendre</Badge>}</td>
                      <td className="num text-up-400">{x ? fmtSigned(x.net_if_win) : "—"}</td>
                      <td className="num text-down-400">{x ? fmtSigned(x.net_if_loss) : "—"}</td>
                      <td className="num">{x ? fmtPct(x.probability) : "—"}</td>
                      <td className={cx("num", tone(x?.expected_net))}>{x ? fmtSigned(x.expected_net, 3) : "—"}</td>
                      <td className="num">{x ? fmtNum(x.volume) : "—"}</td>
                      <td className="max-w-[360px] text-xs text-fog-400">{c.reason}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Panel>

      <Panel title="Historique des réentraînements" pad={false}>
        {(v.retrain_log ?? []).length === 0 ? <Empty>Aucun réentraînement pour l'instant.</Empty> : (
          <table>
            <thead><tr><th>Date</th><th>Modèle</th><th>Résultat</th></tr></thead>
            <tbody>
              {v.retrain_log.map((r: Json, i: number) => (
                <tr key={i}>
                  <td className="num text-xs">{fmtDateTime(r.ts_ms)}</td>
                  <td className="text-xs">{r.symbol} {r.timeframe}</td>
                  <td className="text-xs">{r.proposed ? <Badge tone="gold">proposée</Badge> : null} <span className="text-fog-400">{r.outcome}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
    </div>
  );
}
