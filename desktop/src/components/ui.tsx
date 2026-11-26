import { createContext, ReactNode, useCallback, useContext, useEffect, useRef, useState } from "react";

export function cx(...c: (string | false | null | undefined)[]) {
  return c.filter(Boolean).join(" ");
}

export function Panel({ title, right, children, className, pad = true }: { title?: ReactNode; right?: ReactNode; children: ReactNode; className?: string; pad?: boolean }) {
  return (
    <section className={cx("rounded-xl border border-ink-700 bg-ink-850 shadow-panel animate-rise", className)}>
      {(title || right) && (
        <header className="flex items-center justify-between gap-3 px-4 py-2.5 border-b border-ink-700">
          <h2 className="text-[11px] font-semibold uppercase tracking-[.14em] text-fog-400">{title}</h2>
          <div className="flex items-center gap-2 text-xs text-fog-400">{right}</div>
        </header>
      )}
      <div className={pad ? "p-4" : ""}>{children}</div>
    </section>
  );
}

export function Stat({ label, value, sub, valueClass, big }: { label: ReactNode; value: ReactNode; sub?: ReactNode; valueClass?: string; big?: boolean }) {
  return (
    <div className="min-w-0">
      <div className="text-[11px] uppercase tracking-wider text-fog-500">{label}</div>
      <div className={cx("num truncate", big ? "text-2xl font-semibold mt-1" : "text-base mt-0.5", valueClass ?? "text-fog-100")}>{value}</div>
      {sub && <div className="text-xs text-fog-500 mt-0.5 truncate">{sub}</div>}
    </div>
  );
}

const badgeTones = {
  ok: "bg-up-500/12 text-up-400 border-up-500/30",
  bad: "bg-down-500/12 text-down-400 border-down-500/30",
  warn: "bg-warn-400/10 text-warn-400 border-warn-400/30",
  info: "bg-info-400/10 text-info-400 border-info-400/30",
  gold: "bg-aegis-200/60 text-aegis-300 border-aegis-500/50",
  muted: "bg-ink-750 text-fog-400 border-ink-700",
} as const;
export type Tone = keyof typeof badgeTones;

export function Badge({ tone = "muted", children, dot }: { tone?: Tone; children: ReactNode; dot?: boolean }) {
  return (
    <span className={cx("inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] font-medium whitespace-nowrap", badgeTones[tone])}>
      {dot && <span className="h-1.5 w-1.5 rounded-full bg-current animate-pulseDot" />}
      {children}
    </span>
  );
}

export function Button({ children, onClick, kind = "default", disabled, small, title, type }: { children: ReactNode; onClick?: () => void; kind?: "default" | "primary" | "danger" | "ghost"; disabled?: boolean; small?: boolean; title?: string; type?: "button" | "submit" }) {
  const k = {
    default: "border-ink-600 bg-ink-750 hover:bg-ink-700 text-fog-100",
    primary: "border-aegis-600 bg-gradient-to-b from-aegis-500 to-aegis-600 hover:from-aegis-400 hover:to-aegis-600 text-white shadow-gold",
    danger: "border-down-500/60 bg-down-500/15 hover:bg-down-500/25 text-down-400",
    ghost: "border-transparent hover:bg-ink-750 text-fog-300",
  }[kind];
  return (
    <button type={type ?? "button"} title={title} disabled={disabled} onClick={onClick} className={cx("rounded-md border font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed", small ? "px-2 py-1 text-xs" : "px-3 py-1.5 text-sm", k)}>
      {children}
    </button>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="py-8 text-center text-sm text-fog-500">{children}</div>;
}

export function Bar({ value, max = 1, tone = "gold" }: { value: number; max?: number; tone?: "gold" | "up" | "down" }) {
  const pct = Math.max(0, Math.min(100, (value / (max || 1)) * 100));
  const c = { gold: "from-aegis-600 via-aegis-500 to-aegis-metal", up: "from-up-500 to-up-400", down: "from-down-500 to-down-400" }[tone];
  return (
    <div className="h-2 w-full rounded-full bg-ink-750 overflow-hidden" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
      <div className={cx("h-full rounded-full bg-gradient-to-r transition-[width] duration-500", c)} style={{ width: `${pct}%` }} />
    </div>
  );
}

export function KV({ rows }: { rows: [ReactNode, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[auto,1fr] gap-x-6 gap-y-1.5 text-sm">
      {rows.map(([k, v], i) => (
        <div key={i} className="contents">
          <dt className="text-fog-500">{k}</dt>
          <dd className="num text-fog-100 text-right truncate">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

// ---------------------------------------------------------------- toasts

interface Toast { id: number; tone: "ok" | "bad" | "info"; text: string }
const ToastCtx = createContext<(tone: Toast["tone"], text: string) => void>(() => undefined);
export const useToast = () => useContext(ToastCtx);

export function ToastHost({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Toast[]>([]);
  const seq = useRef(0);
  const push = useCallback((tone: Toast["tone"], text: string) => {
    const id = ++seq.current;
    setItems((x) => [...x.slice(-4), { id, tone, text }]);
    setTimeout(() => setItems((x) => x.filter((t) => t.id !== id)), tone === "bad" ? 9000 : 5000);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="fixed bottom-5 right-5 z-50 flex w-[420px] flex-col gap-2">
        {items.map((t) => (
          <div key={t.id} className={cx("rounded-lg border px-4 py-3 text-sm shadow-panel animate-rise backdrop-blur", t.tone === "ok" ? "border-up-500/40 bg-white text-up-400" : t.tone === "bad" ? "border-down-500/40 bg-white text-down-400" : "border-info-400/40 bg-white text-info-400")}>
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

// ---------------------------------------------------------------- modal

export function Modal({ open, title, children, onClose }: { open: boolean; title: ReactNode; children: ReactNode; onClose: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-[#18202B]/30 backdrop-blur-sm" onClick={onClose} role="dialog" aria-modal="true">
      <div className="w-[520px] max-w-[92vw] rounded-xl border border-ink-600 bg-ink-850 p-5 shadow-panel animate-rise" onClick={(e) => e.stopPropagation()}>
        <h3 className="mb-3 text-base font-semibold">{title}</h3>
        {children}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- polling

/** Poll an async source; keeps the last good value and reports errors. */
export function usePoll<T>(fn: () => Promise<T>, ms: number, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const run = async () => {
      try {
        const v = await fn();
        if (alive) {
          setData(v);
          setError(null);
        }
      } catch (e) {
        if (alive) setError(String((e as Error).message ?? e));
      }
      if (alive) timer = setTimeout(run, ms);
    };
    run();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms, tick, ...deps]);
  return { data, error, refresh: () => setTick((t) => t + 1) };
}
