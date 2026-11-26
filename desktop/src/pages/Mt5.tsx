import { Json } from "../api";
import { Badge, KV, Panel } from "../components/ui";
import { EmptyBox, Grid, Note, PageHeader, StatCard, useView } from "../components/kit";
import { ago, fmtDateTime, fmtNum, fmtTime } from "../format";

function Q({ q, unit, digits = 1 }: { q: Json; unit: string; digits?: number }) {
  if (!q || !q.count) return <span className="text-fog-500">pas de mesure</span>;
  return <span className="num">{fmtNum(q.p50, digits)} / {fmtNum(q.p95, digits)} / {fmtNum(q.max, digits)} {unit} <span className="text-fog-500">({q.count})</span></span>;
}

export default function Mt5({ s }: { s: Json }) {
  const gd = useView("guardian", 3000);
  if (!s) return <PageHeader title="Terminal MT5" subtitle="Connexion au moteur…" />;
  const c = s.connection;
  const b = s.bridge ?? {};
  const sim = c?.terminal === "simulator";
  const ex = gd.data?.execution;
  const silence = c ? Math.max(0, (s.now_ms ?? Date.now()) - c.last_rx_ms) : null;
  return (
    <div>
      <PageHeader
        title="Terminal MT5"
        subtitle="Liaison avec le connecteur MQL5 : session authentifiée (HMAC, anti-rejeu), trames échangées, qualité d'exécution mesurée par le moteur."
        right={c ? <Badge tone={sim ? "warn" : "ok"} dot>{sim ? "SIMULATEUR (test)" : "MT5 connecté"}</Badge> : <Badge tone="bad">déconnecté</Badge>}
      />
      {sim && <div className="mb-4"><Note tone="warn">Terminal de test : le simulateur applique les règles du connecteur mais n'est pas un broker. Aucune mesure ci-dessous ne vaut pour un vrai compte.</Note></div>}
      <Grid cols={4}>
        <StatCard label="Compte" value={c ? `#${c.login}` : "—"} sub={c ? `${c.server} · ${String(c.trade_mode).toUpperCase()}` : "aucun terminal connecté"} accent={!!c} />
        <StatCard label="Dernière trame reçue" value={silence == null ? "—" : `${fmtNum(silence / 1000, 1)} s`} sub={c ? `connecté ${ago(c.connected_ms, s.now_ms)}` : ""} tone={silence != null && silence > 5000 ? "text-down-400" : undefined} />
        <StatCard label="Trames" value={c ? `${c.frames_in} / ${c.frames_out}` : "—"} sub="reçues / envoyées" />
        <StatCard label="Ordres rejetés" value={ex ? ex.rejected : "—"} sub={ex && ex.broker_time_missing ? `${ex.broker_time_missing} sans temps broker` : "réponses du broker"} tone={ex?.rejected ? "text-warn-400" : undefined} />
      </Grid>

      <div className="mt-4 grid gap-4 xl:grid-cols-2">
        <Panel title="Session">
          {c ? (
            <KV rows={[
              ["Terminal", sim ? "simulateur AEGIS" : "MetaTrader 5"],
              ["Version du connecteur", c.build || "—"],
              ["Adresse du terminal", c.peer],
              ["Connecté depuis", fmtDateTime(c.connected_ms)],
              ["Mode de marge", s.account?.margin_mode ?? "inconnu"],
              ["Trading autorisé par le terminal", s.account ? (s.account.trade_allowed ? "oui" : "non") : "—"],
            ]} />
          ) : <EmptyBox>Aucun terminal connecté. Lancez le connecteur AEGIS dans MetaTrader 5 (voir docs/MT5_CONNECTOR.md).</EmptyBox>}
        </Panel>
        <Panel title="Pont (bridge)">
          <KV rows={[
            ["Écoute", <span className="font-mono">{b.listening || "—"}</span>],
            ["Connexions acceptées", b.connections_accepted ?? 0],
            ["Poignées de main refusées", b.handshakes_refused ?? 0],
            ["Violations de protocole", b.protocol_violations ?? 0],
            ["Dernier refus", b.last_refusal || "—"],
            ["Dernière déconnexion", b.last_disconnect || "—"],
          ]} />
          <p className="mt-3 text-xs text-fog-400">La liaison est authentifiée mais non chiffrée : gardez-la sur la boucle locale ou dans un tunnel (WireGuard, SSH).</p>
        </Panel>
      </div>

      <Panel title="Qualité d'exécution (médiane / 95 % / max)" className="mt-4">
        {!ex ? <EmptyBox>Chargement…</EmptyBox> : (
          <>
            <KV rows={[
              ["Temps interne du moteur", <Q q={ex.internal_us} unit="µs" digits={0} />],
              ["Aller-retour complet", <Q q={ex.roundtrip_ms} unit="ms" digits={0} />],
              ["Temps broker", <Q q={ex.broker_ms} unit="ms" />],
              ["Réseau + terminal", <Q q={ex.network_ms} unit="ms" />],
              ["Slippage", <Q q={ex.slippage_points} unit="points" />],
            ]} />
            {(ex.by_symbol ?? []).length > 0 && (
              <div className="mt-3 flex flex-wrap gap-2 text-xs">
                {ex.by_symbol.map((x: Json) => <Badge key={x.symbol} tone="muted">{x.symbol} : slippage médian {fmtNum(x.median_points, 1)} pts ({x.fills})</Badge>)}
              </div>
            )}
          </>
        )}
      </Panel>
      <Panel title="Dernières requêtes" className="mt-4" pad={false}>
        {(ex?.recent ?? []).length === 0 ? <EmptyBox>Aucun ordre envoyé depuis le démarrage.</EmptyBox> : (
          <div className="max-h-80 overflow-y-auto">
            <table>
              <thead><tr><th>Heure</th><th>Action</th><th>Instrument</th><th>Interne</th><th>Aller-retour</th><th>Broker</th><th>Réseau</th><th>Résultat</th></tr></thead>
              <tbody>
                {ex.recent.map((r: Json) => (
                  <tr key={r.cid}>
                    <td className="num text-xs">{fmtTime(r.sent_ms)}</td>
                    <td className="font-mono text-xs">{r.action}</td>
                    <td className="text-xs">{r.symbol}</td>
                    <td className="num text-xs">{r.internal_us == null ? "—" : `${r.internal_us} µs`}</td>
                    <td className="num text-xs">{r.roundtrip_ms == null ? "—" : `${r.roundtrip_ms} ms`}</td>
                    <td className="num text-xs">{r.broker_ms == null ? "—" : `${fmtNum(r.broker_ms, 1)} ms`}</td>
                    <td className="num text-xs">{r.network_ms == null ? "—" : `${fmtNum(r.network_ms, 1)} ms`}</td>
                    <td>{r.ok == null ? <Badge>en attente</Badge> : r.ok ? <Badge tone="ok">ok</Badge> : <Badge tone="bad">refusé</Badge>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
