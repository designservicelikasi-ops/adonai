import { Json } from "../api";
import { Badge, Bar, Button, Panel, cx } from "../components/ui";
import { EmptyBox, Grid, Loaded, Note, PageHeader, StatCard, useCommand, useView } from "../components/kit";
import { ago, fmtMoney, fmtNum, fmtSigned, tone } from "../format";

function rhoTone(r: number, threshold: number) {
  const a = Math.abs(r);
  return a >= threshold ? (r > 0 ? "text-down-400 font-medium" : "text-info-400 font-medium") : "text-fog-400";
}

export default function Portfolio({ s, refresh }: { s: Json; refresh: () => void }) {
  const q = useView("portfolio", 4000);
  const { run, busy } = useCommand(() => { q.refresh(); refresh(); });
  const threshold = s?.risk_config?.correlation_threshold ?? 0.7;
  return (
    <div>
      <PageHeader
        title="Portefeuille multi-actifs"
        subtitle="Instruments suivis, exposition par devise et corrélations. Deux positions fortement corrélées dans le même sens comptent comme un seul risque : le Risk Guardian borne leur somme."
      />
      <Loaded q={q}>
        {(p: Json) => {
          const inst: Json[] = p.instruments ?? [];
          const enabled = inst.filter((i) => i.enabled).length;
          const floating = inst.reduce((n, i) => n + (i.floating ?? 0), 0);
          const strong = (p.correlations ?? []).filter((c: Json) => Math.abs(c.rho) >= threshold);
          return (
            <>
              <Grid cols={4}>
                <StatCard label="Instruments" value={`${enabled} / ${inst.length}`} sub="actifs pour de nouvelles entrées" />
                <StatCard label="Flottant total" value={fmtSigned(floating)} tone={tone(floating)} sub={p.account_currency} />
                <StatCard label="Corrélations fortes" value={strong.length} sub={`|ρ| ≥ ${fmtNum(threshold, 2)} sur ${p.correlation_timeframe || "—"}`} tone={strong.length ? "text-warn-400" : undefined} />
                <StatCard label="Devises exposées" value={(p.currency_exposure ?? []).filter(([, v]: [string, number]) => Math.abs(v) > 0).length} sub={p.correlations_ms ? `corrélations calculées ${ago(p.correlations_ms, s?.now_ms)}` : "corrélations non calculées"} />
              </Grid>

              <Panel title="Instruments" className="mt-4" pad={false}>
                {inst.length === 0 ? <EmptyBox>Aucun instrument : connectez un terminal.</EmptyBox> : (
                  <table>
                    <thead><tr><th>Instrument</th><th>Catégorie</th><th>Spread</th><th>Lot minimum</th><th>Exposition ouverte</th><th>Flottant</th><th>Actualités</th><th>Entrées</th></tr></thead>
                    <tbody>
                      {inst.map((i) => (
                        <tr key={i.symbol}>
                          <td><div className="font-medium">{i.symbol}</div>{!i.tradable_at_broker && <Badge tone="bad">non négociable chez le broker</Badge>}</td>
                          <td className="text-xs">{i.category}</td>
                          <td className="num">{i.spread_points == null ? "—" : `${fmtNum(i.spread_points, 1)} pts`}</td>
                          <td className="num text-xs">{i.spec?.volume_min} lot · {fmtMoney(i.min_lot_notional, i.spec?.currency_profit)}<div className="text-fog-500">marge {fmtMoney(i.min_lot_margin, p.account_currency)}</div></td>
                          <td className="num text-xs">achat {fmtNum(i.open_buy_volume, 2)} · vente {fmtNum(i.open_sell_volume, 2)}</td>
                          <td className={cx("num", tone(i.floating))}>{fmtSigned(i.floating)}</td>
                          <td className="max-w-[240px] text-xs text-fog-400">{i.news}</td>
                          <td>
                            <div className="flex items-center gap-2">
                              <Badge tone={i.enabled ? "ok" : "muted"}>{i.enabled ? "actives" : "suspendues"}</Badge>
                              <Button small kind={i.enabled ? "ghost" : "default"} disabled={busy} onClick={() => run("set_symbol_enabled", { symbol: i.symbol, enabled: !i.enabled })}>{i.enabled ? "Suspendre" : "Réactiver"}</Button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </Panel>
              <p className="mt-2 text-xs text-fog-400">Suspendre un instrument bloque les nouvelles entrées ; ses positions ouvertes restent gérées.</p>

              <div className="mt-4 grid gap-4 xl:grid-cols-2">
                <Panel title="Exposition par devise">
                  {(p.currency_exposure ?? []).length === 0 ? <EmptyBox>Aucune position ouverte.</EmptyBox> : (
                    <ul className="space-y-2.5 text-sm">
                      {p.currency_exposure.map(([ccy, v]: [string, number]) => {
                        const lim = s?.risk_config?.max_currency_exposure ?? 0;
                        return (
                          <li key={ccy}>
                            <div className="flex items-center justify-between gap-3">
                              <span className="font-medium">{ccy}</span>
                              <span className="num">× {fmtNum(v, 2)} <span className="text-fog-500">/ × {fmtNum(lim, 1)}</span></span>
                            </div>
                            <Bar value={v} max={lim} tone={lim && v > lim * 0.8 ? "down" : "gold"} />
                          </li>
                        );
                      })}
                    </ul>
                  )}
                  <p className="mt-3 text-xs text-fog-400">Notionnel brut des positions dans chaque devise, divisé par l'équité (sans compensation entre achats et ventes : prudent).</p>
                </Panel>
                <Panel title="Corrélations des rendements">
                  {(p.correlations ?? []).length === 0 ? <EmptyBox>Il faut au moins deux instruments avec un historique commun.</EmptyBox> : (
                    <table>
                      <thead><tr><th>Paire</th><th>ρ</th><th>Rendements</th></tr></thead>
                      <tbody>
                        {[...p.correlations].sort((a: Json, b: Json) => Math.abs(b.rho) - Math.abs(a.rho)).map((c: Json) => (
                          <tr key={`${c.a}-${c.b}`}>
                            <td className="text-xs">{c.a} / {c.b}</td>
                            <td className={cx("num", rhoTone(c.rho, threshold))}>{fmtSigned(c.rho, 2)}</td>
                            <td className="num text-xs">{c.returns}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </Panel>
              </div>
              <div className="mt-4"><Note>Le détail des règles d'exposition : <span className="font-mono">docs/MULTI_ASSET.md</span>.</Note></div>
            </>
          );
        }}
      </Loaded>
    </div>
  );
}
