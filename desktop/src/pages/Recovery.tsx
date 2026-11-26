import { Json } from "../api";
import { Badge, KV, Panel, cx } from "../components/ui";
import { EmptyBox, Grid, Loaded, Note, PageHeader, StatCard, useView } from "../components/kit";
import { fmtMoney, fmtSigned, tone } from "../format";

function KindCard({ label, k, ccy }: { label: string; k: Json; ccy: string }) {
  const sub = k.trades ? `${k.trades} transaction(s) · ${Math.round((k.wins / k.trades) * 100)} % gagnantes · frais ${fmtMoney(k.costs, ccy)}` : "aucune transaction clôturée";
  return <StatCard label={label} value={fmtSigned(k.net)} tone={tone(k.net)} sub={sub} />;
}

export default function Recovery({ s }: { s: Json }) {
  const q = useView("recovery", 5000);
  const rc = s?.risk_config ?? {};
  const rec = rc.recovery ?? {};
  const ccy = s?.account?.currency ?? "";
  return (
    <div>
      <PageHeader
        title="Recovery & Hedge"
        subtitle="Secondes positions après une perte (re-entry) ou pour couvrir une position (hedge). Jamais une martingale : la taille ne grandit pas après une perte, et chaque seconde position passe par le Risk Guardian comme une entrée normale."
        right={<Badge tone={rc.allow_hedging ? "gold" : "muted"}>{rc.allow_hedging ? "couverture autorisée" : "couverture désactivée"}</Badge>}
      />
      <Loaded q={q}>
        {(r: Json) => (
          <>
            <Grid cols={3}>
              <KindCard label="Entrées normales" k={r.normal} ccy={ccy} />
              <KindCard label="Re-entries" k={r.reentries} ccy={ccy} />
              <KindCard label="Couvertures" k={r.hedges} ccy={ccy} />
            </Grid>
            {r.note && <div className="mt-3"><Note>{r.note}</Note></div>}

            <div className="mt-4 grid gap-4 xl:grid-cols-2">
              <Panel title="Couvertures : ont-elles servi ?">
                {(r.hedge_outcomes ?? []).length === 0 ? <EmptyBox>Aucune couverture clôturée.</EmptyBox> : (
                  <table>
                    <thead><tr><th>Couverture</th><th>Couverte</th><th>Net couverture</th><th>Net couverte</th><th>Ensemble</th><th>Utile</th></tr></thead>
                    <tbody>
                      {r.hedge_outcomes.map((h: Json) => (
                        <tr key={h.hedge_ticket}>
                          <td className="num text-xs">{h.symbol} #{h.hedge_ticket}</td>
                          <td className="num text-xs">#{h.covered_ticket}</td>
                          <td className={cx("num", tone(h.hedge_net))}>{fmtSigned(h.hedge_net)}</td>
                          <td className={cx("num", tone(h.covered_net))}>{fmtSigned(h.covered_net)}</td>
                          <td className={cx("num font-medium", tone(h.combined_net))}>{fmtSigned(h.combined_net)}</td>
                          <td>{h.effective == null ? <Badge>en cours</Badge> : h.effective ? <Badge tone="ok">oui</Badge> : <Badge tone="bad">non</Badge>}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </Panel>
              <Panel title="Re-entries : la séquence a-t-elle récupéré ?">
                {(r.reentry_outcomes ?? []).length === 0 ? <EmptyBox>Aucune re-entry clôturée.</EmptyBox> : (
                  <table>
                    <thead><tr><th>Re-entry</th><th>Après</th><th>Perte initiale</th><th>Re-entry</th><th>Séquence</th></tr></thead>
                    <tbody>
                      {r.reentry_outcomes.map((x: Json) => (
                        <tr key={x.ticket}>
                          <td className="num text-xs">{x.symbol} #{x.ticket}</td>
                          <td className="num text-xs">#{x.after_ticket}</td>
                          <td className={cx("num", tone(x.loss_net))}>{fmtSigned(x.loss_net)}</td>
                          <td className={cx("num", tone(x.reentry_net))}>{fmtSigned(x.reentry_net)}</td>
                          <td className={cx("num font-medium", tone(x.sequence_net))}>{fmtSigned(x.sequence_net)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </Panel>
            </div>

            <div className="mt-4 grid gap-4 xl:grid-cols-2">
              <Panel title="Pertes répétées par contexte">
                {(r.repeated_losses ?? []).length === 0 ? <EmptyBox>Aucun contexte perdant répété.</EmptyBox> : (
                  <ul className="space-y-1 text-sm">
                    {r.repeated_losses.map(([ctx, n]: [string, number]) => (
                      <li key={ctx} className="flex justify-between gap-3"><span className="font-mono text-xs text-fog-300">{ctx}</span><Badge tone={n >= 3 ? "bad" : "warn"}>{n} perte(s)</Badge></li>
                    ))}
                  </ul>
                )}
              </Panel>
              <Panel title="Règles appliquées">
                <KV rows={[
                  ["Re-entries par instrument et par jour", rec.max_reentries_per_symbol_per_day ?? "—"],
                  ["Fenêtre après la perte", rec.reentry_window_minutes != null ? `${rec.reentry_window_minutes} min` : "—"],
                  ["Nouvelle structure exigée", rec.reentry_requires_new_structure ? "oui" : "non"],
                  ["Couvertures par position", rec.max_hedges_per_position ?? "—"],
                  ["Volume de couverture max", rec.max_hedge_ratio != null ? `× ${rec.max_hedge_ratio} de la position` : "—"],
                  ["Couverture (compte hedging)", rc.allow_hedging ? "autorisée" : "désactivée"],
                ]} />
                <p className="mt-3 text-xs text-fog-400">Détail : <span className="font-mono">docs/RECOVERY_HEDGE.md</span>. La couverture n'est possible que sur un compte « hedging », jamais sur un compte « netting ».</p>
              </Panel>
            </div>
          </>
        )}
      </Loaded>
    </div>
  );
}
