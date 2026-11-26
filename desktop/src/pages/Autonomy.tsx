import { useState } from "react";
import { Json } from "../api";
import { Badge, cx, Panel } from "../components/ui";
import { ConfirmButton, EmptyBox, Loaded, Note, PageHeader, useCommand, useView } from "../components/kit";
import { fmtDateTime } from "../format";

const PROFILES: Record<string, { label: string; tone: "muted" | "info" | "gold" | "bad"; text: string }> = {
  observer: { label: "OBSERVER", tone: "muted", text: "Analyse et affichage. Ni transaction ni recherche automatique." },
  research_autonomy: { label: "RESEARCH AUTONOMY", tone: "info", text: "Recherche automatique (entraînements, backtests, comparaisons, Shadow). Aucune transaction." },
  demo_full_autonomy: { label: "DEMO FULL AUTONOMY", tone: "gold", text: "Autonomie complète sur un compte de DÉMONSTRATION identifié, pour une durée donnée." },
  live_authorized_autonomy: { label: "LIVE AUTHORIZED AUTONOMY", tone: "bad", text: "Autonomie sur un compte RÉEL identifié : autorisation explicite, limitée dans le temps, phrase liée au compte." },
};

function remaining(expires: number | null | undefined, now: number) {
  if (!expires) return "sans échéance";
  const m = Math.round((expires - now) / 60000);
  return m <= 0 ? "expirée" : m < 120 ? `encore ${m} min` : `encore ${Math.round(m / 60)} h`;
}

export default function Autonomy({ s, refresh }: { s: Json; refresh: () => void }) {
  const q = useView("autonomy", 2000);
  const { run, busy } = useCommand(() => { q.refresh(); refresh(); });
  const [profile, setProfile] = useState<"demo_full_autonomy" | "live_authorized_autonomy">("demo_full_autonomy");
  const [minutes, setMinutes] = useState(240);
  const [persist, setPersist] = useState(false);
  const [note, setNote] = useState("");
  const [phrase, setPhrase] = useState("");
  const now = s?.now_ms ?? Date.now();
  const demoAccount = s?.account?.trade_mode === "demo";
  return (
    <div>
      <PageHeader
        title="Autonomy Commander"
        subtitle="Ce qu'AEGIS a le droit de faire seul, sur quel compte et jusqu'à quand. Toute autorisation est liée au compte connecté, datée, révocable à tout instant, et inscrite au journal."
        right={q.data && <Badge tone={PROFILES[q.data.profile]?.tone ?? "muted"}>{q.data.profile_label}</Badge>}
      />
      <Loaded q={q}>
        {(a: Json) => {
          const g = a.active_grant;
          return (
            <>
              <div className="grid gap-4 xl:grid-cols-3">
                <Panel title="Autorisation active" className="xl:col-span-2">
                  {g ? (
                    <div className="space-y-3">
                      <div className="flex flex-wrap items-center gap-3">
                        <Badge tone={PROFILES[g.profile]?.tone ?? "muted"}>{PROFILES[g.profile]?.label ?? g.profile}</Badge>
                        <span className="num text-sm">compte #{g.account_login} · {g.account_server}</span>
                        <span className="text-sm text-aegis-300">{remaining(g.expires_ms, now)}</span>
                      </div>
                      <div className="grid gap-2 text-xs text-fog-400 md:grid-cols-3">
                        <span>Accordée par {g.granted_by} le {fmtDateTime(g.granted_ms)}</span>
                        <span>Échéance {g.expires_ms ? fmtDateTime(g.expires_ms) : "aucune"}</span>
                        <span>{g.persist_across_restart ? "conservée après un redémarrage" : "perdue au redémarrage (retour en MONITOR)"}</span>
                      </div>
                      {g.note && <p className="text-xs text-fog-300">Note : {g.note}</p>}
                      {a.grant_inactive_reason && <Note tone="warn">Autorisation sans effet pour l'instant : {a.grant_inactive_reason}</Note>}
                      <ConfirmButton kind="danger" label="Révoquer maintenant" title="Révoquer l'autorisation ?" body="Le moteur repasse immédiatement en MONITOR : plus aucune nouvelle entrée. Les positions ouvertes gardent leurs stops et objectifs chez le broker." onConfirm={() => run("revoke_autonomy", { reason: "révoquée depuis AEGIS Desktop" })} />
                    </div>
                  ) : (
                    <div className="space-y-2 text-sm">
                      <p className="text-fog-300">Aucune autorisation de trading. Profil appliqué : <b>{a.profile_label}</b>.</p>
                      <p className="text-xs text-fog-400">{PROFILES[a.profile]?.text}</p>
                    </div>
                  )}
                </Panel>
                <Panel title="Profil sans autorisation">
                  <p className="mb-3 text-xs text-fog-400">Profil appliqué quand aucune autorisation n'est active ou à son expiration.</p>
                  <div className="space-y-2">
                    {(["observer", "research_autonomy"] as const).map((p) => (
                      <button key={p} disabled={busy} onClick={() => a.default_profile !== p && run("set_default_profile", { profile: p })} className={cx("w-full rounded-lg border px-3 py-2 text-left transition-colors", a.default_profile === p ? "border-aegis-500 bg-aegis-200/40" : "border-ink-700 bg-white hover:border-aegis-500/60")}>
                        <div className="text-sm font-medium">{PROFILES[p].label}</div>
                        <div className="text-xs text-fog-400">{PROFILES[p].text}</div>
                      </button>
                    ))}
                  </div>
                </Panel>
              </div>

              <Panel title="Accorder une autorisation" className="mt-4">
                <div className="grid gap-4 lg:grid-cols-2">
                  <div className="space-y-3">
                    <div className="flex gap-2">
                      {(["demo_full_autonomy", "live_authorized_autonomy"] as const).map((p) => (
                        <button key={p} onClick={() => setProfile(p)} disabled={p === "live_authorized_autonomy" && !a.live_enabled_in_config} className={cx("flex-1 rounded-lg border px-3 py-2 text-left text-sm disabled:opacity-40", profile === p ? (p === "live_authorized_autonomy" ? "border-down-500 bg-down-500/5" : "border-aegis-500 bg-aegis-200/40") : "border-ink-700 bg-white")}>
                          {PROFILES[p].label}
                        </button>
                      ))}
                    </div>
                    <p className="text-xs text-fog-400">{PROFILES[profile].text}</p>
                    {!a.live_enabled_in_config && <p className="text-xs text-fog-500">LIVE désactivé dans la configuration du serveur (<span className="font-mono">live_trading_enabled</span>).</p>}
                    <div className="flex flex-wrap items-center gap-3 text-sm">
                      <label className="flex items-center gap-2">Durée <input type="number" min={5} max={profile === "live_authorized_autonomy" ? 7 * 1440 : 365 * 1440} className="num w-24" value={minutes} onChange={(e) => setMinutes(Number(e.target.value))} /> min</label>
                      {profile === "demo_full_autonomy" && <label className="flex items-center gap-2"><input type="checkbox" checked={persist} onChange={(e) => setPersist(e.target.checked)} /> conserver après un redémarrage</label>}
                    </div>
                    <p className="text-xs text-fog-500">{profile === "live_authorized_autonomy" ? "LIVE : durée obligatoire, 7 jours au plus, liée à la configuration de risque actuelle, jamais restaurée après un redémarrage." : "DEMO : 365 jours au plus."}</p>
                    <input className="w-full" placeholder="Note (facultatif) : pourquoi cette autorisation" value={note} onChange={(e) => setNote(e.target.value)} />
                    {profile === "live_authorized_autonomy" && (
                      <div>
                        <input className="num w-full" placeholder={a.live_phrase_hint || "<login> J'ACCEPTE LE RISQUE REEL"} value={phrase} onChange={(e) => setPhrase(e.target.value)} aria-label="phrase de confirmation" />
                        <p className="mt-1 text-xs text-down-400">Tapez exactement : <span className="font-mono">{a.live_phrase_hint || "connectez d'abord le compte"}</span></p>
                      </div>
                    )}
                  </div>
                  <div className="space-y-3">
                    {profile === "demo_full_autonomy" && s?.account && !demoAccount && <Note tone="warn">Le compte connecté n'est pas un compte de démonstration : le moteur refusera DEMO FULL AUTONOMY.</Note>}
                    {!s?.account && <Note tone="warn">Aucun compte connecté : une autorisation est toujours liée à un compte précis.</Note>}
                    <Note>Une autorisation ne contourne aucune limite : chaque entrée passe encore par le Risk Guardian, et l'arrêt d'urgence reste prioritaire.</Note>
                    <ConfirmButton
                      kind={profile === "live_authorized_autonomy" ? "danger" : "primary"}
                      disabled={busy || !s?.account || (profile === "live_authorized_autonomy" && !phrase)}
                      label={`Accorder ${PROFILES[profile].label}`}
                      title={`Accorder ${PROFILES[profile].label} ?`}
                      body={<>Compte <b className="num">#{s?.account?.login}</b> ({s?.account?.server}) pendant <b>{minutes} min</b>{profile === "demo_full_autonomy" && persist ? ", conservée après un redémarrage" : ""}. AEGIS pourra ouvrir et gérer des positions seul jusqu'à l'échéance ou la révocation.</>}
                      onConfirm={async () => { if (await run("grant_autonomy", { profile, duration_minutes: minutes, persist_across_restart: profile === "demo_full_autonomy" && persist, confirmation: profile === "live_authorized_autonomy" ? phrase : "", note })) { setPhrase(""); setNote(""); } }}
                    />
                  </div>
                </div>
              </Panel>

              <div className="mt-4 grid gap-4 xl:grid-cols-2">
                <Panel title="Capacités du profil actuel">
                  <ul className="grid gap-1.5 sm:grid-cols-2">
                    {(a.capabilities ?? []).map((c: Json) => (
                      <li key={c.capability} className="flex items-center gap-2 text-sm">
                        <span className={cx("h-2 w-2 shrink-0 rounded-full", c.allowed ? (c.trading ? "bg-aegis-500" : "bg-up-500") : "bg-ink-600")} aria-label={c.allowed ? "autorisé" : "refusé"} />
                        <span className={c.allowed ? "text-fog-100" : "text-fog-500 line-through decoration-ink-500"}>{c.label}</span>
                        {c.trading && <Badge tone={c.allowed ? "gold" : "muted"}>trading</Badge>}
                      </li>
                    ))}
                  </ul>
                </Panel>
                <Panel title="Autorisations (historique)">
                  {(a.grants ?? []).length === 0 ? <EmptyBox>Aucune autorisation accordée.</EmptyBox> : (
                    <div className="max-h-72 overflow-y-auto">
                      <table>
                        <thead><tr><th>Profil</th><th>Compte</th><th>Accordée</th><th>Échéance</th><th>État</th></tr></thead>
                        <tbody>
                          {a.grants.map((r: Json) => (
                            <tr key={r.grant_id}>
                              <td><Badge tone={PROFILES[r.profile]?.tone ?? "muted"}>{PROFILES[r.profile]?.label ?? r.profile}</Badge></td>
                              <td className="num text-xs">#{r.account_login}</td>
                              <td className="num text-xs">{fmtDateTime(r.granted_ms)}</td>
                              <td className="num text-xs">{r.expires_ms ? fmtDateTime(r.expires_ms) : "—"}</td>
                              <td className="text-xs">{r.revoked_ms ? <span className="text-fog-400">révoquée {fmtDateTime(r.revoked_ms)}{r.revoke_reason ? ` — ${r.revoke_reason}` : ""}</span> : r.expires_ms && r.expires_ms < now ? <span className="text-fog-400">expirée</span> : <span className="text-aegis-300">active</span>}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </Panel>
              </div>

              <Panel title="Journal des permissions" className="mt-4">
                {(a.events ?? []).length === 0 ? <EmptyBox>Aucun événement.</EmptyBox> : (
                  <div className="max-h-80 overflow-y-auto">
                    <table>
                      <thead><tr><th>Heure</th><th>Événement</th><th>Acteur</th><th>Capacité</th><th>Résultat</th><th>Détail</th></tr></thead>
                      <tbody>
                        {a.events.map((e: Json) => (
                          <tr key={e.id}>
                            <td className="num text-xs">{fmtDateTime(e.ts_ms)}</td>
                            <td className="font-mono text-xs">{e.event}</td>
                            <td className="text-xs">{e.actor}{e.requester ? ` / ${e.requester}` : ""}</td>
                            <td className="font-mono text-xs">{e.capability || "—"}</td>
                            <td>{e.allowed ? <Badge tone="ok">autorisé</Badge> : <Badge tone="bad">refusé</Badge>}</td>
                            <td className="text-xs">{e.detail}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </Panel>
            </>
          );
        }}
      </Loaded>
    </div>
  );
}
