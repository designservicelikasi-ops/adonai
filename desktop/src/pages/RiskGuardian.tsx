import { useState } from "react";
import { Json } from "../api";
import { Badge, Bar, Button, KV, Panel } from "../components/ui";
import { EmptyBox, Grid, Loaded, Note, PageHeader, StatCard, useCommand, useView } from "../components/kit";
import { fmtDateTime, fmtMoney, fmtNum } from "../format";

const WEEKDAYS: Record<string, string> = { mon: "lun", tue: "mar", wed: "mer", thu: "jeu", fri: "ven", sat: "sam", sun: "dim" };
const MISSING_STOP: Record<string, string> = { restore: "remettre le stop", close: "clôturer la position", alert: "alerter seulement" };

/** "0 = …" limits of the configuration, shown with their meaning instead of a bare zero. */
function orDefault(v: number | undefined, unit: string, zero: string) {
  return !v ? <span className="text-fog-400">{zero}</span> : `${fmtNum(v, 2)}${unit}`;
}

export default function RiskGuardian({ s, refresh }: { s: Json; refresh: () => void }) {
  const gd = useView("guardian", 2000);
  const { run, busy } = useCommand(refresh);
  const [phrase, setPhrase] = useState("");
  if (!s) return <PageHeader title="Risk Guardian" subtitle="Connexion au moteur…" />;
  const rc = s.risk_config ?? {};
  const ccy = s.account?.currency ?? "";
  const kill = s.kill ?? {};
  return (
    <div>
      <PageHeader
        title="Risk Guardian"
        subtitle="Gardien indépendant des stratégies : il peut refuser ou réduire une entrée, jamais l'agrandir. Chaque limite ci-dessous est appliquée par le moteur, pas par l'interface."
        right={kill.active ? <Badge tone="bad" dot>ARRÊT D'URGENCE</Badge> : <Badge tone="ok">protections actives</Badge>}
      />

      {kill.active && (
        <Panel title="Arrêt d'urgence engagé" className="mb-4">
          <p className="mb-3 text-sm text-down-400">
            Engagé par <b>{kill.actor || "—"}</b> le {fmtDateTime(kill.activated_ms)} : {kill.reason || "sans raison"}.
            Aucune nouvelle entrée n'est acceptée. Positions : {rc.kill_position_policy === "close" ? "clôturées" : "conservées avec leurs stops"} ; ordres en attente {rc.kill_cancel_pending ? "annulés" : "conservés"}.
          </p>
          <div className="flex flex-wrap gap-2">
            <input className="num" placeholder="RESET AEGIS" value={phrase} onChange={(e) => setPhrase(e.target.value)} aria-label="phrase de réinitialisation" />
            <Button kind="danger" disabled={busy || !phrase} onClick={async () => { if (await run("reset_kill", { phrase })) setPhrase(""); }}>
              Réinitialiser (le moteur reste en MONITOR)
            </Button>
          </div>
        </Panel>
      )}

      <Loaded q={gd}>
        {(g: Json) => (
          <>
            <Grid cols={4}>
              <StatCard
                label="Risque aux stops (portefeuille)"
                value={g.portfolio_risk_pct == null ? "non borné" : `${fmtNum(g.portfolio_risk_pct, 2)} %`}
                sub={`limite ${fmtNum(g.portfolio_risk_limit_pct, 2)} % · ${fmtMoney(g.portfolio_risk, ccy)}`}
                tone={g.portfolio_risk_pct == null ? "text-down-400" : undefined}
                accent
              />
              <StatCard
                label="Risque envoyé aujourd'hui"
                value={fmtMoney(g.daily_risk_sent, ccy)}
                sub={g.daily_risk_budget == null ? "budget journalier désactivé" : `budget ${fmtMoney(g.daily_risk_budget, ccy)}`}
              />
              <StatCard label="Taille après pertes" value={`× ${fmtNum(g.loss_streak_scale, 2)}`} sub={g.loss_streak_scale < 1 ? "réduite après une série de pertes" : "taille normale"} tone={g.loss_streak_scale < 1 ? "text-warn-400" : undefined} />
              <StatCard
                label="Positions sans stop"
                value={(g.unprotected_tickets ?? []).length}
                sub={(g.unprotected_tickets ?? []).length ? `tickets ${g.unprotected_tickets.join(", ")}` : "toutes protégées chez le broker"}
                tone={(g.unprotected_tickets ?? []).length ? "text-down-400" : "text-up-400"}
              />
            </Grid>
            <div className="mt-4 grid gap-4 xl:grid-cols-2">
              <Panel title="Exposition au risque">
                <div className="space-y-3 text-sm">
                  <div className="flex justify-between"><span className="text-fog-400">Pertes aux stops / limite</span><span className="num">{g.portfolio_risk_pct == null ? "position sans stop : non borné" : `${fmtNum(g.portfolio_risk_pct, 2)} % / ${fmtNum(g.portfolio_risk_limit_pct, 2)} %`}</span></div>
                  <Bar value={g.portfolio_risk_pct ?? g.portfolio_risk_limit_pct} max={g.portfolio_risk_limit_pct} tone={(g.portfolio_risk_pct ?? Infinity) > g.portfolio_risk_limit_pct * 0.8 ? "down" : "gold"} />
                  {g.daily_risk_budget != null && (
                    <>
                      <div className="flex justify-between"><span className="text-fog-400">Budget de risque du jour</span><span className="num">{fmtMoney(g.daily_risk_sent, ccy)} / {fmtMoney(g.daily_risk_budget, ccy)}</span></div>
                      <Bar value={g.daily_risk_sent ?? 0} max={g.daily_risk_budget} tone={(g.daily_risk_sent ?? 0) > g.daily_risk_budget * 0.8 ? "down" : "gold"} />
                    </>
                  )}
                  <div className="flex justify-between"><span className="text-fog-400">Mode du compte</span><span>{g.account_margin_mode || "inconnu (connecteur antérieur à 1.2)"}</span></div>
                  <div className="flex justify-between"><span className="text-fog-400">Modifications de stop vérifiées</span><span className="num">{g.verified_modifications} vérifiée(s) · {g.unverified_modifications} non constatée(s) · {g.pending_verifications} en attente</span></div>
                  {s.daily_halt && <Note tone="warn">{s.daily_halt}</Note>}
                </div>
              </Panel>
              <Panel title="Incidents de protection">
                {(g.incidents ?? []).length === 0 ? <EmptyBox>Aucun incident : chaque position AEGIS a son stop chez le broker.</EmptyBox> : (
                  <div className="max-h-72 overflow-y-auto">
                    <table>
                      <thead><tr><th>Heure</th><th>Ticket</th><th>Type</th><th>Détail</th></tr></thead>
                      <tbody>{g.incidents.map((i: Json, k: number) => <tr key={k}><td className="num text-xs">{fmtDateTime(i.ts_ms)}</td><td className="num">{i.ticket}</td><td className="font-mono text-xs">{i.kind}</td><td className="text-xs">{i.detail}</td></tr>)}</tbody>
                    </table>
                  </div>
                )}
                <p className="mt-2 text-xs text-fog-400">Position sans stop : {MISSING_STOP[rc.missing_stop_policy] ?? rc.missing_stop_policy} après {fmtNum((rc.missing_stop_grace_ms ?? 0) / 1000, 0)} s.</p>
              </Panel>
            </div>
          </>
        )}
      </Loaded>

      <div className="mt-4 grid gap-4 xl:grid-cols-3">
        <Panel title="Pertes et drawdown">
          <KV rows={[
            ["Risque par transaction", `${fmtNum(rc.risk_per_trade_pct, 2)} %`],
            ["Perte journalière max", `${fmtNum(rc.max_daily_loss_pct, 2)} %`],
            ["Drawdown max", `${fmtNum(rc.max_drawdown_pct, 1)} %`],
            ["Drawdown actuel", s.drawdown_pct == null ? "—" : `${fmtNum(s.drawdown_pct, 2)} %`],
            ["Pertes consécutives max", rc.max_consecutive_losses],
            ["Pause après la série", `${rc.loss_cooldown_minutes} min`],
            ["Réduction après pertes", (rc.loss_streak_risk_scale ?? []).length ? rc.loss_streak_risk_scale.map((x: number) => `×${x}`).join(" → ") : <span className="text-fog-400">aucune</span>],
            ["Budget de risque du jour", orDefault(rc.daily_risk_budget_pct, " %", "désactivé")],
          ]} />
        </Panel>
        <Panel title="Exposition">
          <KV rows={[
            ["Positions ouvertes max", rc.max_open_positions],
            ["… par instrument", rc.max_positions_per_symbol],
            ["Risque cumulé aux stops", orDefault(rc.max_portfolio_risk_pct, " %", "auto")],
            ["Risque corrélé", orDefault(rc.max_correlated_risk_pct, " %", "auto (2 × risque)")],
            ["Seuil de corrélation", fmtNum(rc.correlation_threshold, 2)],
            ["Levier notionnel max", `× ${fmtNum(rc.max_notional_leverage, 1)}`],
            ["Exposition par devise", `× ${fmtNum(rc.max_currency_exposure, 1)}`],
            ["Lots max par transaction", orDefault(rc.max_lots_per_trade, " lot", "aucun")],
          ]} />
        </Panel>
        <Panel title="Marché et horaires">
          <KV rows={[
            ["Spread max", `${fmtNum(rc.max_spread_points, 0)} points`],
            ["Slippage max", `${fmtNum(rc.max_slippage_points, 0)} points`],
            ["Âge max du prix", `${fmtNum(rc.max_tick_age_ms / 1000, 0)} s`],
            ["Âge max du compte", `${fmtNum(rc.max_account_age_ms / 1000, 0)} s`],
            ["Ordres par minute", rc.max_orders_per_minute],
            ["Transactions par jour", rc.max_trades_per_day],
            ["Jours autorisés (UTC)", (rc.allowed_weekdays ?? []).length ? rc.allowed_weekdays.map((d: string) => WEEKDAYS[d] ?? d).join(", ") : <span className="text-fog-400">tous</span>],
            ["Plages horaires", (rc.trading_windows ?? []).length ? `${rc.trading_windows.length} plage(s)` : <span className="text-fog-400">toutes</span>],
          ]} />
        </Panel>
      </div>
      <div className="mt-4">
        <Note>Ces limites se modifient dans « Risque & configuration » ; le moteur valide chaque nouvelle valeur. « auto » : risque cumulé = positions max × risque par transaction ; risque corrélé = 2 × risque par transaction. Une limite ne peut que refuser ou réduire une transaction.</Note>
      </div>
    </div>
  );
}
