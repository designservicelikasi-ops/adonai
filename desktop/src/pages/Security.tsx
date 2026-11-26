import { useEffect, useState } from "react";
import { api, Json, ProfileView } from "../api";
import { Badge, Button, KV, Panel } from "../components/ui";
import { EmptyBox, Grid, Note, PageHeader, StatCard, useCommand, useView } from "../components/kit";
import { fmtDateTime } from "../format";

export default function Security({ s, refresh }: { s: Json; refresh: () => void }) {
  const autonomy = useView("autonomy", 5000);
  const data = useView("data", 10000);
  const [profile, setProfile] = useState<ProfileView | null>(null);
  const { run, busy } = useCommand(() => { data.refresh(); refresh(); });
  useEffect(() => { api.profileLoad().then(setProfile).catch(() => setProfile(null)); }, []);
  if (!s) return <PageHeader title="Sécurité" subtitle="Connexion au moteur…" />;
  const b = s.bridge ?? {};
  const d = data.data;
  const refused: Json[] = (autonomy.data?.events ?? []).filter((e: Json) => !e.allowed);
  const auditOk = d ? !d.audit_error : null;
  return (
    <div>
      <PageHeader
        title="Sécurité"
        subtitle="Qui peut parler au moteur, avec quelle clé, et la preuve que le journal n'a pas été modifié. Aucun secret n'est affiché ici."
      />
      <Grid cols={4}>
        <StatCard label="Journal d'audit" value={auditOk == null ? "—" : auditOk ? "intègre" : "ALTÉRÉ"} tone={auditOk === false ? "text-down-400" : auditOk ? "text-up-400" : undefined} sub={d?.audit_entries_verified != null ? `${d.audit_entries_verified} entrées vérifiées` : "pas encore vérifié"} accent />
        <StatCard label="Refus de permission" value={refused.length} sub="par l'Autonomy Commander" tone={refused.length ? "text-warn-400" : undefined} />
        <StatCard label="Connexions MT5 refusées" value={(b.handshakes_refused ?? 0) + (b.protocol_violations ?? 0)} sub={`${b.handshakes_refused ?? 0} poignée(s) de main · ${b.protocol_violations ?? 0} violation(s)`} tone={(b.handshakes_refused ?? 0) + (b.protocol_violations ?? 0) ? "text-warn-400" : undefined} />
        <StatCard label="LIVE" value={s.live_trading_enabled_in_config ? "autorisable" : "verrouillé"} sub={s.live_trading_enabled_in_config ? (s.live_armed ? "armé" : "désarmé") : "désactivé dans la configuration"} tone={s.live_trading_enabled_in_config ? "text-warn-400" : "text-up-400"} />
      </Grid>

      <div className="mt-4 grid gap-4 xl:grid-cols-2">
        <Panel title="Journal d'audit chaîné">
          <p className="mb-3 text-sm text-fog-300">Chaque entrée contient l'empreinte SHA-256 de la précédente : modifier ou supprimer une ligne casse la chaîne, et la vérification le signale.</p>
          {d?.audit_error && <div className="mb-3"><Note tone="warn">{d.audit_error}</Note></div>}
          <Button kind="primary" disabled={busy} onClick={() => run("verify_audit")}>Vérifier la chaîne maintenant</Button>
        </Panel>
        <Panel title="Accès">
          <KV rows={[
            ["Moteur (API)", <span className="font-mono text-xs">{profile?.url ?? "—"}</span>],
            ["Certificat épinglé", <span className="font-mono text-xs">{profile?.fingerprint ? `${profile.fingerprint.slice(0, 23)}…` : "—"}</span>],
            ["Jeton d'accès", profile?.has_token ? "enregistré (jamais affiché)" : "absent"],
            ["Pont MT5", <span className="font-mono text-xs">{b.listening || "—"}</span>],
            ["Clé partagée MT5", <span className="text-xs">{b.key_source || "—"}</span>],
          ]} />
          <p className="mt-3 text-xs text-fog-400">Le desktop n'accepte que le certificat dont l'empreinte a été enregistrée à l'appairage ; la liaison MT5 est authentifiée (HMAC) mais non chiffrée.</p>
        </Panel>
      </div>

      <Panel title="Demandes refusées" className="mt-4" pad={false}>
        {refused.length === 0 ? <EmptyBox>Aucune demande refusée.</EmptyBox> : (
          <div className="max-h-80 overflow-y-auto">
            <table>
              <thead><tr><th>Heure</th><th>Demandeur</th><th>Capacité</th><th>Profil</th><th>Motif</th></tr></thead>
              <tbody>
                {refused.map((e) => (
                  <tr key={e.id}>
                    <td className="num text-xs">{fmtDateTime(e.ts_ms)}</td>
                    <td className="text-xs">{e.requester || e.actor}</td>
                    <td className="font-mono text-xs">{e.capability || "—"}</td>
                    <td className="text-xs">{e.profile}</td>
                    <td className="text-xs">{e.detail}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      <div className="mt-4 flex flex-wrap gap-2">
        <Badge tone="muted">Démarrage toujours en MONITOR</Badge>
        <Badge tone="muted">Autorisation d'ordre signée, 3 s, usage unique</Badge>
        <Badge tone="muted">Aucune IA ne peut s'accorder une permission</Badge>
        <Badge tone="muted">LIVE jamais restauré après un redémarrage</Badge>
      </div>
    </div>
  );
}
