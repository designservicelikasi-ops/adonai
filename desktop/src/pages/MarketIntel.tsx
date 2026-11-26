import { Json } from "../api";
import { Badge, Panel, cx } from "../components/ui";
import { EmptyBox, Loaded, Note, PageHeader, useView } from "../components/kit";
import { fmtDateTime, fmtNum } from "../format";

export const REGIME: Record<string, string> = {
  trend_up: "Tendance haussière",
  trend_down: "Tendance baissière",
  transition: "Transition",
  consolidation: "Consolidation",
  range: "Marché latéral",
  chaotic: "Désordonné",
  unknown: "Indéterminé",
};
const ROLE: Record<string, string> = { context: "Contexte", structure: "Structure", confirmation: "Confirmation", setup: "Préparation", trigger: "Précision" };
const BIAS: Record<string, [string, "ok" | "bad" | "muted"]> = { bullish: ["haussier", "ok"], bearish: ["baissier", "bad"], neutral: ["neutre", "muted"] };

export function RegimeBadge({ r }: { r: Json }) {
  const k = r?.kind ?? "unknown";
  const t = k === "trend_up" ? "ok" : k === "trend_down" ? "bad" : k === "chaotic" ? "bad" : k === "transition" ? "warn" : "muted";
  return <Badge tone={t}>{REGIME[k] ?? k}</Badge>;
}

export default function MarketIntel() {
  const q = useView("scanner", 4000);
  return (
    <div>
      <PageHeader
        title="Market Intelligence"
        subtitle="Régime détaillé et lecture multi-unités de chaque instrument, calculés sur bougies clôturées. H4 et D1 sont reconstruits depuis les bougies H1 du moteur (UTC). Une unité sans assez de données est « non disponible », jamais comptée comme un accord."
      />
      <Loaded q={q}>
        {(d) => (
          <>
            {(d.mtf ?? []).length === 0 && <EmptyBox>Aucune analyse encore : elle a lieu à chaque bougie clôturée d'une unité de signal.</EmptyBox>}
            {(d.mtf ?? []).map(([symbol, reads]: [string, Json[]]) => (
              <Panel key={symbol} title={`${symbol} — lecture multi-unités`} className="mb-4">
                <div className="grid gap-3 md:grid-cols-4 xl:grid-cols-7">
                  {reads.map((r) => {
                    const [bl, bt] = BIAS[r.bias] ?? ["—", "muted"];
                    return (
                      <div key={r.timeframe} className={cx("rounded-lg border p-3", r.available ? "border-ink-700 bg-white" : "border-dashed border-ink-600 bg-ink-900")}>
                        <div className="flex items-center justify-between">
                          <span className="font-semibold">{r.timeframe}</span>
                          <span className="text-[10px] uppercase tracking-wider text-fog-500">{ROLE[r.role] ?? r.role}</span>
                        </div>
                        {r.available ? (
                          <div className="mt-2 space-y-1.5 text-xs">
                            <div><Badge tone={bt}>{bl}</Badge></div>
                            <div><RegimeBadge r={r.regime} /></div>
                            <div className="text-fog-400">ER {fmtNum(r.regime?.efficiency, 2)} · vol ×{fmtNum(r.regime?.volatility_ratio, 2)}</div>
                            {(r.setups ?? []).length > 0 && <div className="text-fog-400">{r.setups.map((x: Json) => x[0]).join(", ")}</div>}
                          </div>
                        ) : (
                          <div className="mt-2 text-xs text-fog-500">non disponible ({r.bars} bougies)</div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </Panel>
            ))}
            <div className="grid gap-4 xl:grid-cols-2">
              <Panel title="Régime par instrument et unité de signal">
                {(d.regimes ?? []).length === 0 ? <EmptyBox /> : (
                  <table>
                    <thead><tr><th>Instrument</th><th>Unité</th><th>Régime</th><th>Volatilité</th><th>Dynamique</th><th>Conditions</th></tr></thead>
                    <tbody>
                      {d.regimes.map(([s, tf, r]: [string, string, Json]) => (
                        <tr key={s + tf}>
                          <td className="font-medium">{s}</td><td>{tf}</td><td><RegimeBadge r={r} /></td>
                          <td className="text-xs">{r.volatility}</td><td className="text-xs">{r.dynamics}</td>
                          <td className="text-xs text-warn-400">{(r.flags ?? []).join(", ") || "—"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </Panel>
              <Panel title="Historique des régimes">
                {(d.regime_history ?? []).length === 0 ? <EmptyBox>Aucun changement enregistré.</EmptyBox> : (
                  <div className="max-h-80 overflow-y-auto">
                    <table>
                      <thead><tr><th>Heure</th><th>Instrument</th><th>Changement</th></tr></thead>
                      <tbody>
                        {d.regime_history.map((h: Json, i: number) => (
                          <tr key={i}><td className="num text-xs">{fmtDateTime(h.ts_ms)}</td><td>{h.symbol} {h.timeframe}</td><td className="text-xs">{REGIME[h.previous] ?? (h.previous || "—")} → <span className="font-medium">{REGIME[h.regime] ?? h.regime}</span></td></tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </Panel>
            </div>
            <Note>Effets sur les entrées : marché désordonné → refus {d.settings?.block_chaotic ? "(activé)" : "(désactivé)"} ; transition → probabilité exigée + {fmtNum(d.settings?.transition_probability_margin, 2)} ; contradictions multi-unités {d.settings?.require_mtf_alignment ? "bloquantes" : "affichées (non bloquantes)"}.</Note>
          </>
        )}
      </Loaded>
    </div>
  );
}
