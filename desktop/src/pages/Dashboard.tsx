import { useMemo, useState } from "react";
import { api, Json } from "../api";
import { TimeChart } from "../components/Chart";
import { Badge, Bar, Empty, KV, Panel, Stat, cx, usePoll } from "../components/ui";
import { ago, fmtMoney, fmtNum, fmtPct, fmtPctRaw, fmtPrice, fmtSigned, fmtTime, tone } from "../format";

const regimeLabel: Record<string, string> = { trend_up: "Tendance haussière", trend_down: "Tendance baissière", range: "Range", high_volatility: "Volatilité élevée", unknown: "Indéterminé" };

function issueText(i: Json): string {
  if (typeof i === "string") return i === "NoData" ? "aucune donnée" : i;
  const [k, v] = Object.entries(i)[0] as [string, Json];
  if (k === "StalePrice") return `prix périmé (${v.age_ms} ms)`;
  if (k === "AbnormalSpread") return `spread anormal (${v.spread_points_x10 / 10} pts)`;
  if (k === "MissingBars") return `${v.count} bougies manquantes ${v.timeframe}`;
  if (k === "RejectedTicks") return `${v.count} ticks rejetés`;
  return k;
}

function GoalCard({ s }: { s: Json }) {
  const g = s.goal;
  const ccy = s.account?.currency ?? "";
  if (!g) return <Panel title="Objectif quotidien"><Empty>Comptabilité du jour indisponible (aucun compte connecté).</Empty></Panel>;
  const reached = g.reached;
  return (
    <Panel
      title="Objectif quotidien"
      right={<Badge tone={g.blocks_entries ? "warn" : reached ? "ok" : "gold"}>{!g.enabled ? "désactivé" : g.blocks_entries ? "entrées arrêtées" : reached ? "atteint" : "en cours"}</Badge>}
    >
      <div className="flex items-end justify-between">
        <Stat big label={`Réalisé (${g.basis === "gross" ? "BRUT" : "NET"})`} value={fmtMoney(g.achieved, ccy)} valueClass={tone(g.achieved)} />
        <div className="text-right">
          <div className="text-[11px] uppercase tracking-wider text-fog-500">Objectif {g.basis === "gross" ? "brut" : "net"}</div>
          <div className="num text-lg text-aegis-300">{fmtMoney(g.amount, ccy)}</div>
        </div>
      </div>
      <div className="mt-3"><Bar value={Math.max(0, g.achieved)} max={g.amount} tone={reached ? "up" : "gold"} /></div>
      <div className="mt-2 flex justify-between text-xs text-fog-400">
        <span className="num">{fmtNum(g.progress_pct, 1)} %</span>
        <span>Reste <span className="num text-fog-100">{fmtMoney(g.remaining, ccy)}</span></span>
      </div>
      <p className="mt-3 text-xs text-fog-500">{g.detail}. Un objectif n'est jamais une garantie : AEGIS préfère ne pas trader plutôt que forcer.</p>
    </Panel>
  );
}

function TodayCard({ s }: { s: Json }) {
  const d = s.day;
  const ccy = s.account?.currency ?? "";
  if (!d) return <Panel title="Aujourd'hui"><Empty>Aucune donnée de compte.</Empty></Panel>;
  const net = d.realized_gross + d.commissions + d.swaps + (d.fees ?? 0);
  const costs = -(d.commissions + d.swaps + (d.fees ?? 0)) + (d.spread_cost_est ?? 0) + Math.max(0, d.slippage_cost ?? 0);
  return (
    <Panel title="Aujourd'hui (UTC)" right={<span className="num">{d.closed_trades} clôturées · {d.entries_sent} entrées</span>}>
      <div className="grid grid-cols-2 gap-4">
        <Stat label="Profit brut" value={fmtSigned(d.gross_profit)} valueClass="text-up-400" />
        <Stat label="Perte brute" value={fmtSigned(d.gross_loss)} valueClass="text-down-400" />
        <Stat label="Résultat net réalisé" value={fmtSigned(net)} valueClass={tone(net)} />
        <Stat label="Coûts totaux" value={fmtMoney(costs, ccy)} valueClass="text-warn-400" />
      </div>
      <div className="mt-4 border-t border-ink-700 pt-3">
        <KV
          rows={[
            ["Commissions", fmtSigned(d.commissions)],
            ["Spread (estimé)", fmtSigned(-(d.spread_cost_est ?? 0))],
            ["Slippage (mesuré)", fmtSigned(-(d.slippage_cost ?? 0))],
            ["Swap", fmtSigned(d.swaps)],
            ["Autres frais", fmtSigned(d.fees ?? 0)],
            ["Pertes consécutives", d.consecutive_losses],
          ]}
        />
      </div>
    </Panel>
  );
}

function AccountCard({ s }: { s: Json }) {
  const a = s.account;
  if (!a) return <Panel title="Compte"><Empty>Aucun compte : le connecteur MT5 n'est pas connecté.</Empty></Panel>;
  return (
    <Panel title="Compte" right={<Badge tone={a.trade_mode === "real" ? "bad" : "info"}>{String(a.trade_mode).toUpperCase()}</Badge>}>
      <div className="grid grid-cols-2 gap-4">
        <Stat big label="Équité" value={fmtMoney(a.equity, a.currency)} />
        <Stat label="Solde" value={fmtMoney(a.balance, a.currency)} sub={`levier 1:${a.leverage}`} />
        <Stat label="Flottant" value={fmtSigned(s.floating_pnl)} valueClass={tone(s.floating_pnl)} />
        <Stat label="Résultat du jour" value={fmtSigned(s.daily_net)} sub={fmtPctRaw(s.daily_net_pct)} valueClass={tone(s.daily_net)} />
        <Stat label="Drawdown" value={fmtPctRaw(s.drawdown_pct)} sub={`max ${s.risk_config?.max_drawdown_pct} %`} valueClass={(s.drawdown_pct ?? 0) > (s.risk_config?.max_drawdown_pct ?? 100) / 2 ? "text-warn-400" : "text-fog-100"} />
        <Stat label="Perte du jour max" value={`${s.risk_config?.max_daily_loss_pct} %`} sub={s.daily_halt ?? "entrées autorisées"} />
      </div>
    </Panel>
  );
}

function MarketsPanel({ s, selected, onSelect }: { s: Json; selected: string | null; onSelect: (x: string) => void }) {
  const markets: Json[] = s.markets ?? [];
  const entries: Json[] = s.entry_states ?? [];
  if (!markets.length) return <Panel title="Marchés & décisions"><Empty>Aucun instrument : en attente du connecteur MT5.</Empty></Panel>;
  return (
    <Panel title="Marchés & décisions (Entry Intelligence)" pad={false}>
      <div className="overflow-x-auto">
        <table>
          <thead>
            <tr>
              <th>Instrument</th><th>Bid / Ask</th><th>Spread</th><th>Volatilité (ATR)</th><th>Régime</th><th>P(BUY)</th><th>P(SELL)</th><th>Décision</th><th>Raison</th>
            </tr>
          </thead>
          <tbody>
            {markets.map((m) => {
              const tf = (s.entry_states ?? []).find((e: Json) => e.symbol === m.symbol)?.timeframe;
              const ind = (m.indicators ?? []).find((i: Json) => i.timeframe === tf) ?? m.indicators?.[0];
              const e = entries.find((x) => x.symbol === m.symbol);
              const digits = (s.specs ?? []).find((x: Json) => x.symbol === m.symbol)?.digits ?? 5;
              const dTone = e?.decision === "BUY" ? "ok" : e?.decision === "SELL" ? "bad" : "muted";
              return (
                <tr key={m.symbol} onClick={() => onSelect(m.symbol)} className={cx("cursor-pointer", selected === m.symbol && "bg-ink-800")}>
                  <td className="font-medium">
                    {m.symbol}
                    {(m.issues ?? []).length > 0 && <div className="text-[11px] text-warn-400">{m.issues.map(issueText).join(" · ")}</div>}
                  </td>
                  <td className="num">{m.tick ? `${fmtPrice(m.tick.bid, digits)} / ${fmtPrice(m.tick.ask, digits)}` : "—"}<div className="text-[11px] text-fog-500">{ago(m.received_ms, s.now_ms)}</div></td>
                  <td className="num">{m.spread_points?.toFixed(1) ?? "—"} pts</td>
                  <td className="num">{ind?.atr ? ind.atr.toPrecision(3) : "—"}<div className="text-[11px] text-fog-500">{ind ? `${ind.timeframe} · vol ${fmtNum(ind.regime?.volatility_ratio, 2)}` : ""}</div></td>
                  <td>{ind ? regimeLabel[ind.regime?.regime] ?? ind.regime?.regime : "—"}</td>
                  <td className="num text-up-400">{fmtPct(e?.buy_probability)}</td>
                  <td className="num text-down-400">{fmtPct(e?.sell_probability)}</td>
                  <td><Badge tone={dTone}>{e?.decision ?? "WAIT"}</Badge></td>
                  <td className="max-w-[340px] text-xs text-fog-400">{e?.reason ?? "—"}<div className="text-[11px] text-fog-500">{e?.probability_source}</div></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function PositionsPanel({ s }: { s: Json }) {
  const pos: Json[] = s.positions ?? [];
  const dec: Json[] = s.position_decisions ?? [];
  return (
    <Panel title="Positions ouvertes (Position Intelligence)" right={<span className="num">{pos.length}</span>} pad={false}>
      {pos.length === 0 ? (
        <Empty>Aucune position ouverte.</Empty>
      ) : (
        <table>
          <thead><tr><th>Ticket</th><th>Sens</th><th>Volume</th><th>Entrée</th><th>SL / TP</th><th>Flottant</th><th>Action</th></tr></thead>
          <tbody>
            {pos.map((p) => {
              const d = dec.find((x) => x.ticket === p.ticket);
              const foreign = (s.foreign_tickets ?? []).includes(p.ticket);
              return (
                <tr key={p.ticket}>
                  <td className="num">{p.ticket}<div className="text-[11px] text-fog-500">{p.symbol}{foreign ? " · hors AEGIS" : ""}</div></td>
                  <td><Badge tone={p.side === "buy" ? "ok" : "bad"}>{p.side.toUpperCase()}</Badge></td>
                  <td className="num">{p.volume}</td>
                  <td className="num">{p.price_open}</td>
                  <td className="num text-xs">{p.sl || "—"}<br />{p.tp || "—"}</td>
                  <td className={cx("num", tone(p.profit + p.swap + p.commission))}>{fmtSigned(p.profit + p.swap + p.commission)}</td>
                  <td className="text-xs">
                    {d ? (
                      <>
                        <Badge tone={d.action.kind === "hold" ? "muted" : d.action.kind === "close" ? "bad" : "gold"}>{String(d.action.kind).replace("_", " ").toUpperCase()}</Badge>
                        <div className="mt-1 max-w-[260px] text-fog-400">{d.action.reason}{d.note ? ` (${d.note})` : ""}</div>
                      </>
                    ) : (
                      "—"
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </Panel>
  );
}

export default function Dashboard({ s }: { s: Json | null; refresh: () => void }) {
  const [sel, setSel] = useState<string | null>(null);
  const charts = usePoll(() => api.view("charts"), 5000);
  const symbol = sel ?? s?.markets?.[0]?.symbol ?? null;
  const points = useMemo(() => {
    const c = (charts.data ?? []).find((x: Json) => x[0] === symbol);
    return c ? c[2].map((p: [number, number]) => ({ t: p[0], v: p[1] })) : [];
  }, [charts.data, symbol]);
  if (!s) return <Empty>Chargement de l'état du moteur…</Empty>;
  const strategies: Json[] = s.strategies ?? [];
  const news = s.news;
  const shadow = s.shadow;
  return (
    <div className="space-y-5">
      <div className="grid gap-5 xl:grid-cols-3">
        <GoalCard s={s} />
        <TodayCard s={s} />
        <AccountCard s={s} />
      </div>
      <MarketsPanel s={s} selected={symbol} onSelect={setSel} />
      <div className="grid gap-5 xl:grid-cols-5">
        <Panel title={`Prix ${symbol ?? ""} (clôtures, unité de signal)`} className="xl:col-span-3">
          {points.length ? <TimeChart points={points} /> : <Empty>Pas encore d'historique de prix.</Empty>}
        </Panel>
        <div className="xl:col-span-2"><PositionsPanel s={s} /></div>
      </div>
      <div className="grid gap-5 xl:grid-cols-3">
        <Panel title="Stratégies" right={<span>{strategies.filter((x) => x.enabled && x.stage !== "disabled").length} actives</span>}>
          <div className="space-y-1.5 text-sm">
            {strategies.map((x) => (
              <div key={x.id} className="flex items-center justify-between">
                <span className={x.enabled ? "" : "text-fog-500 line-through"}>{x.name}</span>
                <span className="flex items-center gap-2">
                  <span className="num text-xs text-fog-500">{x.signals} sig.</span>
                  <Badge tone={x.stage === "live_approved" ? "bad" : x.stage === "demo" ? "info" : x.stage === "disabled" ? "muted" : "gold"}>{x.stage}</Badge>
                </span>
              </div>
            ))}
          </div>
        </Panel>
        <Panel title="Actualités" right={news && <Badge tone={news.feed_ok ? "ok" : news.enabled ? "warn" : "muted"}>{!news.enabled ? "désactivé" : news.feed_ok ? "calendrier à jour" : "calendrier indisponible"}</Badge>}>
          {(news?.by_symbol ?? []).map(([sym, a]: [string, Json]) => (
            <div key={sym} className="mb-2 flex items-start justify-between gap-3 text-sm">
              <span>{sym}</span>
              <span className="text-right">
                <Badge tone={a.recommendation === "allowed" ? "ok" : a.recommendation === "caution" ? "warn" : "bad"}>{a.recommendation === "allowed" ? "autorisé" : a.recommendation === "caution" ? "prudent" : "suspendu"}</Badge>
                <div className="mt-1 max-w-[260px] text-[11px] text-fog-500">{a.reasons?.[0]}</div>
              </span>
            </div>
          ))}
          <div className="mt-3 space-y-1 border-t border-ink-700 pt-3">
            {(news?.upcoming ?? []).slice(0, 5).map((e: Json) => (
              <div key={e.id} className="flex justify-between gap-3 text-xs">
                <span className="num text-fog-400">{fmtTime(e.ts_ms)}</span>
                <span className="flex-1 truncate">{e.currency} · {e.title}</span>
                <Badge tone={e.impact === "high" ? "bad" : "warn"}>{e.impact}</Badge>
              </div>
            ))}
            {!(news?.upcoming ?? []).length && <div className="text-xs text-fog-500">Aucune annonce moyenne ou forte dans les 48 h.</div>}
          </div>
        </Panel>
        <Panel title="Shadow Intelligence" right={shadow ? <Badge tone="info" dot>indépendant</Badge> : <Badge>désactivé</Badge>}>
          {shadow ? (
            <KV
              rows={[
                ["Bougies observées", shadow.bars_processed],
                ["Décisions suivies", shadow.decisions_seen],
                ["Candidats (champions + challengers)", shadow.candidates],
                ["Challengers prêts pour la démo", shadow.ready],
                ["Mise à jour", ago(shadow.updated_ms, s.now_ms)],
              ]}
            />
          ) : (
            <Empty>Le moteur Shadow est désactivé dans la configuration.</Empty>
          )}
          {s.shadow_dropped > 0 && <p className="mt-2 text-xs text-warn-400">{s.shadow_dropped} copies perdues (file pleine).</p>}
        </Panel>
      </div>
    </div>
  );
}
