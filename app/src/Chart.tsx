import { useMemo } from "react";

interface Props {
  prices: number[];
  openPrice: number;
  entry?: number;
  side?: number;
  waves: { index: number; dir: number }[];
}

const W = 760;
const H = 300;
const PAD = 12;

/** Price line for the last ticks, drawn straight from onchain history. */
export function Chart({ prices, openPrice, entry, side, waves }: Props) {
  const view = useMemo(() => {
    const pts = prices.length ? prices : [openPrice];
    const lo = Math.min(...pts, openPrice, entry ?? Infinity);
    const hi = Math.max(...pts, openPrice, entry ?? -Infinity);
    const span = Math.max(hi - lo, openPrice * 0.01);
    const y = (p: number) => PAD + (H - 2 * PAD) * (1 - (p - (lo - span * 0.08)) / (span * 1.16));
    const x = (i: number) => PAD + ((W - 2 * PAD) * i) / Math.max(pts.length - 1, 1);
    const line = pts.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p).toFixed(1)}`).join("");
    const area = `${line}L${x(pts.length - 1).toFixed(1)},${H}L${x(0).toFixed(1)},${H}Z`;
    return { pts, x, y, line, area };
  }, [prices, openPrice, entry]);

  const last = view.pts[view.pts.length - 1];
  const up = last >= openPrice;

  return (
    <svg className="chart" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="Price chart">
      <defs>
        <linearGradient id="fill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor={up ? "var(--up)" : "var(--down)"} stopOpacity="0.28" />
          <stop offset="100%" stopColor={up ? "var(--up)" : "var(--down)"} stopOpacity="0" />
        </linearGradient>
      </defs>
      <line x1={0} x2={W} y1={view.y(openPrice)} y2={view.y(openPrice)} className="chart-open" />
      {entry ? (
        <g>
          <line x1={0} x2={W} y1={view.y(entry)} y2={view.y(entry)} className={side! > 0 ? "chart-entry long" : "chart-entry short"} />
          <text x={W - PAD} y={view.y(entry) - 6} textAnchor="end" className="chart-label">
            {side! > 0 ? "LONG" : "SHORT"} @ {entry.toFixed(2)}
          </text>
        </g>
      ) : null}
      <path d={view.area} fill="url(#fill)" />
      <path d={view.line} className={up ? "chart-line up" : "chart-line down"} />
      {waves.map((w) =>
        w.index >= 0 && w.index < view.pts.length ? (
          <circle
            key={`${w.index}-${w.dir}`}
            cx={view.x(w.index)}
            cy={view.y(view.pts[w.index])}
            r={6}
            className={w.dir > 0 ? "wave-dot up" : "wave-dot down"}
          />
        ) : null,
      )}
      <circle cx={view.x(view.pts.length - 1)} cy={view.y(last)} r={5} className="chart-head" />
    </svg>
  );
}
