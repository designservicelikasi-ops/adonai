import { useState } from "react";
import { Json } from "../api";
import { Badge, Panel, cx } from "../components/ui";
import { EmptyBox, Grid, Note, PageHeader, StatCard, Tabs } from "../components/kit";
import { ago, fmtDateTime, fmtTime } from "../format";

export default function Notifications({ s }: { s: Json }) {
  const [tab, setTab] = useState<"alerts" | "warnings">("alerts");
  const [source, setSource] = useState("");
  if (!s) return <PageHeader title="Notifications" subtitle="Connexion au moteur…" />;
  const alerts: Json[] = s.alerts ?? [];
  const warnings: Json[] = (s.logs ?? []).filter((l: Json) => l.level === "warn" || l.level === "error");
  const list = tab === "alerts" ? alerts : warnings;
  const sources = [...new Set(list.map((x) => x.source))].sort();
  const shown = source ? list.filter((x) => x.source === source) : list;
  const critical = alerts.filter((a) => a.level === "critical").length;
  return (
    <div>
      <PageHeader
        title="Notifications"
        subtitle="Alertes émises par le moteur (déconnexion, stop manquant, autorisation terminée, sauvegarde, base de données…) et avertissements du journal. Rien ici n'est généré par l'interface."
      />
      <Grid cols={3}>
        <StatCard label="Alertes critiques" value={critical} tone={critical ? "text-down-400" : "text-up-400"} sub={critical ? "à examiner" : "aucune"} accent={critical > 0} />
        <StatCard label="Alertes" value={alerts.length} sub={alerts[0] ? `dernière ${ago(alerts[0].ts_ms, s.now_ms)}` : "aucune depuis le démarrage"} />
        <StatCard label="Avertissements récents" value={warnings.length} sub="dans le journal en mémoire" />
      </Grid>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
        <Tabs value={tab} onChange={(t) => { setTab(t); setSource(""); }} items={[["alerts", `Alertes (${alerts.length})`], ["warnings", `Avertissements (${warnings.length})`]]} />
        {sources.length > 1 && (
          <select value={source} onChange={(e) => setSource(e.target.value)} aria-label="source" className="mb-4">
            <option value="">toutes les sources</option>
            {sources.map((x) => <option key={x} value={x}>{x}</option>)}
          </select>
        )}
      </div>
      <Panel pad={false}>
        {shown.length === 0 ? <EmptyBox>{tab === "alerts" ? "Aucune alerte depuis le démarrage du moteur." : "Aucun avertissement."}</EmptyBox> : (
          <div className="max-h-[560px] overflow-y-auto">
            <table>
              <thead><tr><th>Heure</th><th>Niveau</th><th>Source</th><th>Message</th></tr></thead>
              <tbody>
                {shown.map((a: Json, i: number) => (
                  <tr key={i}>
                    <td className="num whitespace-nowrap text-xs">{tab === "alerts" ? fmtDateTime(a.ts_ms) : fmtTime(a.ts_ms)}</td>
                    <td><Badge tone={a.level === "critical" || a.level === "error" ? "bad" : "warn"}>{a.level}</Badge></td>
                    <td className="font-mono text-xs">{a.source}</td>
                    <td className={cx("text-sm", (a.level === "critical" || a.level === "error") && "text-down-400")}>{a.text}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      <div className="mt-4">
        <Note>Envoi externe : le moteur peut poster chaque alerte en JSON vers une adresse https (<span className="font-mono">[ops] alert_webhook_url</span> dans la configuration du serveur), par exemple un relais vers une messagerie. Le moteur garde les 100 dernières alertes en mémoire.</Note>
      </div>
    </div>
  );
}
