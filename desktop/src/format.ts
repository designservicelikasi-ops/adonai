export const fmtMoney = (v: number | null | undefined, ccy = "", digits = 2) =>
  v === null || v === undefined || Number.isNaN(v) ? "—" : `${v >= 0 ? "" : "−"}${Math.abs(v).toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits })}${ccy ? " " + ccy : ""}`;
export const fmtSigned = (v: number | null | undefined, digits = 2) =>
  v === null || v === undefined || Number.isNaN(v) ? "—" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${Math.abs(v).toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;
export const fmtPct = (v: number | null | undefined, digits = 1) => (v === null || v === undefined || Number.isNaN(v) ? "—" : `${(v * 100).toFixed(digits)} %`);
export const fmtPctRaw = (v: number | null | undefined, digits = 2) => (v === null || v === undefined || Number.isNaN(v) ? "—" : `${v.toFixed(digits)} %`);
export const fmtNum = (v: number | null | undefined, digits = 2) => (v === null || v === undefined || Number.isNaN(v) ? "—" : v.toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits }));
export const fmtPrice = (v: number | null | undefined, digits = 5) => (v === null || v === undefined || !v ? "—" : v.toFixed(digits));
export const fmtTime = (ms: number | null | undefined) => (!ms ? "—" : new Date(ms).toLocaleTimeString("fr-FR", { hour12: false }));
export const fmtDateTime = (ms: number | null | undefined) =>
  !ms ? "—" : new Date(ms).toLocaleString("fr-FR", { hour12: false, day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
export const ago = (ms: number | null | undefined, now = Date.now()) => {
  if (!ms) return "—";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `il y a ${s} s`;
  if (s < 3600) return `il y a ${Math.round(s / 60)} min`;
  return `il y a ${Math.round(s / 3600)} h`;
};
export const tone = (v: number | null | undefined) => (v === null || v === undefined || v === 0 || Number.isNaN(v) ? "text-fog-300" : v > 0 ? "text-up-400" : "text-down-400");
