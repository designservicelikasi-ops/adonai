import { useState } from "react";
import { api, Json } from "../api";
import { Badge, Button, Empty, Modal, Panel, cx, usePoll, useToast } from "../components/ui";
import { fmtDateTime, fmtNum, fmtPct, fmtSigned, tone } from "../format";

const TABS = [
  ["positions", "Positions"],
  ["pending", "Ordres en attente"],
  ["orders", "Ordres"],
  ["commands", "Commandes"],
  ["journal", "Journal des transactions"],
] as const;

const statusTone = (st: string) => (st === "filled" || st === "done" ? "ok" : st === "rejected" || st === "failed" ? "bad" : st === "unknown" ? "warn" : "muted");

export default function Trading({ s, refresh }: { s: Json | null; refresh: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<(typeof TABS)[number][0]>("positions");
  const orders = usePoll(() => (tab === "orders" ? api.view("orders") : Promise.resolve(null)), 3000, [tab]);
  const commands = usePoll(() => (tab === "commands" ? api.view("commands") : Promise.resolve(null)), 3000, [tab]);
  const journal = usePoll(() => (tab === "journal" ? api.view("journal") : Promise.resolve(null)), 3000, [tab]);
  const [edit, setEdit] = useState<Json | null>(null);
  const [sl, setSl] = useState("");
  const [tp, setTp] = useState("");
  const canExecute = s?.mode && s.mode !== "monitor";

  const run = async (type: string, args: Json) => {
    try {
      toast("ok", await api.command(type, args));
      refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };

  const positions: Json[] = s?.positions ?? [];
  const pending: Json[] = s?.pending ?? [];

  return (
    <div className="space-y-5">
      <div className="flex gap-1">
        {TABS.map(([id, label]) => (
          <button key={id} onClick={() => setTab(id)} className={cx("rounded-md px-3 py-1.5 text-sm", tab === id ? "bg-aegis-500/15 text-aegis-300" : "text-fog-400 hover:bg-ink-800")}>{label}</button>
        ))}
      </div>
      {!canExecute && (tab === "positions" || tab === "pending") && (
        <div className="rounded-lg border border-ink-600 bg-ink-850 px-4 py-2 text-xs text-fog-400">Mode MONITOR : aucune instruction ne part vers le broker. Les actions manuelles demandent DEMO AUTO ou LIVE AUTO (ou se font dans MT5).</div>
      )}
      {tab === "positions" && (
        <Panel title="Positions ouvertes" pad={false}>
          {positions.length === 0 ? <Empty>Aucune position.</Empty> : (
            <table>
              <thead><tr><th>Ticket</th><th>Instrument</th><th>Sens</th><th>Volume</th><th>Entrée</th><th>SL</th><th>TP</th><th>Profit</th><th>Swap</th><th>Commission</th><th>Ouverte</th><th></th></tr></thead>
              <tbody>
                {positions.map((p) => (
                  <tr key={p.ticket}>
                    <td className="num">{p.ticket}</td>
                    <td>{p.symbol}{(s?.foreign_tickets ?? []).includes(p.ticket) && <div className="text-[11px] text-warn-400">hors AEGIS : jamais gérée</div>}</td>
                    <td><Badge tone={p.side === "buy" ? "ok" : "bad"}>{p.side.toUpperCase()}</Badge></td>
                    <td className="num">{p.volume}</td>
                    <td className="num">{p.price_open}</td>
                    <td className="num">{p.sl || "—"}</td>
                    <td className="num">{p.tp || "—"}</td>
                    <td className={cx("num", tone(p.profit))}>{fmtSigned(p.profit)}</td>
                    <td className="num">{fmtSigned(p.swap)}</td>
                    <td className="num">{fmtSigned(p.commission)}</td>
                    <td className="num text-xs">{fmtDateTime(p.open_ts)}</td>
                    <td className="whitespace-nowrap">
                      <Button small disabled={!canExecute} onClick={() => { setEdit(p); setSl(String(p.sl || "")); setTp(String(p.tp || "")); }}>SL/TP</Button>{" "}
                      <Button small kind="danger" disabled={!canExecute} onClick={() => run("close_position", { ticket: p.ticket, volume: null })}>Clôturer</Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Panel>
      )}
      {tab === "pending" && (
        <Panel title="Ordres en attente" pad={false}>
          {pending.length === 0 ? <Empty>Aucun ordre en attente.</Empty> : (
            <table>
              <thead><tr><th>Ticket</th><th>Instrument</th><th>Type</th><th>Volume</th><th>Prix</th><th>SL / TP</th><th></th></tr></thead>
              <tbody>
                {pending.map((o) => (
                  <tr key={o.ticket}>
                    <td className="num">{o.ticket}</td><td>{o.symbol}</td><td>{o.side} {o.kind}</td><td className="num">{o.volume}</td><td className="num">{o.price}</td><td className="num text-xs">{o.sl} / {o.tp}</td>
                    <td><Button small kind="danger" disabled={!canExecute} onClick={() => run("cancel_order", { ticket: o.ticket })}>Annuler</Button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Panel>
      )}
      {tab === "orders" && (
        <Panel title="Ordres d'entrée (le broker fait foi)" pad={false}>
          {!orders.data?.length ? <Empty>Aucun ordre.</Empty> : (
            <div className="max-h-[70vh] overflow-auto"><table>
              <thead><tr><th>Créé</th><th>CID</th><th>Instrument</th><th>Sens</th><th>Volume</th><th>Prix demandé</th><th>Exécution</th><th>Statut</th><th>Message</th></tr></thead>
              <tbody>
                {orders.data.map((o: Json) => (
                  <tr key={o.cid}>
                    <td className="num text-xs">{fmtDateTime(o.created_ms)}</td><td className="num text-xs">{o.cid}</td><td>{o.symbol}</td><td>{o.side}</td><td className="num">{o.volume}</td><td className="num">{o.price}</td>
                    <td className="num text-xs">{o.fill_price ? `${o.fill_volume} @ ${o.fill_price}` : "—"}{o.ticket ? <div className="text-fog-500">ticket {o.ticket}</div> : null}</td>
                    <td><Badge tone={statusTone(o.status)}>{o.status}</Badge></td>
                    <td className="text-xs text-fog-400">{o.message ?? ""}{o.retcode ? ` (code ${o.retcode})` : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table></div>
          )}
        </Panel>
      )}
      {tab === "commands" && (
        <Panel title="Commandes (modifications, clôtures, annulations)" pad={false}>
          {!commands.data?.length ? <Empty>Aucune commande.</Empty> : (
            <div className="max-h-[70vh] overflow-auto"><table>
              <thead><tr><th>Créée</th><th>Action</th><th>Ticket</th><th>Raison</th><th>Statut</th><th>Message</th></tr></thead>
              <tbody>
                {commands.data.map((c: Json) => (
                  <tr key={c.cid}><td className="num text-xs">{fmtDateTime(c.created_ms)}</td><td>{c.action}</td><td className="num">{c.ticket}</td><td className="text-xs text-fog-400">{c.reason}</td><td><Badge tone={statusTone(c.status)}>{c.status}</Badge></td><td className="text-xs text-fog-400">{c.message ?? ""}</td></tr>
                ))}
              </tbody>
            </table></div>
          )}
        </Panel>
      )}
      {tab === "journal" && (
        <Panel title="Journal : décision → probabilité → coûts → résultat réel" pad={false}>
          {!journal.data?.length ? <Empty>Aucune transaction AEGIS.</Empty> : (
            <div className="max-h-[70vh] overflow-auto"><table>
              <thead><tr><th>Ouverte</th><th>Ticket</th><th>Stratégie</th><th>Sens</th><th>Volume</th><th>Entrée (demandée → exécutée)</th><th>P(TP&lt;SL)</th><th>Espérance</th><th>Coûts est.</th><th>Brut</th><th>Net</th><th>Sortie</th><th>MFE / MAE</th></tr></thead>
              <tbody>
                {journal.data.map((j: Json) => (
                  <tr key={j.ticket}>
                    <td className="num text-xs">{fmtDateTime(j.opened_ms)}</td>
                    <td className="num">{j.ticket}<div className="text-[11px] text-fog-500">{j.symbol} {j.timeframe} · {j.terminal}</div></td>
                    <td className="text-xs">{j.strategy_id}<div className="text-fog-500">{j.regime}</div></td>
                    <td>{j.side}</td>
                    <td className="num">{j.volume}</td>
                    <td className="num text-xs">{j.entry_requested} → {j.entry_filled}</td>
                    <td className="num" title={j.probability_source}>{fmtPct(j.probability)}</td>
                    <td className={cx("num", tone(j.expected_net))}>{fmtSigned(j.expected_net)}</td>
                    <td className="num text-warn-400">{fmtNum(j.est_spread_cost + j.est_commission)}</td>
                    <td className={cx("num", tone(j.gross))}>{j.closed_ms ? fmtSigned(j.gross) : "ouverte"}</td>
                    <td className={cx("num font-medium", tone(j.net))}>{j.closed_ms ? fmtSigned(j.net) : "—"}</td>
                    <td className="text-xs">{j.exit_reason}</td>
                    <td className="num text-xs">{j.mfe_price.toPrecision(3)} / {j.mae_price.toPrecision(3)}</td>
                  </tr>
                ))}
              </tbody>
            </table></div>
          )}
        </Panel>
      )}
      <Modal open={!!edit} title={`Modifier SL/TP — position ${edit?.ticket}`} onClose={() => setEdit(null)}>
        <p className="mb-3 text-xs text-fog-400">Le stop-loss ne peut être déplacé que dans le sens protecteur : le moteur refuse tout élargissement.</p>
        <div className="mb-4 grid grid-cols-2 gap-3">
          <label className="text-xs text-fog-400">Stop-loss<input className="mt-1 w-full num" value={sl} onChange={(e) => setSl(e.target.value)} /></label>
          <label className="text-xs text-fog-400">Take-profit<input className="mt-1 w-full num" value={tp} onChange={(e) => setTp(e.target.value)} /></label>
        </div>
        <div className="flex justify-end gap-2">
          <Button kind="ghost" onClick={() => setEdit(null)}>Annuler</Button>
          <Button kind="primary" onClick={async () => { await run("modify_position", { ticket: edit.ticket, sl: Number(sl), tp: Number(tp) }); setEdit(null); }}>Envoyer</Button>
        </div>
      </Modal>
    </div>
  );
}
