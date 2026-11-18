import { api, Json } from "../api";
import { Badge, Empty, Panel, usePoll } from "../components/ui";
import { ago, fmtDateTime } from "../format";

const recTone = (r: string) => (r === "allowed" ? "ok" : r === "caution" ? "warn" : "bad");
const recLabel = (r: string) => (r === "allowed" ? "trading autorisé" : r === "caution" ? "trading prudent" : "trading suspendu");

export default function News(_: { s: Json | null }) {
  const n = usePoll(() => api.view("news"), 5000);
  const d = n.data;
  if (!d) return <Empty>{n.error ?? "Chargement…"}</Empty>;
  return (
    <div className="space-y-5">
      <Panel title="News Intelligence" right={<Badge tone={!d.enabled ? "muted" : d.feed_ok ? "ok" : "warn"}>{!d.enabled ? "désactivé" : d.feed_ok ? `calendrier à jour (${ago(d.last_ok_ms)})` : "calendrier indisponible"}</Badge>}>
        <p className="text-sm text-fog-300">
          Événements lus depuis les sources configurées uniquement. Autour d'une annonce à fort impact, les entrées sur les devises concernées sont suspendues ; autour d'une annonce moyenne, une probabilité plus élevée est exigée. Un calendrier injoignable n'est jamais considéré comme « calme ». Aucune valeur publiée n'est inventée : si la source ne la fournit pas, elle reste vide.
        </p>
        {d.last_error && <p className="mt-2 text-xs text-down-400">Dernière erreur : {d.last_error}</p>}
      </Panel>
      <div className="grid gap-5 xl:grid-cols-3">
        <Panel title="Recommandation par instrument" className="xl:col-span-1">
          {(d.by_symbol ?? []).length === 0 ? <Empty>Aucun instrument connu.</Empty> : d.by_symbol.map(([sym, a]: [string, Json]) => (
            <div key={sym} className="mb-4">
              <div className="flex items-center justify-between"><span className="font-medium">{sym}</span><Badge tone={recTone(a.recommendation)}>{recLabel(a.recommendation)}</Badge></div>
              <ul className="mt-1 space-y-0.5 text-xs text-fog-400">{a.reasons.map((r: string, i: number) => <li key={i}>· {r}</li>)}</ul>
              {a.next && <div className="mt-1 text-[11px] text-fog-500">Prochaine : {a.next.currency} {a.next.title} — {fmtDateTime(a.next.ts_ms)}</div>}
            </div>
          ))}
        </Panel>
        <Panel title="Annonces moyennes et fortes (−2 h à +48 h)" className="xl:col-span-2" pad={false}>
          {(d.upcoming ?? []).length === 0 ? <Empty>Aucune annonce dans la fenêtre.</Empty> : (
            <table>
              <thead><tr><th>Heure</th><th>Devise</th><th>Événement</th><th>Impact</th><th>Prévision</th><th>Précédent</th><th>Publié</th><th>Source</th></tr></thead>
              <tbody>
                {d.upcoming.map((e: Json) => (
                  <tr key={e.id}>
                    <td className="num text-xs">{fmtDateTime(e.ts_ms)}</td><td className="font-medium">{e.currency}</td><td>{e.title}</td>
                    <td><Badge tone={e.impact === "high" ? "bad" : "warn"}>{e.impact}</Badge></td>
                    <td className="num">{e.forecast ?? "—"}</td><td className="num">{e.previous ?? "—"}</td><td className="num">{e.actual ?? <span className="text-fog-500">non fourni</span>}</td><td className="text-xs text-fog-500">{e.source}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Panel>
      </div>
    </div>
  );
}
