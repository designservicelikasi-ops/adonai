import { createChart, ColorType, IChartApi, ISeriesApi, LineStyle, UTCTimestamp } from "lightweight-charts";
import { useEffect, useRef } from "react";

export interface Point { t: number; v: number }

/** Line/area chart of real engine data. Times are epoch milliseconds. */
export function TimeChart({ points, height = 260, kind = "area", color = "#C6A15B", baseline, priceDigits = 5 }: { points: Point[]; height?: number; kind?: "area" | "line"; color?: string; baseline?: number; priceDigits?: number }) {
  const box = useRef<HTMLDivElement>(null);
  const chart = useRef<IChartApi | null>(null);
  const series = useRef<ISeriesApi<"Area"> | ISeriesApi<"Line"> | null>(null);

  useEffect(() => {
    if (!box.current) return;
    const c = createChart(box.current, {
      height,
      layout: { background: { type: ColorType.Solid, color: "transparent" }, textColor: "#697386", fontFamily: "JetBrains Mono, SF Mono, Consolas, monospace", fontSize: 11 },
      grid: { vertLines: { color: "rgba(232,227,216,.8)" }, horzLines: { color: "rgba(232,227,216,.8)" } },
      rightPriceScale: { borderColor: "#E8E3D8" },
      timeScale: { borderColor: "#E8E3D8", timeVisible: true, secondsVisible: false },
      crosshair: { vertLine: { color: "#9A7839", style: LineStyle.Dashed }, horzLine: { color: "#9A7839", style: LineStyle.Dashed } },
      autoSize: true,
    });
    const fmt = { type: "price" as const, precision: priceDigits, minMove: 1 / 10 ** priceDigits };
    series.current =
      kind === "area"
        ? c.addAreaSeries({ lineColor: color, topColor: color + "55", bottomColor: color + "00", lineWidth: 2, priceFormat: fmt })
        : c.addLineSeries({ color, lineWidth: 2, priceFormat: fmt });
    if (baseline !== undefined) series.current.createPriceLine({ price: baseline, color: "#BFB39A", lineWidth: 1, lineStyle: LineStyle.Dotted, axisLabelVisible: false, title: "" });
    chart.current = c;
    return () => {
      c.remove();
      chart.current = null;
      series.current = null;
    };
  }, [height, kind, color, baseline, priceDigits]);

  useEffect(() => {
    if (!series.current) return;
    // Strictly increasing seconds, as the library requires.
    const data: { time: UTCTimestamp; value: number }[] = [];
    let last = -Infinity;
    for (const p of points) {
      const s = Math.floor(p.t / 1000);
      if (s > last && Number.isFinite(p.v)) {
        data.push({ time: s as UTCTimestamp, value: p.v });
        last = s;
      } else if (s === last && data.length) data[data.length - 1].value = p.v;
    }
    series.current.setData(data);
    chart.current?.timeScale().fitContent();
  }, [points]);

  return <div ref={box} style={{ height }} className="w-full" />;
}
