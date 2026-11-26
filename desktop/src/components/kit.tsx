// Shared building blocks of the 0.3 pages: page header, data loading states, checks,
// confirmations. Every value shown comes from the engine; nothing is invented here.
import { ReactNode, useState } from "react";
import { api, ApiError, Json } from "../api";
import { Badge, Button, Modal, Panel, Tone, cx, usePoll, useToast } from "./ui";

export function PageHeader({ title, subtitle, right }: { title: string; subtitle?: ReactNode; right?: ReactNode }) {
  return (
    <div className="mb-5">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-fog-100">{title}</h1>
          {subtitle && <p className="mt-1 max-w-3xl text-sm text-fog-400">{subtitle}</p>}
        </div>
        {right && <div className="flex items-center gap-2">{right}</div>}
      </div>
      <div className="gold-rule mt-4" />
    </div>
  );
}

/** Poll one section of the engine state (`/api/v1/view/<section>`). */
export function useView(section: string, ms = 3000) {
  return usePoll(() => api.view(section), ms, [section]);
}

/** Loading / error / content of a polled section. */
export function Loaded({ q, children, empty }: { q: { data: Json; error: string | null }; children: (d: Json) => ReactNode; empty?: (d: Json) => boolean }) {
  if (q.error && q.data == null) return <ErrorBox text={q.error} />;
  if (q.data === null || q.data === undefined) return <Skeleton />;
  if (empty?.(q.data)) return <EmptyBox />;
  return <>{children(q.data)}</>;
}

export function Skeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="space-y-2" aria-busy="true" aria-label="chargement">
      {Array.from({ length: lines }).map((_, i) => (
        <div key={i} className="h-4 animate-pulse rounded bg-ink-750" style={{ width: `${90 - i * 15}%` }} />
      ))}
    </div>
  );
}

export function ErrorBox({ text }: { text: string }) {
  return <div className="rounded-lg border border-down-500/30 bg-down-500/5 px-4 py-3 text-sm text-down-400">Données indisponibles — {text}</div>;
}

export function EmptyBox({ children }: { children?: ReactNode }) {
  return <div className="rounded-lg border border-dashed border-ink-600 px-4 py-8 text-center text-sm text-fog-500">{children ?? "Aucune donnée pour l'instant."}</div>;
}

export function Note({ children, tone = "muted" }: { children: ReactNode; tone?: "muted" | "warn" | "gold" }) {
  const c = { muted: "border-ink-700 bg-ink-900 text-fog-400", warn: "border-warn-400/30 bg-warn-400/5 text-warn-400", gold: "border-aegis-500/40 bg-aegis-200/30 text-aegis-300" }[tone];
  return <p className={cx("rounded-lg border px-3 py-2 text-xs leading-relaxed", c)}>{children}</p>;
}

export function CheckList({ checks }: { checks: Json[] }) {
  if (!checks?.length) return <EmptyBox>Aucun contrôle.</EmptyBox>;
  return (
    <ul className="space-y-1">
      {checks.map((c: Json, i: number) => (
        <li key={i} className="flex gap-2 text-xs">
          <span className={cx("mt-0.5 h-2 w-2 shrink-0 rounded-full", c.passed ? "bg-up-500" : "bg-down-500")} aria-label={c.passed ? "réussi" : "échoué"} />
          <span className="font-mono text-fog-300">{c.name}</span>
          <span className="text-fog-400">{c.detail}</span>
        </li>
      ))}
    </ul>
  );
}

export function StatCard({ label, value, sub, tone, accent }: { label: ReactNode; value: ReactNode; sub?: ReactNode; tone?: string; accent?: boolean }) {
  return (
    <div className={cx("rounded-xl border bg-white p-4 shadow-panel", accent ? "border-aegis-500/50" : "border-ink-700")}>
      <div className="text-[11px] font-medium uppercase tracking-[.12em] text-fog-500">{label}</div>
      <div className={cx("num mt-1.5 truncate text-2xl font-semibold", tone ?? "text-fog-100")}>{value}</div>
      {sub && <div className="mt-1 truncate text-xs text-fog-400">{sub}</div>}
    </div>
  );
}

export function Grid({ children, cols = 4 }: { children: ReactNode; cols?: 2 | 3 | 4 | 5 }) {
  const c = { 2: "md:grid-cols-2", 3: "md:grid-cols-3", 4: "md:grid-cols-2 xl:grid-cols-4", 5: "md:grid-cols-3 xl:grid-cols-5" }[cols];
  return <div className={cx("grid gap-4", c)}>{children}</div>;
}

export function Tabs<T extends string>({ value, onChange, items }: { value: T; onChange: (v: T) => void; items: [T, string][] }) {
  return (
    <div className="mb-4 inline-flex rounded-lg border border-ink-700 bg-white p-0.5" role="tablist">
      {items.map(([k, label]) => (
        <button key={k} role="tab" aria-selected={value === k} onClick={() => onChange(k)} className={cx("rounded-md px-3 py-1.5 text-sm transition-colors", value === k ? "bg-aegis-200/70 text-aegis-300 font-medium" : "text-fog-400 hover:text-fog-100")}>
          {label}
        </button>
      ))}
    </div>
  );
}

/** Send a command and show the engine's answer (or refusal) as a toast. */
export function useCommand(after?: () => void) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const run = async (type: string, args?: Json) => {
    setBusy(true);
    try {
      toast("ok", await api.command(type, args));
      after?.();
      return true;
    } catch (e) {
      toast("bad", (e as ApiError).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { run, busy };
}

/** A sensitive operation: explained, then confirmed. */
export function ConfirmButton({ label, title, body, kind = "default", onConfirm, disabled, small }: { label: ReactNode; title: string; body: ReactNode; kind?: "default" | "primary" | "danger"; onConfirm: () => Promise<unknown> | void; disabled?: boolean; small?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button kind={kind} small={small} disabled={disabled} onClick={() => setOpen(true)}>{label}</Button>
      <Modal open={open} title={title} onClose={() => setOpen(false)}>
        <div className="mb-4 text-sm text-fog-300">{body}</div>
        <div className="flex justify-end gap-2">
          <Button kind="ghost" onClick={() => setOpen(false)}>Annuler</Button>
          <Button kind={kind === "default" ? "primary" : kind} onClick={async () => { await onConfirm(); setOpen(false); }}>Confirmer</Button>
        </div>
      </Modal>
    </>
  );
}

export function toneBadge(ok: boolean | null | undefined, yes = "oui", no = "non"): ReactNode {
  return <Badge tone={ok ? "ok" : ok === false ? "bad" : "muted"}>{ok ? yes : ok === false ? no : "—"}</Badge>;
}

export function Section({ title, right, children }: { title: string; right?: ReactNode; children: ReactNode }) {
  return <Panel title={title} right={right} className="mb-4">{children}</Panel>;
}

/** A stored candle history (symbol, timeframe, origin) chosen among what the engine holds. */
export interface Series { symbol: string; timeframe: string; origin: string; bars: number }

export function useStoredSeries(): Series[] {
  const q = useView("data", 15000);
  return ((q.data?.candles ?? []) as [string, string, string, number][]).map(([symbol, timeframe, origin, bars]) => ({ symbol, timeframe, origin, bars }));
}

export const seriesKey = (x: Series) => `${x.symbol}|${x.timeframe}|${x.origin}`;

export function SeriesSelect({ series, value, onChange, minBars = 0 }: { series: Series[]; value: string; onChange: (k: string) => void; minBars?: number }) {
  const usable = series.filter((x) => x.bars >= minBars);
  return (
    <select value={value} onChange={(e) => onChange(e.target.value)} aria-label="historique">
      {usable.length === 0 && <option value="">aucun historique stocké</option>}
      {usable.map((x) => (
        <option key={seriesKey(x)} value={seriesKey(x)}>
          {x.symbol} {x.timeframe} — {x.origin === "synthetic" ? "SYNTHÉTIQUE" : x.origin} ({x.bars} bougies)
        </option>
      ))}
    </select>
  );
}

export function parseSeriesKey(k: string) {
  const [symbol, timeframe, origin] = k.split("|");
  return { symbol, timeframe, origin };
}

export type { Tone };
