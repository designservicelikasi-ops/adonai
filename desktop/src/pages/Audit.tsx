import { useMemo, useState } from "react";
import { Json } from "../api";
import { Badge, Button, Panel, cx } from "../components/ui";
import { EmptyBox, Loaded, PageHeader, Tabs, useCommand, useView } from "../components/kit";
import { fmtDateTime, fmtPrice } from "../format";

function detail(d: string) {
  try {
    const o = JSON.parse(d);
    return typeof o === "object" && o ? Object.entries(o).map(([k, v]) => `${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`).join(" · ") : d;
  } catch {
    return d;
  }
}

// Order statuses (aegis-execution OrderStatus) and position-command statuses.
const STATUS_TONE: Record<string, "ok" | "bad" | "warn" | "muted"> = {
  filled: "ok", placed: "ok", done: "ok",
  partially_filled: "warn", authorized: "warn", sent: "warn", created: "warn", unknown: "warn",
  rejected: "bad", not_executed: "bad", failed: "bad",
};
const statusTone = (st: string) => STATUS_TONE[st] ?? "muted";

export default function Audit({ refresh }: { s: Json; refresh: () => void }) {
  const [tab, setTab] = useState<"audit" | "orders" | "commands">("audit");
  const [filter, setFilter] = useState("");
  const audit = useView("audit", 5000);
  const orders = useView("orders", 5000);
  const commands = useView("commands", 5000);
  const { run, busy } = useCommand(() => { audit.refresh(); refresh(); });
  const f = filter.trim().toLowerCase();
  const match = (o: Json) => !f || JSON.stringify(o).toLowerCase().includes(f);
  const events = useMemo(() => [...new Set(((audit.data ?? []) as Json[]).map((a) => a.event))].sort(), [audit.data]);
  return (
    <div>
      <PageHeader
        title="Audit et traçabilité"
        subtitle="Journal d'audit chaîné (200 dernières entrées), ordres envoyés au broker et commandes de gestion de position. La décision est écrite avant tout envoi ; ce qui part vers MT5 est retrouvable ici."
        right={<Button disabled={busy} onClick={() => run("verify_audit")}>Vérifier la chaîne</Button>}
      />
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Tabs value={tab} onChange={setTab} items={[["audit", `Journal (${(audit.data ?? []).length})`], ["orders", `Ordres (${(orders.data ?? []).length})`], ["commands", `Commandes (${(commands.data ?? []).length})`]]} />
        <div className="mb-4 flex gap-2">
          {tab === "audit" && events.length > 1 && (
            <select value={events.includes(filter) ? filter : ""} onChange={(e) => setFilter(e.target.value)} aria-label="événement">
              <option value="">tous les événements</option>
              {events.map((e) => <option key={e} value={e}>{e}</option>)}
            </select>
          )}
          <input placeholder="Filtrer (ticket, instrument, acteur…)" value={filter} onChange={(e) => setFilter(e.target.value)} className="w-64" />
        </div>
      </div>
      <Panel pad={false}>
        {tab === "audit" && (
          <Loaded q={audit}>
            {(rows: Json[]) => (rows.filter(match).length === 0 ? <EmptyBox>Aucune entrée.</EmptyBox> : (
              <div className="max-h-[600px] overflow-y-auto">
                <table>
                  <thead><tr><th>N°</th><th>Heure</th><th>Acteur</th><th>Événement</th><th>Détail</th></tr></thead>
                  <tbody>
                    {rows.filter(match).map((a) => (
                      <tr key={a.id}>
                        <td className="num text-xs text-fog-500">{a.id}</td>
                        <td className="num whitespace-nowrap text-xs">{fmtDateTime(a.ts_ms)}</td>
                        <td><Badge tone={a.actor === "user" ? "gold" : "muted"}>{a.actor}</Badge></td>
                        <td className="font-mono text-xs">{a.event}</td>
                        <td className="break-all text-xs text-fog-300">{detail(a.detail)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))}
          </Loaded>
        )}
        {tab === "orders" && (
          <Loaded q={orders}>
            {(rows: Json[]) => (rows.filter(match).length === 0 ? <EmptyBox>Aucun ordre envoyé.</EmptyBox> : (
              <div className="max-h-[600px] overflow-y-auto">
                <table>
                  <thead><tr><th>Créé</th><th>Instrument</th><th>Sens</th><th>Volume</th><th>Prix / SL / TP</th><th>Exécution</th><th>État</th><th>Réponse du broker</th></tr></thead>
                  <tbody>
                    {rows.filter(match).map((o) => (
                      <tr key={o.cid} title={`cid ${o.cid}\ndécision ${o.decision_id}\nautorisation ${o.auth_id}`}>
                        <td className="num whitespace-nowrap text-xs">{fmtDateTime(o.created_ms)}</td>
                        <td className="text-xs">{o.symbol} <span className="text-fog-500">{o.kind}</span></td>
                        <td><Badge tone={o.side === "buy" ? "ok" : "bad"}>{String(o.side).toUpperCase()}</Badge></td>
                        <td className="num">{o.volume}</td>
                        <td className="num text-xs">{fmtPrice(o.price)} / {fmtPrice(o.sl)} / {fmtPrice(o.tp)}</td>
                        <td className="num text-xs">{o.ticket ? `#${o.ticket} ` : ""}{o.fill_price ? `${fmtPrice(o.fill_price)} × ${o.fill_volume}` : "—"}</td>
                        <td><Badge tone={statusTone(o.status)}>{o.status}</Badge></td>
                        <td className={cx("text-xs", o.retcode && o.retcode !== 10009 ? "text-down-400" : "text-fog-400")}>{o.retcode != null ? `${o.retcode} ` : ""}{o.message ?? ""}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))}
          </Loaded>
        )}
        {tab === "commands" && (
          <Loaded q={commands}>
            {(rows: Json[]) => (rows.filter(match).length === 0 ? <EmptyBox>Aucune commande de gestion (modification, clôture, annulation).</EmptyBox> : (
              <div className="max-h-[600px] overflow-y-auto">
                <table>
                  <thead><tr><th>Créée</th><th>Action</th><th>Ticket</th><th>Instrument</th><th>Volume / SL / TP</th><th>Raison</th><th>État</th><th>Réponse</th></tr></thead>
                  <tbody>
                    {rows.filter(match).map((c) => (
                      <tr key={c.cid}>
                        <td className="num whitespace-nowrap text-xs">{fmtDateTime(c.created_ms)}</td>
                        <td className="font-mono text-xs">{c.action}</td>
                        <td className="num text-xs">#{c.ticket}</td>
                        <td className="text-xs">{c.symbol}</td>
                        <td className="num text-xs">{c.volume || "—"} / {fmtPrice(c.sl)} / {fmtPrice(c.tp)}</td>
                        <td className="text-xs text-fog-400">{c.reason}</td>
                        <td><Badge tone={statusTone(c.status)}>{c.status}</Badge></td>
                        <td className="text-xs text-fog-400">{c.retcode != null ? `${c.retcode} ` : ""}{c.message ?? ""}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))}
          </Loaded>
        )}
      </Panel>
    </div>
  );
}
