import { useState } from "react";
import { Json } from "../api";
import { Badge, Bar, Panel, cx } from "../components/ui";
import { EmptyBox, Loaded, Note, PageHeader, useView } from "../components/kit";
import { fmtNum, fmtPct, fmtSigned, fmtTime, tone } from "../format";
import { RegimeBadge } from "./MarketIntel";

const VERDICT: Record<string, [string, "ok" | "bad" | "muted" | "warn"]> = { agrees: ["accord", "ok"], opposes: ["opposé", "bad"], neutral: ["neutre", "muted"], unavailable: ["non disponible", "warn"] };

function Opportunity({ o }: { o: Json }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={cx("rounded-xl border bg-white p-4 shadow-panel", o.admissible ? "border-aegis-500/60" : "border-ink-700")}>
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-base font-semibold">{o.symbol}</span>
        <Badge tone={o.side === "buy" ? "ok" : "bad"}>{String(o.side).toUpperCase()}</Badge>
        <span className="text-xs text-fog-400">{o.timeframe} · {fmtTime(o.ts_ms)}</span>
        <RegimeBadge r={o.regime} />
        <div className="flex-1" />
        <Badge tone={o.admissible ? "gold" : "muted"}>{o.admissible ? "opportunité admissible" : "refusée"}</Badge>
      </div>
      <div className="mt-3 grid gap-4 md:grid-cols-4">
        <div>
          <div className="text-[11px] uppercase tracking-wider text-fog-500">Score de configuration</div>
          <div className="num text-2xl font-semibold text-aegis-300">{o.score}/100</div>
          <div className="mt-1"><Bar value={o.score} max={100} /></div>
          <div className="mt-1 text-[10px] text-fog-500">ce n'est pas une probabilité</div>
        </div>
        <div>
          <div className="text-[11px] uppercase tracking-wider text-fog-500">P(TP avant SL)</div>
          <div className="num text-2xl font-semibold">{o.probability == null ? "—" : fmtPct(o.probability, 0)}</div>
          <div className="text-[11px] text-fog-400 line-clamp-2">{o.probability == null ? "aucun modèle calibré" : o.probability_source}</div>
        </div>
        <div>
          <div className="text-[11px] uppercase tracking-wider text-fog-500">Espérance nette</div>
          <div className={cx("num text-2xl font-semibold", tone(o.expected_net))}>{o.expected_net == null ? "—" : fmtSigned(o.expected_net)}</div>
          <div className="text-[11px] text-fog-400">coûts {o.costs == null ? "—" : fmtNum(o.costs)}</div>
        </div>
        <div>
          <div className="text-[11px] uppercase tracking-wider text-fog-500">Multi-unités</div>
          <div className="num text-2xl font-semibold">{fmtNum(o.alignment?.score, 0)}/100</div>
          <div className="text-[11px] text-fog-400">{(o.contradictions ?? []).length} contradiction(s)</div>
        </div>
      </div>
      {!o.admissible && (o.reasons ?? []).length > 0 && <ul className="mt-3 list-disc pl-5 text-xs text-down-400">{o.reasons.map((r: string, i: number) => <li key={i}>{r}</li>)}</ul>}
      <button className="mt-3 text-xs font-medium text-aegis-300 hover:underline" onClick={() => setOpen(!open)}>{open ? "Masquer le détail" : "Expliquer le score"}</button>
      {open && (
        <div className="mt-3 grid gap-4 md:grid-cols-2">
          <table>
            <thead><tr><th>Composante</th><th>Points</th><th>Détail</th></tr></thead>
            <tbody>{(o.components ?? []).map((c: Json) => <tr key={c.name}><td className="font-mono text-xs">{c.name}</td><td className="num">{fmtNum(c.points, 1)} / {c.max || "—"}</td><td className="text-xs text-fog-400">{c.detail}</td></tr>)}</tbody>
          </table>
          <table>
            <thead><tr><th>Unité</th><th>Rôle</th><th>Verdict</th></tr></thead>
            <tbody>
              {(o.alignment?.verdicts ?? []).map((v: Json) => {
                const [l, t] = VERDICT[v.verdict] ?? [v.verdict, "muted"];
                return <tr key={v.timeframe}><td>{v.timeframe}</td><td className="text-xs">{v.role}</td><td><Badge tone={t}>{l}</Badge> <span className="text-xs text-fog-400">{v.detail}</span></td></tr>;
              })}
            </tbody>
          </table>
          <div className="md:col-span-2 text-xs text-fog-400">Configurations : {(o.setups ?? []).map((s: Json) => s.detail).join(" · ")}</div>
        </div>
      )}
    </div>
  );
}

export default function Scanner() {
  const q = useView("scanner", 3000);
  const [only, setOnly] = useState(false);
  return (
    <div>
      <PageHeader
        title="Opportunity Scanner"
        subtitle="Recherche continue dans le moteur, à chaque bougie clôturée, sur tous les instruments. Le scanner n'envoie aucun ordre : il éclaire les décisions."
        right={<label className="flex items-center gap-2 text-sm text-fog-400"><input type="checkbox" className="accent-[#C6A15B]" checked={only} onChange={(e) => setOnly(e.target.checked)} /> admissibles seulement</label>}
      />
      <Note tone="gold">Le score (0–100) mesure la netteté d'une configuration. La probabilité vient uniquement d'un modèle calibré sur données indépendantes ; sans modèle, elle est absente et l'opportunité n'est pas admissible.</Note>
      <div className="mt-4 space-y-3">
        <Loaded q={q}>
          {(d) => {
            const list = (d.opportunities ?? []).filter((o: Json) => !only || o.admissible);
            return list.length === 0 ? <EmptyBox>Aucune opportunité {only ? "admissible " : ""}détectée.</EmptyBox> : list.slice(0, 40).map((o: Json) => <Opportunity key={o.id} o={o} />);
          }}
        </Loaded>
      </div>
      <Panel title="Comment le score est construit" className="mt-4">
        <p className="text-xs text-fog-400">Configuration (30) + alignement multi-unités (30) + adéquation au régime (20) + coût spread/ATR (10) + volatilité (10) − 10 par signal contraire. Définitions des configurations : docs/MARKET_INTELLIGENCE.md.</p>
      </Panel>
    </div>
  );
}
