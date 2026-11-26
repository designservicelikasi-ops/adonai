import { Json } from "../api";
import { Badge, Bar, Panel, cx } from "../components/ui";
import { Grid, Note, PageHeader, StatCard, useView } from "../components/kit";
import { ago, fmtMoney, fmtNum, fmtPctRaw, fmtSigned, fmtTime, tone } from "../format";

function Light({ ok, label, detail }: { ok: boolean | null; label: string; detail?: string }) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-ink-700 bg-white px-3 py-2.5">
      <span className={cx("h-2.5 w-2.5 rounded-full", ok === null ? "bg-ink-500" : ok ? "bg-up-500" : "bg-down-500")} />
      <div className="min-w-0">
        <div className="text-sm font-medium text-fog-100">{label}</div>
        {detail && <div className="truncate text-xs text-fog-400">{detail}</div>}
      </div>
    </div>
  );
}

export default function CommandCenter({ s }: { s: Json }) {
  const autonomy = useView("autonomy", 3000);
  const guardian = useView("guardian", 3000);
  const scanner = useView("scanner", 4000);
  const learning = useView("learning", 8000);
  const models = useView("probability_models", 10000);
  if (!s) return <PageHeader title="Command Center" subtitle="Connexion au moteur…" />;
  const ccy = s.account?.currency ?? "";
  const conn = s.connection;
  const sim = conn?.terminal === "simulator";
  const g = s.goal;
  const a = autonomy.data;
  const gd = guardian.data;
  const opps: Json[] = scanner.data?.opportunities ?? [];
  const admissible = opps.filter((o) => o.admissible).length;
  const lastDecision = (s.decisions ?? [])[0];
  const usableModels = (models.data ?? []).filter((m: Json) => m.usable);
  return (
    <div>
      <PageHeader
        title="Command Center"
        subtitle="État d'AEGIS en un coup d'œil. Toutes les valeurs viennent du moteur ; ce qui manque est indiqué comme tel."
        right={<Badge tone={s.mode === "live_auto" ? "bad" : s.mode === "demo_auto" ? "gold" : "muted"}>{a?.profile_label ?? "—"}</Badge>}
      />
      <div className="mb-4 grid gap-3 md:grid-cols-3 xl:grid-cols-6">
        <Light ok={true} label={`Moteur ${s.version}`} detail={`démarré ${ago(s.started_ms, s.now_ms)}`} />
        <Light ok={!!conn} label={conn ? (sim ? "SIMULATEUR (test)" : "MT5 connecté") : "MT5 déconnecté"} detail={conn ? `${conn.server} · #${conn.login}` : "aucun terminal"} />
        <Light ok={!s.kill?.active} label={s.kill?.active ? "ARRÊT D'URGENCE" : "Protections actives"} detail={s.kill?.active ? s.kill.reason : "Risk Guardian indépendant"} />
        <Light ok={gd ? (gd.unprotected_tickets ?? []).length === 0 : null} label="Stops chez le broker" detail={gd ? `${(gd.unprotected_tickets ?? []).length} position(s) sans stop` : "—"} />
        <Light ok={s.shadow_dropped === 0} label="Shadow" detail={s.shadow ? `${s.shadow.bars_processed ?? 0} bougies observées` : "désactivé"} />
        <Light ok={usableModels.length > 0 ? true : null} label="Modèle Champion" detail={usableModels.length ? `${usableModels.length} modèle(s) utilisable(s)` : "aucun modèle validé"} />
      </div>
      <Grid cols={5}>
        <StatCard label="Solde" value={fmtMoney(s.account?.balance, ccy)} sub={s.account ? `levier 1:${s.account.leverage}` : "aucun compte"} />
        <StatCard label="Équité" value={fmtMoney(s.account?.equity, ccy)} sub={`flottant ${fmtSigned(s.floating_pnl)}`} />
        <StatCard label="Marge libre" value={fmtMoney(s.account?.margin_free, ccy)} sub={`marge ${fmtMoney(s.account?.margin, ccy)}`} />
        <StatCard label="Drawdown" value={fmtPctRaw(s.drawdown_pct)} sub={`max ${fmtNum(s.risk_config?.max_drawdown_pct, 1)} %`} tone={(s.drawdown_pct ?? 0) > (s.risk_config?.max_drawdown_pct ?? 99) / 2 ? "text-down-400" : undefined} />
        <StatCard label="Résultat net du jour" value={fmtSigned(s.daily_net)} sub={fmtPctRaw(s.daily_net_pct)} tone={tone(s.daily_net)} accent />
      </Grid>
      <div className="mt-4 grid gap-4 xl:grid-cols-3">
        <Panel title="Objectif">
          {g ? (
            <>
              <div className="flex items-baseline justify-between">
                <span className="num text-2xl font-semibold">{fmtMoney(g.achieved, ccy)}</span>
                <span className="text-sm text-fog-400">objectif <span className="num text-aegis-300">{fmtMoney(g.amount, ccy)}</span></span>
              </div>
              <div className="my-3"><Bar value={Math.max(0, g.achieved)} max={g.amount} /></div>
              <div className="flex justify-between text-xs text-fog-400"><span>{fmtNum(g.progress_pct, 1)} %</span><span>reste {fmtMoney(g.remaining, ccy)}</span></div>
              <Note>Un objectif n'est jamais une promesse : il peut arrêter les entrées, jamais augmenter le risque.</Note>
            </>
          ) : <Note>Comptabilité indisponible : aucun compte connecté.</Note>}
        </Panel>
        <Panel title="Risque global">
          {gd ? (
            <div className="space-y-3 text-sm">
              <div className="flex justify-between"><span className="text-fog-400">Risque aux stops</span><span className="num">{gd.portfolio_risk_pct == null ? "non borné" : `${fmtNum(gd.portfolio_risk_pct, 2)} % / ${fmtNum(gd.portfolio_risk_limit_pct, 2)} %`}</span></div>
              <Bar value={gd.portfolio_risk_pct ?? gd.portfolio_risk_limit_pct} max={gd.portfolio_risk_limit_pct} tone={(gd.portfolio_risk_pct ?? 99) > gd.portfolio_risk_limit_pct * 0.8 ? "down" : "gold"} />
              <div className="flex justify-between"><span className="text-fog-400">Positions ouvertes</span><span className="num">{(s.positions ?? []).length} / {s.risk_config?.max_open_positions}</span></div>
              <div className="flex justify-between"><span className="text-fog-400">Mode du compte</span><span>{gd.account_margin_mode || "—"}</span></div>
              <div className="flex justify-between"><span className="text-fog-400">Taille après pertes</span><span className="num">× {fmtNum(gd.loss_streak_scale, 2)}</span></div>
              {s.daily_halt && <Note tone="warn">{s.daily_halt}</Note>}
            </div>
          ) : null}
        </Panel>
        <Panel title="Intelligence">
          <div className="space-y-2 text-sm">
            <div className="flex justify-between"><span className="text-fog-400">Opportunités détectées</span><span className="num">{opps.length}</span></div>
            <div className="flex justify-between"><span className="text-fog-400">… admissibles</span><span className="num text-aegis-300">{admissible}</span></div>
            <div className="flex justify-between"><span className="text-fog-400">Analyses réalisées</span><span className="num">{scanner.data?.scans ?? 0}</span></div>
            <div className="flex justify-between"><span className="text-fog-400">Recherche en cours</span><span>{learning.data?.sweep_running ? "oui" : "non"}</span></div>
            <div className="border-t border-ink-700 pt-2">
              <div className="text-[11px] uppercase tracking-wider text-fog-500">Dernière décision</div>
              {lastDecision ? (
                <div className="mt-1 text-xs">
                  <Badge tone={lastDecision.outcome === "approved" ? "ok" : lastDecision.outcome === "observed" ? "muted" : "bad"}>{lastDecision.outcome}</Badge>{" "}
                  <span className="font-medium">{String(lastDecision.side).toUpperCase()} {lastDecision.symbol}</span> <span className="text-fog-400">{fmtTime(lastDecision.ts_ms)} — {lastDecision.reason}</span>
                </div>
              ) : <div className="mt-1 text-xs text-fog-400">Aucune décision : WAIT.</div>}
            </div>
          </div>
        </Panel>
      </div>
      <Panel title="Dernières activités" className="mt-4">
        <ul className="max-h-64 space-y-1 overflow-y-auto text-xs">
          {(s.logs ?? []).slice(0, 25).map((l: Json, i: number) => (
            <li key={i} className="flex gap-3">
              <span className="num w-16 shrink-0 text-fog-500">{fmtTime(l.ts_ms)}</span>
              <span className={cx("w-20 shrink-0 font-mono", l.level === "error" ? "text-down-400" : l.level === "warn" ? "text-warn-400" : "text-fog-400")}>{l.source}</span>
              <span className="text-fog-300">{l.text}</span>
            </li>
          ))}
        </ul>
      </Panel>
    </div>
  );
}
