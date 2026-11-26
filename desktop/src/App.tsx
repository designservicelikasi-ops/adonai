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
import Brain from "./pages/Brain";
import News from "./pages/News";
import Settings from "./pages/Settings";
import System from "./pages/System";
import CommandCenter from "./pages/CommandCenter";
import MarketIntel from "./pages/MarketIntel";
import Scanner from "./pages/Scanner";
import Micro from "./pages/Micro";
import PositionIntel from "./pages/PositionIntel";
import Recovery from "./pages/Recovery";
import Autonomy from "./pages/Autonomy";
import RiskGuardian from "./pages/RiskGuardian";
import Learning from "./pages/Learning";
import Backtesting from "./pages/Backtesting";
import Portfolio from "./pages/Portfolio";
import Mt5 from "./pages/Mt5";
import Security from "./pages/Security";
import Notifications from "./pages/Notifications";
import Audit from "./pages/Audit";

type PageProps = { s: Json; refresh: () => void };
interface PageDef { id: string; label: string; icon: string; group: string; keywords?: string; render?: (p: PageProps) => JSX.Element }

const GROUPS = ["Pilotage", "Intelligence", "Recherche", "Contrôle", "Système"] as const;

// Every page reads the engine through the API; none invents a value.
const PAGES: PageDef[] = [
  { id: "command", label: "Command Center", icon: "◆", group: "Pilotage", keywords: "accueil état synthèse", render: (p) => <CommandCenter s={p.s} /> },
  { id: "dashboard", label: "Tableau de bord", icon: "◈", group: "Pilotage", keywords: "graphique marché compte", render: (p) => <Dashboard {...p} /> },
  { id: "trading", label: "Positions & ordres", icon: "⇅", group: "Pilotage", keywords: "trading clôturer modifier", render: (p) => <Trading {...p} /> },
  { id: "positions", label: "Position Intelligence", icon: "⌖", group: "Pilotage", keywords: "hold secure stop suiveur", render: (p) => <PositionIntel {...p} /> },
  { id: "micro", label: "Micro-Profit", icon: "◦", group: "Pilotage", keywords: "objectifs nets frais", render: (p) => <Micro s={p.s} /> },
  { id: "performance", label: "Performance", icon: "↗", group: "Pilotage", keywords: "résultats statistiques", render: (p) => <Performance s={p.s} /> },
  { id: "market", label: "Market Intelligence", icon: "◐", group: "Intelligence", keywords: "régimes multi-unités structure", render: () => <MarketIntel /> },
  { id: "scanner", label: "Opportunity Scanner", icon: "⌕", group: "Intelligence", keywords: "opportunités score", render: () => <Scanner /> },
  { id: "decisions", label: "Décisions", icon: "⎇", group: "Intelligence", keywords: "signaux refus raisons", render: (p) => <Decisions s={p.s} /> },
  { id: "brain", label: "Cerveau IA", icon: "✦", group: "Intelligence", keywords: "micro-profits apprentissage", render: (p) => <Brain s={p.s} /> },
  { id: "intelligence", label: "Probabilités & Shadow", icon: "◎", group: "Intelligence", keywords: "modèles calibration champion challenger", render: (p) => <Intelligence s={p.s} /> },
  { id: "news", label: "Actualités", icon: "☰", group: "Intelligence", keywords: "calendrier annonces", render: (p) => <News s={p.s} /> },
  { id: "strategies", label: "Stratégies", icon: "⚙", group: "Recherche", keywords: "paramètres promotion", render: (p) => <Strategies {...p} /> },
  { id: "backtesting", label: "Backtesting", icon: "⟲", group: "Recherche", keywords: "historique walk-forward", render: (p) => <Backtesting {...p} /> },
  { id: "learning", label: "Fast Learning", icon: "↯", group: "Recherche", keywords: "recherche paramètres dérive audit données", render: (p) => <Learning {...p} /> },
  { id: "autonomy", label: "Autonomy Commander", icon: "⚑", group: "Contrôle", keywords: "autorisation profil permissions", render: (p) => <Autonomy {...p} /> },
  { id: "guardian", label: "Risk Guardian", icon: "⛨", group: "Contrôle", keywords: "limites risque arrêt d'urgence", render: (p) => <RiskGuardian {...p} /> },
  { id: "recovery", label: "Recovery & Hedge", icon: "⤾", group: "Contrôle", keywords: "re-entry couverture", render: (p) => <Recovery s={p.s} /> },
  { id: "portfolio", label: "Portefeuille", icon: "▦", group: "Contrôle", keywords: "multi-actifs corrélations exposition devises", render: (p) => <Portfolio {...p} /> },
  { id: "settings", label: "Risque & configuration", icon: "⚙", group: "Contrôle", keywords: "modes réglages", render: (p) => <Settings {...p} /> },
  { id: "mt5", label: "Terminal MT5", icon: "⇄", group: "Système", keywords: "connecteur latence exécution bridge", render: (p) => <Mt5 s={p.s} /> },
  { id: "security", label: "Sécurité", icon: "⚿", group: "Système", keywords: "clés certificat refus", render: (p) => <Security {...p} /> },
  { id: "notifications", label: "Notifications", icon: "◔", group: "Système", keywords: "alertes avertissements", render: (p) => <Notifications {...p} /> },
  { id: "audit", label: "Audit", icon: "≣", group: "Système", keywords: "journal traçabilité ordres commandes", render: (p) => <Audit {...p} /> },
  { id: "system", label: "Système", icon: "⌁", group: "Système", keywords: "santé journaux sauvegarde", render: (p) => <System {...p} /> },
  { id: "connection", label: "Connexion", icon: "⇆", group: "Système", keywords: "moteur appairage jeton" },
];
type PageId = string;

export function modeTone(mode?: string): Tone {
  return mode === "live_auto" ? "bad" : mode === "demo_auto" ? "info" : "muted";
}
export function modeLabel(mode?: string) {
  return mode === "live_auto" ? "LIVE AUTO" : mode === "demo_auto" ? "DEMO AUTO" : "MONITOR";
}

const PROFILE_TONE: Record<string, Tone> = { observer: "muted", research_autonomy: "info", demo_full_autonomy: "gold", live_authorized_autonomy: "bad" };

function StatusBar({ s, error, autonomy, onKill, onSearch, onAutonomy }: { s: Json | null; error: string | null; autonomy: Json | null; onKill: () => void; onSearch: () => void; onAutonomy: () => void }) {
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
        <span className="hidden text-xs text-fog-400 num lg:inline">
          {conn.server} · #{conn.login} · {String(conn.trade_mode).toUpperCase()} · dernière trame {latency} ms
        </span>
      )}
      <div className="flex-1" />
      <button onClick={onSearch} className="hidden items-center gap-2 rounded-md border border-ink-700 bg-white px-2.5 py-1 text-xs text-fog-400 hover:border-aegis-500/60 md:flex" title="Aller à une page">
        Rechercher <kbd className="rounded border border-ink-600 px-1 font-mono text-[10px]">{navigator.platform.includes("Mac") ? "⌘" : "Ctrl"} K</kbd>
      </button>
      {s?.kill?.active && <Badge tone="bad" dot>ARRÊT D'URGENCE</Badge>}
      {s?.daily_halt && !s?.kill?.active && <Badge tone="warn">Entrées arrêtées</Badge>}
      {autonomy && (
        <button onClick={onAutonomy} title={autonomy.active_grant?.expires_ms ? `autorisation jusqu'au ${new Date(autonomy.active_grant.expires_ms).toLocaleString("fr-FR")}` : "profil d'autonomie"}>
          <Badge tone={PROFILE_TONE[autonomy.profile] ?? "muted"}>{autonomy.profile_label}</Badge>
        </button>
      )}
      <Badge tone={modeTone(s?.mode)}>{modeLabel(s?.mode)}</Badge>
      <Button kind="danger" small onClick={onKill} disabled={!s || s?.kill?.active} title="Bloque toute nouvelle entrée immédiatement">
        ■ Arrêt d'urgence
      </Button>
    </div>
  );
}

const fold = (x: string) => x.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

/** Ctrl/Cmd+K: jump to any page by name or keyword. */
function Palette({ open, onClose, onPick, enabled }: { open: boolean; onClose: () => void; onPick: (id: PageId) => void; enabled: (id: PageId) => boolean }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  useEffect(() => { if (open) { setQ(""); setSel(0); } }, [open]);
  const items = PAGES.filter((p) => enabled(p.id) && (!q || fold(`${p.label} ${p.group} ${p.keywords ?? ""}`).includes(fold(q))));
  useEffect(() => setSel(0), [q]);
  if (!open) return null;
  const pick = (p?: PageDef) => { if (p) { onPick(p.id); onClose(); } };
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-[#18202B]/30 pt-[12vh] backdrop-blur-sm" onClick={onClose} role="dialog" aria-modal="true" aria-label="Aller à une page">
      <div className="w-[560px] max-w-[92vw] overflow-hidden rounded-xl border border-ink-600 bg-white shadow-panel animate-rise" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          className="w-full rounded-none border-0 border-b border-ink-700 px-4 py-3 text-base focus:shadow-none"
          placeholder="Aller à… (page, fonction, mot-clé)"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") { e.preventDefault(); setSel((i) => Math.min(items.length - 1, i + 1)); }
            else if (e.key === "ArrowUp") { e.preventDefault(); setSel((i) => Math.max(0, i - 1)); }
            else if (e.key === "Enter") pick(items[sel]);
            else if (e.key === "Escape") onClose();
          }}
        />
        <ul className="max-h-[50vh] overflow-y-auto py-1" role="listbox">
          {items.length === 0 && <li className="px-4 py-6 text-center text-sm text-fog-500">Aucune page ne correspond.</li>}
          {items.map((p, i) => (
            <li key={p.id} role="option" aria-selected={i === sel}>
              <button onMouseEnter={() => setSel(i)} onClick={() => pick(p)} className={cx("flex w-full items-center gap-3 px-4 py-2 text-left text-sm", i === sel ? "bg-aegis-200/50 text-aegis-300" : "text-fog-300")}>
                <span className="w-4 text-center text-fog-500">{p.icon}</span>
                <span className="flex-1">{p.label}</span>
                <span className="text-[11px] uppercase tracking-wider text-fog-500">{p.group}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

export default function App() {
  const toast = useToast();
  // Deep link (#page=performance) — used by the visual-QA preview.
  const initial = (new URLSearchParams(window.location.hash.slice(1)).get("page") ?? "command") as PageId;
  const [page, setPage] = useState<PageId>(PAGES.some((p) => p.id === initial) ? initial : "command");
  const [profile, setProfile] = useState<ProfileView | null | undefined>(undefined);
  const [killOpen, setKillOpen] = useState(false);
  const [killReason, setKillReason] = useState("");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const configured = !!profile;
  const sum = usePoll(() => (configured ? api.summary() : Promise.resolve(null)), 1000, [configured]);
  const autonomy = usePoll(() => (configured ? api.view("autonomy") : Promise.resolve(null)), 4000, [configured]);

  useEffect(() => {
    api.profileLoad()
      .then((p) => {
        setProfile(p);
        if (!p) setPage("connection");
        else if (initial === "connection") setPage("connection");
      })
      .catch(() => setProfile(null));
  }, []);

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  // Keep the current page visible in the (scrollable) sidebar.
  useEffect(() => { document.querySelector('nav [aria-current="page"]')?.scrollIntoView({ block: "nearest" }); }, [page]);

  const s = sum.data;
  const unauthorized = sum.error && sum.error.startsWith("UNAUTHORIZED");
  const enabled = (id: PageId) => configured || id === "connection";

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
            setPage("command");
            sum.refresh();
          }}
          onForget={() => setProfile(null)}
        />
      );
    const def = PAGES.find((p) => p.id === page);
    return def?.render?.({ s, refresh: sum.refresh }) ?? null;
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
        <nav className="min-h-0 flex-1 overflow-y-auto px-3 pb-3" aria-label="pages">
          {GROUPS.map((g) => (
            <div key={g} className="mb-3">
              <div className="px-3 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-[.16em] text-fog-500">{g}</div>
              {PAGES.filter((p) => p.group === g).map((p) => (
                <button
                  key={p.id}
                  disabled={!enabled(p.id)}
                  onClick={() => setPage(p.id)}
                  aria-current={page === p.id ? "page" : undefined}
                  className={cx(
                    "flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left text-sm transition-colors disabled:opacity-30",
                    page === p.id ? "bg-aegis-500/12 text-aegis-300 shadow-[inset_2px_0_0_#C6A15B]" : "text-fog-300 hover:bg-ink-800",
                  )}
                >
                  <span className="w-4 text-center text-fog-500">{p.icon}</span>
                  {p.label}
                </button>
              ))}
            </div>
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
        {configured && <StatusBar s={s} error={sum.error} autonomy={autonomy.data} onKill={() => setKillOpen(true)} onSearch={() => setPaletteOpen(true)} onAutonomy={() => setPage("autonomy")} />}
        {configured && sum.error && page !== "connection" && (
          <div className="border-b border-down-500/30 bg-down-500/10 px-5 py-2 text-sm text-down-400">
            {unauthorized ? "Le moteur refuse le jeton : reconfigurez la connexion." : `Moteur injoignable — ${sum.error}. Les dernières données affichées peuvent être périmées.`}
          </div>
        )}
        <main className="min-h-0 flex-1 overflow-y-auto p-5">{content()}</main>
      </div>
      <Palette open={paletteOpen} onClose={() => setPaletteOpen(false)} onPick={setPage} enabled={enabled} />
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
