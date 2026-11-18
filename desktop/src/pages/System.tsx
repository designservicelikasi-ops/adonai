import { useState } from "react";
import { api, Json } from "../api";
import { Badge, Bar, Button, Empty, KV, Panel, cx, usePoll, useToast } from "../components/ui";
import { ago, fmtDateTime, fmtNum, fmtTime } from "../format";

const healthTone = (s: string) => (s === "ok" ? "ok" : s === "degraded" ? "warn" : s === "down" ? "bad" : "muted");

export default function System({ s, refresh }: { s: Json | null; refresh: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<"logs" | "audit">("logs");
  const logs = usePoll(() => (tab === "logs" ? api.view("logs") : Promise.resolve(null)), 3000, [tab]);
  const audit = usePoll(() => (tab === "audit" ? api.view("audit") : Promise.resolve(null)), 5000, [tab]);
  const data = usePoll(() => api.view("data"), 10000);
  const sys = s?.system;
  const run = async (type: string) => {
    try {
      toast("ok", await api.command(type));
      data.refresh();
      refresh();
    } catch (e) {
      toast("bad", String((e as Error).message));
    }
  };
  if (!s) return <Empty>Chargement…</Empty>;
  return (
    <div className="space-y-5">
      <div className="grid gap-5 xl:grid-cols-3">
        <Panel title="État des modules">
          <div className="space-y-1.5">
            {(s.health ?? []).map((h: Json) => (
              <div key={h.name} className="flex items-start justify-between gap-3 text-sm">
                <span>{h.name}</span>
                <span className="text-right"><Badge tone={healthTone(h.status)}>{h.status}</Badge><div className="max-w-[240px] text-[11px] text-fog-500">{h.detail}</div></span>
              </div>
            ))}
          </div>
        </Panel>
        <Panel title="Serveur (VPS)">
          {sys ? (
            <div className="space-y-3">
              <div><div className="mb-1 flex justify-between text-xs"><span className="text-fog-400">CPU machine</span><span className="num">{fmtNum(sys.system_cpu_pct, 1)} %</span></div><Bar value={sys.system_cpu_pct} max={100} tone={sys.system_cpu_pct > 85 ? "down" : "gold"} /></div>
              <div><div className="mb-1 flex justify-between text-xs"><span className="text-fog-400">Mémoire machine</span><span className="num">{fmtNum(sys.system_memory_used_mb / 1024, 1)} / {fmtNum(sys.system_memory_total_mb / 1024, 1)} Go</span></div><Bar value={sys.system_memory_used_mb} max={sys.system_memory_total_mb} tone={sys.system_memory_used_mb / sys.system_memory_total_mb > 0.9 ? "down" : "gold"} /></div>
              <KV rows={[["Moteur : CPU", `${fmtNum(sys.process_cpu_pct, 1)} %`], ["Moteur : mémoire", `${fmtNum(sys.process_memory_mb, 1)} Mo`], ["Cœurs", sys.cpus], ["Machine démarrée depuis", `${fmtNum(sys.system_uptime_s / 3600, 1)} h`], ["Moteur démarré", fmtDateTime(s.started_ms)]]} />
            </div>
          ) : (
            <Empty>Mesures système indisponibles.</Empty>
          )}
        </Panel>
        <Panel title="Liaison MT5">
          <KV
            rows={[
              ["Écoute", s.bridge?.listening],
              ["Clé partagée", s.bridge?.key_source],
              ["Connexions acceptées", s.bridge?.connections_accepted],
              ["Poignées de main refusées", s.bridge?.handshakes_refused],
              ["Violations de protocole", s.bridge?.protocol_violations],
              ["Dernier refus", s.bridge?.last_refusal || "—"],
              ["Dernière déconnexion", s.bridge?.last_disconnect || "—"],
              ["Trames reçues / envoyées", s.connection ? `${s.connection.frames_in} / ${s.connection.frames_out}` : "—"],
            ]}
          />
        </Panel>
      </div>
      <div className="grid gap-5 xl:grid-cols-3">
        <Panel title="Alertes" className="xl:col-span-2" pad={false}>
          {(s.alerts ?? []).length === 0 ? <Empty>Aucune alerte.</Empty> : (
            <table>
              <thead><tr><th>Heure</th><th>Niveau</th><th>Source</th><th>Message</th></tr></thead>
              <tbody>{s.alerts.map((a: Json, i: number) => <tr key={i}><td className="num text-xs">{fmtDateTime(a.ts_ms)}</td><td><Badge tone={a.level === "critical" ? "bad" : "warn"}>{a.level}</Badge></td><td className="text-xs">{a.source}</td><td className="text-sm">{a.text}</td></tr>)}</tbody>
            </table>
          )}
        </Panel>
        <Panel title="Données et sauvegardes">
          {data.data ? (
            <>
              <KV rows={[["Base", <Badge tone={data.data.db_ok ? "ok" : "bad"}>{data.data.db_ok ? "accessible" : "indisponible"}</Badge>], ["Fichier", <span className="text-xs">{data.data.db_path}</span>], ["Journal d'audit", data.data.audit_error ? <span className="text-down-400">ALTÉRÉ</span> : data.data.audit_entries_verified != null ? `${data.data.audit_entries_verified} entrées vérifiées` : "non vérifié"], ["Dernière sauvegarde", <span className="text-xs">{data.data.last_backup ?? "—"}</span>]]} />
              <div className="mt-3 flex gap-2"><Button small onClick={() => run("backup")}>Sauvegarder maintenant</Button><Button small onClick={() => run("verify_audit")}>Vérifier l'audit</Button></div>
              <div className="mt-3 max-h-40 overflow-auto text-[11px] num text-fog-400">{(data.data.candles ?? []).map((c: Json[]) => <div key={c.join()}>{c[0]} {c[1]} {c[2]} : {c[3]} bougies</div>)}</div>
            </>
          ) : <Empty>…</Empty>}
        </Panel>
      </div>
      <Panel
        title={tab === "logs" ? "Journal du moteur" : "Journal d'audit (chaîné SHA-256)"}
        pad={false}
        right={<div className="flex gap-1">{[["logs", "Journal"], ["audit", "Audit"]].map(([id, l]) => <button key={id} onClick={() => setTab(id as "logs" | "audit")} className={cx("rounded px-2 py-0.5 text-xs", tab === id ? "bg-aegis-500/15 text-aegis-300" : "text-fog-400")}>{l}</button>)}</div>}
      >
        <div className="max-h-[50vh] overflow-auto font-mono text-xs">
          {tab === "logs"
            ? (logs.data ?? []).map((l: Json, i: number) => (
                <div key={i} className="flex gap-3 border-b border-ink-750 px-4 py-1">
                  <span className="text-fog-500">{fmtTime(l.ts_ms)}</span>
                  <span className={cx("w-12", l.level === "error" ? "text-down-400" : l.level === "warn" ? "text-warn-400" : "text-fog-500")}>{l.level}</span>
                  <span className="w-20 text-info-400">{l.source}</span>
                  <span className="flex-1 text-fog-300">{l.text}</span>
                </div>
              ))
            : (audit.data ?? []).map((a: Json) => (
                <div key={a.id} className="flex gap-3 border-b border-ink-750 px-4 py-1">
                  <span className="text-fog-500">#{a.id}</span><span className="text-fog-500">{fmtDateTime(a.ts_ms)}</span><span className="w-20 text-info-400">{a.actor}</span><span className="w-44 text-aegis-300">{a.event}</span><span className="flex-1 truncate text-fog-400" title={a.detail}>{a.detail}</span>
                </div>
              ))}
        </div>
      </Panel>
      <div className="text-right text-[11px] text-fog-500">État publié {ago(s.now_ms)}</div>
    </div>
  );
}
