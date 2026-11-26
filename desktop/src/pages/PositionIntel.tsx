import { Json } from "../api";
import { Badge, Panel, cx } from "../components/ui";
import { ConfirmButton, EmptyBox, Note, PageHeader, useCommand, useView } from "../components/kit";
import { ago, fmtDateTime, fmtPrice, fmtSigned, tone } from "../format";

const ACTION_TONE: Record<string, "ok" | "gold" | "warn" | "bad" | "muted"> = { hold: "muted", secure: "ok", adjust_sl: "gold", adjust_tp: "gold", close_partial: "warn", close: "bad" };

export default function PositionIntel({ s, refresh }: { s: Json; refresh: () => void }) {
  const gd = useView("guardian", 3000);
  const journal = useView("journal", 5000);
  const { run } = useCommand(refresh);
  if (!s) return null;
  const foreign = new Set<number>(s.foreign_tickets ?? []);
  const specs: Record<string, Json> = Object.fromEntries((s.specs ?? []).map((x: Json) => [x.symbol, x]));
  const unprotected: number[] = gd.data?.unprotected_tickets ?? [];
  return (
    <div>
      <PageHeader
        title="Position Intelligence"
        subtitle="Chaque position AEGIS revue à chaque bougie : HOLD, SECURE, stop suiveur, cible rapprochée, clôture partielle, clôture. Un stop n'est jamais élargi ; les modifications sont vérifiées dans l'instantané du broker."
      />
      {unprotected.length > 0 && <Note tone="warn">Positions SANS stop-loss chez le broker : {unprotected.join(", ")} — la garde de protection applique sa politique.</Note>}
      {(s.positions ?? []).length === 0 ? <EmptyBox>Aucune position ouverte.</EmptyBox> : (
        <div className="space-y-3">
          {(s.positions as Json[]).map((p) => {
            const d = (s.position_decisions ?? []).find((x: Json) => x.ticket === p.ticket);
            const j = (journal.data ?? []).find((x: Json) => x.ticket === p.ticket);
            const spec = specs[p.symbol];
            const digits = spec?.digits ?? 5;
            const mine = !foreign.has(p.ticket);
            const half = spec ? Number((Math.floor(p.volume / 2 / spec.volume_step) * spec.volume_step).toFixed(8)) : 0;
            return (
              <Panel key={p.ticket} title={`${p.symbol} — ticket ${p.ticket}`} right={<>{mine ? <Badge tone="gold">AEGIS</Badge> : <Badge tone="muted">hors AEGIS</Badge>} {unprotected.includes(p.ticket) && <Badge tone="bad">SANS STOP</Badge>}</>}>
                <div className="grid gap-4 text-sm md:grid-cols-6">
                  <div><div className="text-[11px] uppercase text-fog-500">Direction / volume</div><Badge tone={p.side === "buy" ? "ok" : "bad"}>{String(p.side).toUpperCase()}</Badge> <span className="num">{p.volume}</span></div>
                  <div><div className="text-[11px] uppercase text-fog-500">Entrée</div><span className="num">{fmtPrice(p.price_open, digits)}</span></div>
                  <div><div className="text-[11px] uppercase text-fog-500">SL initial → actuel</div><span className="num">{fmtPrice(d?.initial_sl ?? j?.sl_initial, digits)} → {fmtPrice(p.sl, digits)}</span></div>
                  <div><div className="text-[11px] uppercase text-fog-500">TP</div><span className="num">{fmtPrice(p.tp, digits)}</span></div>
                  <div><div className="text-[11px] uppercase text-fog-500">P&L brut / net estimé</div><span className={cx("num", tone(p.profit))}>{fmtSigned(p.profit)}</span> / <span className={cx("num", tone(d?.net_realizable))}>{fmtSigned(d?.net_realizable)}</span></div>
                  <div><div className="text-[11px] uppercase text-fog-500">Durée</div><span>{ago(p.open_ts, s.now_ms).replace("il y a ", "")}</span></div>
                </div>
                {j && (
                  <div className="mt-3 grid gap-2 text-xs text-fog-400 md:grid-cols-4">
                    <span>Stratégie <span className="font-mono text-fog-300">{j.strategy_id}</span></span>
                    <span>Probabilité initiale {j.probability == null ? "—" : `${(j.probability * 100).toFixed(0)} %`}</span>
                    <span>Régime {j.regime_detail || j.regime}</span>
                    <span>Stop {j.stop_role || "stratégie"} · entrée {j.entry_kind || "normal"}</span>
                  </div>
                )}
                {d && (
                  <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
                    <Badge tone={ACTION_TONE[d.action.kind] ?? "muted"}>{String(d.action.kind).toUpperCase().replace("_", " ")}</Badge>
                    <span className="text-fog-300">{d.action.reason}</span>
                    <span className="text-fog-500">{d.executed ? "exécuté" : d.note} · {fmtDateTime(d.ts_ms)}</span>
                  </div>
                )}
                {mine && (
                  <div className="mt-3 flex gap-2">
                    <ConfirmButton small kind="danger" label="Clôturer" title={`Clôturer la position ${p.ticket} ?`} body="Ordre de clôture au marché (refusé en MONITOR). Le résultat est celui que le broker applique." onConfirm={() => run("close_position", { ticket: p.ticket, volume: null })} />
                    {spec && half >= spec.volume_min && p.volume - half >= spec.volume_min && (
                      <ConfirmButton small label="Clôturer la moitié" title="Clôture partielle" body={`Clôture de ${half} lot(s), arrondi au pas du broker.`} onConfirm={() => run("close_position", { ticket: p.ticket, volume: half })} />
                    )}
                  </div>
                )}
              </Panel>
            );
          })}
        </div>
      )}
      <Panel title="Incidents de protection" className="mt-4">
        {(gd.data?.incidents ?? []).length === 0 ? <EmptyBox>Aucun incident.</EmptyBox> : (
          <table>
            <thead><tr><th>Heure</th><th>Ticket</th><th>Type</th><th>Détail</th></tr></thead>
            <tbody>{gd.data.incidents.map((i: Json, k: number) => <tr key={k}><td className="num text-xs">{fmtDateTime(i.ts_ms)}</td><td className="num">{i.ticket}</td><td className="font-mono text-xs">{i.kind}</td><td className="text-xs">{i.detail}</td></tr>)}</tbody>
          </table>
        )}
        <p className="mt-2 text-xs text-fog-400">Modifications vérifiées : {gd.data?.verified_modifications ?? 0} · non constatées : {gd.data?.unverified_modifications ?? 0} · en attente : {gd.data?.pending_verifications ?? 0}</p>
      </Panel>
    </div>
  );
}
