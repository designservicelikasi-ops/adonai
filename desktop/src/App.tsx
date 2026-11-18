import { useEffect, useState } from "react";
import { api, ApiError, Json, ProfileView } from "./api";
import { Badge, Button, cx, Modal, Tone, usePoll, useToast } from "./components/ui";
import { ago } from "./format";
import Connection from "./pages/Connection";
import Dashboard from "./pages/Dashboard";
import Performance from "./pages/Performance";
import Trading from "./pages/Trading";
import Decisions from "./pages/Decisions";
import Strategies from "./pages/Strategies";
import Intelligence from "./pages/Intelligence";
import News from "./pages/News";
import Settings from "./pages/Settings";
import System from "./pages/System";

const PAGES = [
  { id: "dashboard", label: "Tableau de bord", icon: "◈" },
  { id: "performance", label: "Performance", icon: "↗" },
  { id: "trading", label: "Positions & ordres", icon: "⇅" },
  { id: "decisions", label: "Décisions", icon: "⎇" },
  { id: "strategies", label: "Stratégies", icon: "⚙" },
  { id: "intelligence", label: "Probabilités & Shadow", icon: "◎" },
  { id: "news", label: "Actualités", icon: "☰" },
  { id: "settings", label: "Risque & configuration", icon: "⛨" },
  { id: "system", label: "Système", icon: "⌁" },
  { id: "connection", label: "Connexion", icon: "⇄" },
] as const;
type PageId = (typeof PAGES)[number]["id"];

export function modeTone(mode?: string): Tone {
  return mode === "live_auto" ? "bad" : mode === "demo_auto" ? "info" : "muted";
}
export function modeLabel(mode?: string) {
  return mode === "live_auto" ? "LIVE AUTO" : mode === "demo_auto" ? "DEMO AUTO" : "MONITOR";
}

function StatusBar({ s, error, onKill }: { s: Json | null; error: string | null; onKill: () => void }) {
  const conn = s?.connection;
  const now = s?.now_ms ?? Date.now();
  const latency = conn ? Math.max(0, now - conn.last_rx_ms) : null;
  const engine: [Tone, string] = error ? ["bad", "Moteur injoignable"] : s ? ["ok", `Moteur ${s.version}`] : ["muted", "Connexion…"];
  const mt5: [Tone, string] = !s ? ["muted", "—"] : conn ? [conn.terminal === "simulator" ? "warn" : "ok", conn.terminal === "simulator" ? "SIMULATEUR (test)" : "MT5 connecté"] : ["bad", "MT5 déconnecté"];
  return (
    <div className="flex h-12 items-center gap-3 border-b border-ink-700 bg-ink-900/90 px-4 backdrop-blur">
      <Badge tone={engine[0]} dot={!error && !!s}>{engine[1]}</Badge>
      <Badge tone={mt5[0]} dot={!!conn}>{mt5[1]}</Badge>
      {conn && (
        <span className="text-xs text-fog-400 num">
          {conn.server} · #{conn.login} · {String(conn.trade_mode).toUpperCase()} · latence {latency} ms
        </span>
      )}
      <div className="flex-1" />
      {s?.kill?.active && <Badge tone="bad" dot>ARRÊT D'URGENCE</Badge>}
      {s?.daily_halt && !s?.kill?.active && <Badge tone="warn">Entrées arrêtées</Badge>}
      <Badge tone={modeTone(s?.mode)}>{modeLabel(s?.mode)}</Badge>
      <Button kind="danger" small onClick={onKill} disabled={!s || s?.kill?.active} title="Bloque toute nouvelle entrée immédiatement">
        ■ Arrêt d'urgence
      </Button>
    </div>
  );
}

export default function App() {
  const toast = useToast();
  // Deep link (#page=performance) — used by the visual-QA preview.
  const initial = (new URLSearchParams(window.location.hash.slice(1)).get("page") ?? "dashboard") as PageId;
  const [page, setPage] = useState<PageId>(PAGES.some((p) => p.id === initial) ? initial : "dashboard");
  const [profile, setProfile] = useState<ProfileView | null | undefined>(undefined);
  const [killOpen, setKillOpen] = useState(false);
  const [killReason, setKillReason] = useState("");
  const configured = !!profile;
  const sum = usePoll(() => (configured ? api.summary() : Promise.resolve(null)), 1000, [configured]);

  useEffect(() => {
    api.profileLoad()
      .then((p) => {
        setProfile(p);
        if (!p) setPage("connection");
        else if (initial === "connection") setPage("connection");
      })
      .catch(() => setProfile(null));
  }, []);

  const s = sum.data;
  const unauthorized = sum.error && sum.error.startsWith("UNAUTHORIZED");

  const kill = async () => {
    try {
      toast("ok", await api.command("kill", { reason: killReason || "arrêt depuis AEGIS Desktop" }));
      setKillOpen(false);
      setKillReason("");
      sum.refresh();
    } catch (e) {
      toast("bad", (e as ApiError).message);
    }
  };

  const content = () => {
    if (profile === undefined) return null;
    if (!configured || page === "connection")
      return (
        <Connection
          profile={profile}
          onConnected={(p) => {
            setProfile(p);
            setPage("dashboard");
            sum.refresh();
          }}
          onForget={() => setProfile(null)}
        />
      );
    const props = { s, refresh: sum.refresh };
    switch (page) {
      case "dashboard":
        return <Dashboard {...props} />;
      case "performance":
        return <Performance {...props} />;
      case "trading":
        return <Trading {...props} />;
      case "decisions":
        return <Decisions {...props} />;
      case "strategies":
        return <Strategies {...props} />;
      case "intelligence":
        return <Intelligence {...props} />;
      case "news":
        return <News {...props} />;
      case "settings":
        return <Settings {...props} />;
      case "system":
        return <System {...props} />;
    }
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-60 shrink-0 flex-col border-r border-ink-700 bg-ink-950">
        <div className="flex items-center gap-3 px-5 py-5">
          <img src="/aegis-logo.svg" className="h-9 w-9" alt="" />
          <div>
            <div className="text-sm font-semibold tracking-[.35em]">AEGIS</div>
            <div className="text-[10px] text-fog-500">Intelligent Strategies</div>
          </div>
        </div>
        <nav className="flex-1 space-y-0.5 px-3">
          {PAGES.map((p) => (
            <button
              key={p.id}
              disabled={!configured && p.id !== "connection"}
              onClick={() => setPage(p.id)}
              className={cx(
                "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors disabled:opacity-30",
                page === p.id ? "bg-aegis-500/12 text-aegis-300 shadow-[inset_2px_0_0_#C9A227]" : "text-fog-300 hover:bg-ink-800",
              )}
            >
              <span className="w-4 text-center text-fog-500">{p.icon}</span>
              {p.label}
            </button>
          ))}
        </nav>
        <div className="border-t border-ink-700 px-5 py-4 text-[11px] leading-relaxed text-fog-500">
          {profile ? (
            <>
              <div className="truncate num">{profile.url}</div>
              <div>{profile.local ? "moteur local (processus détaché)" : "moteur distant"}</div>
              <div>Données : {s ? ago(s.now_ms) : "—"}</div>
            </>
          ) : (
            "Aucun moteur configuré"
          )}
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        {configured && <StatusBar s={s} error={sum.error} onKill={() => setKillOpen(true)} />}
        {configured && sum.error && page !== "connection" && (
          <div className="border-b border-down-500/30 bg-down-500/10 px-5 py-2 text-sm text-down-400">
            {unauthorized ? "Le moteur refuse le jeton : reconfigurez la connexion." : `Moteur injoignable — ${sum.error}. Les dernières données affichées peuvent être périmées.`}
          </div>
        )}
        <main className="min-h-0 flex-1 overflow-y-auto p-5">{content()}</main>
      </div>
      <Modal open={killOpen} title="Engager l'arrêt d'urgence ?" onClose={() => setKillOpen(false)}>
        <p className="mb-3 text-sm text-fog-300">
          Toutes les nouvelles entrées sont bloquées immédiatement. Selon la politique configurée, les ordres en attente d'AEGIS sont annulés et les positions conservées avec leurs stops (ou clôturées). La réinitialisation exige la phrase de confirmation.
        </p>
        <input className="mb-4 w-full" placeholder="Raison (facultatif)" value={killReason} onChange={(e) => setKillReason(e.target.value)} />
        <div className="flex justify-end gap-2">
          <Button kind="ghost" onClick={() => setKillOpen(false)}>Annuler</Button>
          <Button kind="danger" onClick={kill}>Engager l'arrêt</Button>
        </div>
      </Modal>
    </div>
  );
}
