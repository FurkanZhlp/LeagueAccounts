// LP tracking charts: the card sparkline and the full history chart.
//
// Ranks are plotted on one continuous "score" axis (see history.rs): 400 per
// tier, 100 per division below Master, then shared Master+ LP. Tier bands
// behind the line make the axis readable without converting numbers.

import type { HistoryPoint, TrendPoint } from "./api";
import { esc } from "./dom";
import { lang, t, tierName } from "./i18n";
import { emblem, palette } from "./ranks";

const LADDER = ["Iron", "Bronze", "Silver", "Gold", "Platinum", "Emerald", "Diamond"];
const DIVISIONS = ["IV", "III", "II", "I"];
const APEX_BASE = 2800;
const DAY = 24 * 60 * 60 * 1000;

/** Human label for a score, e.g. "Gold II · 45 LP" or "Master+ · 312 LP". */
export function scoreLabel(score: number): string {
  if (score >= APEX_BASE) return `${tierName("Master")}+ · ${score - APEX_BASE} LP`;
  const tier = LADDER[Math.max(0, Math.floor(score / 400))];
  const division = DIVISIONS[Math.floor((score % 400) / 100)];
  return `${tierName(tier)} ${division} · ${score % 100} LP`;
}

function tierOfScore(score: number): string {
  return score >= APEX_BASE ? "Master" : LADDER[Math.max(0, Math.floor(score / 400))];
}

/** Score change over the last 24 h (or since the first point), if any. */
export function recentDelta(points: TrendPoint[]): number | null {
  if (points.length < 2) return null;
  const latest = points[points.length - 1];
  const cutoff = latest[0] - DAY;
  let base = points[0];
  for (const point of points) {
    if (point[0] <= cutoff) base = point;
  }
  if (base === latest) base = points[points.length - 2];
  return latest[1] - base[1];
}

export function deltaBadge(delta: number | null): string {
  if (delta === null || delta === 0) return "";
  const up = delta > 0;
  return `<span class="delta ${up ? "up" : "down"}" title="${esc(t("chart.delta24h"))}">${up ? "▲" : "▼"} ${Math.abs(delta)} LP</span>`;
}

/** Card sparkline; `data-chart` opens the full chart. */
export function sparkline(points: TrendPoint[], width = 200, height = 30): string {
  if (points.length < 2) {
    return `<span class="spark spark-empty" data-chart title="${esc(t("chart.open"))}">${esc(t("chart.collecting"))}</span>`;
  }
  const [t0, t1] = [points[0][0], points[points.length - 1][0]];
  const scores = points.map((point) => point[1]);
  const [lo, hi] = [Math.min(...scores), Math.max(...scores)];
  const span = Math.max(hi - lo, 20);
  const mid = (hi + lo) / 2;
  const x = (time: number) => 2 + ((time - t0) / Math.max(t1 - t0, 1)) * (width - 6);
  const y = (score: number) => height / 2 - ((score - mid) / span) * (height - 8);
  const path = points.map((point, index) => `${index ? "L" : "M"}${x(point[0]).toFixed(1)},${y(point[1]).toFixed(1)}`).join("");
  const last = points[points.length - 1];
  const area = `${path}L${x(last[0]).toFixed(1)},${height}L${x(t0).toFixed(1)},${height}Z`;
  // Stretched to the card width; the stroke keeps its width while scaling.
  return `<svg class="spark" data-chart viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" role="img" aria-label="${esc(t("chart.open"))}">
    <title>${esc(t("chart.open"))}</title>
    <path class="spark-area" d="${area}"/>
    <path class="spark-line" d="${path}" pathLength="1" vector-effect="non-scaling-stroke"/>
  </svg>`;
}

// ---------------------------------------------------------------- full chart

export type Range = "1d" | "7d" | "30d" | "all";
const RANGE_MS: Record<Range, number> = { "1d": DAY, "7d": 7 * DAY, "30d": 30 * DAY, all: Infinity };

const dateFormat = (options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat(lang(), options);

function ticks(t0: number, t1: number, count: number): number[] {
  if (t1 <= t0) return [t0];
  return Array.from({ length: count }, (_, index) => t0 + ((t1 - t0) * index) / (count - 1));
}

/**
 * Render the history chart into `root`. Returns nothing; interactivity
 * (crosshair + tooltip) is wired on the inserted SVG.
 */
export function renderHistoryChart(root: HTMLElement, all: HistoryPoint[], range: Range): void {
  const now = Date.now();
  const inRange = all.filter((point) => now - point.t <= RANGE_MS[range]);
  // Carry the last point before the window so the line starts at its edge.
  const before = [...all].reverse().find((point) => now - point.t > RANGE_MS[range]);
  const points = before && range !== "all" ? [{ ...before, t: now - RANGE_MS[range] }, ...inRange] : inRange;

  if (points.length < 1) {
    root.innerHTML = `<div class="chart-empty">${esc(t("chart.noData"))}</div>`;
    return;
  }
  // Extend a flat line to "now" so the latest standing is visible.
  const series = [...points];
  const last = series[series.length - 1];
  if (now - last.t > 60_000) series.push({ ...last, t: now });

  const width = 760;
  const height = 300;
  const pad = { top: 14, right: 16, bottom: 30, left: 112 };
  const plotW = width - pad.left - pad.right;
  const plotH = height - pad.top - pad.bottom;

  const scores = series.map((point) => point.score);
  let lo = Math.min(...scores);
  let hi = Math.max(...scores);
  const margin = Math.max(40, (hi - lo) * 0.15);
  lo = Math.max(0, lo - margin);
  hi = hi + margin;
  const t0 = series[0].t;
  const t1 = series[series.length - 1].t;
  const x = (time: number) => pad.left + ((time - t0) / Math.max(t1 - t0, 1)) * plotW;
  const y = (score: number) => pad.top + plotH - ((score - lo) / (hi - lo)) * plotH;

  // Tier bands and division gridlines inside the visible score window.
  const bands: string[] = [];
  const labels: string[] = [];
  for (let base = Math.floor(lo / 400) * 400; base < hi; base += 400) {
    const top = base >= APEX_BASE ? hi : base + 400;
    const tier = tierOfScore(base);
    const color = palette(tier).base;
    const y0 = y(Math.min(top, hi));
    const y1 = y(Math.max(base, lo));
    bands.push(`<rect x="${pad.left}" y="${y0.toFixed(1)}" width="${plotW}" height="${Math.max(0, y1 - y0).toFixed(1)}" fill="${color}" opacity="0.07"/>`);
    if (base >= lo) {
      bands.push(`<line class="grid tier-line" x1="${pad.left}" x2="${pad.left + plotW}" y1="${y(base).toFixed(1)}" y2="${y(base).toFixed(1)}"/>`);
    }
    const labelY = base >= APEX_BASE ? Math.min(y1 - 12, pad.top + 14) : (y0 + y1) / 2;
    if (y1 - y0 > 18) {
      labels.push(`<g transform="translate(${pad.left - 12}, ${labelY.toFixed(1)})">
        <g transform="translate(-16,-9)">${emblem(tier, 18)}</g>
        <text class="axis-label" x="-22" y="4" text-anchor="end">${esc(base >= APEX_BASE ? `${tierName("Master")}+` : tierName(tier))}</text>
      </g>`);
    }
    if (base >= APEX_BASE) {
      // One shared LP ladder above Master: label LP gridlines instead.
      const step = hi - lo > 600 ? 200 : 100;
      for (let score = Math.ceil(Math.max(lo, APEX_BASE) / step) * step; score < hi; score += step) {
        if (score === APEX_BASE) continue;
        bands.push(`<line class="grid" x1="${pad.left}" x2="${pad.left + plotW}" y1="${y(score).toFixed(1)}" y2="${y(score).toFixed(1)}"/>`);
        labels.push(`<text class="axis-label lp-label" x="${pad.left - 8}" y="${(y(score) + 4).toFixed(1)}" text-anchor="end">${score - APEX_BASE} LP</text>`);
      }
    }
    if (base < APEX_BASE) {
      for (let division = 1; division < 4; division++) {
        const score = base + division * 100;
        if (score > lo && score < hi) {
          bands.push(`<line class="grid" x1="${pad.left}" x2="${pad.left + plotW}" y1="${y(score).toFixed(1)}" y2="${y(score).toFixed(1)}"/>`);
        }
      }
    }
  }

  const span = t1 - t0;
  const timeFormat = dateFormat(span <= 2 * DAY ? { hour: "2-digit", minute: "2-digit" } : { day: "numeric", month: "short" });
  const xTicks = ticks(t0, t1, 5)
    .map((time) => `<text class="axis-label" x="${x(time).toFixed(1)}" y="${height - 8}" text-anchor="middle">${esc(timeFormat.format(time))}</text>`)
    .join("");

  // Step line: rank holds until the next observation.
  let path = "";
  series.forEach((point, index) => {
    const px = x(point.t).toFixed(1);
    const py = y(point.score).toFixed(1);
    if (index === 0) path = `M${px},${py}`;
    else path += `H${px}V${py}`;
  });
  const area = `${path}V${pad.top + plotH}H${x(t0).toFixed(1)}Z`;
  const markers = points
    .map((point) => `<circle class="chart-dot" cx="${x(point.t).toFixed(1)}" cy="${y(point.score).toFixed(1)}" r="3.5"/>`)
    .join("");

  root.innerHTML = `
    <svg class="history-chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="${esc(t("chart.title"))}">
      <defs>
        <linearGradient id="chart-area" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="var(--accent)" stop-opacity="0.22"/>
          <stop offset="1" stop-color="var(--accent)" stop-opacity="0"/>
        </linearGradient>
      </defs>
      ${bands.join("")}
      ${labels.join("")}
      ${xTicks}
      <path class="chart-area" d="${area}"/>
      <path class="chart-line" d="${path}" pathLength="1"/>
      ${markers}
      <line class="crosshair" y1="${pad.top}" y2="${pad.top + plotH}" visibility="hidden"/>
      <circle class="crosshair-dot" r="5" visibility="hidden"/>
      <rect class="hit" x="${pad.left}" y="${pad.top}" width="${plotW}" height="${plotH}" fill="transparent"/>
    </svg>
    <div class="chart-tip" hidden></div>`;

  const svg = root.querySelector("svg")!;
  const tip = root.querySelector<HTMLElement>(".chart-tip")!;
  const crosshair = svg.querySelector<SVGLineElement>(".crosshair")!;
  const dot = svg.querySelector<SVGCircleElement>(".crosshair-dot")!;
  const tipFormat = dateFormat({ day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  const hide = () => {
    tip.hidden = true;
    crosshair.setAttribute("visibility", "hidden");
    dot.setAttribute("visibility", "hidden");
  };
  svg.querySelector(".hit")!.addEventListener("pointermove", (event) => {
    const pointer = event as PointerEvent;
    const box = svg.getBoundingClientRect();
    const svgX = ((pointer.clientX - box.left) / box.width) * width;
    const time = t0 + ((svgX - pad.left) / plotW) * Math.max(t1 - t0, 1);
    // Snap to the observation in effect at that time.
    let current = points[0];
    for (const point of points) if (point.t <= time) current = point;
    const cx = x(Math.max(current.t, t0));
    crosshair.setAttribute("x1", String(svgX));
    crosshair.setAttribute("x2", String(svgX));
    crosshair.setAttribute("visibility", "visible");
    dot.setAttribute("cx", cx.toFixed(1));
    dot.setAttribute("cy", y(current.score).toFixed(1));
    dot.setAttribute("visibility", "visible");
    const rank = current.division && current.score < APEX_BASE ? `${tierName(current.tier)} ${current.division}` : tierName(current.tier);
    tip.innerHTML = `<span class="tip-date">${esc(tipFormat.format(current.t))}</span><strong>${esc(rank)}</strong><span>${current.lp} LP</span>`;
    tip.hidden = false;
    const left = (svgX / width) * box.width;
    tip.style.left = `${Math.min(Math.max(left, 70), box.width - 70)}px`;
    tip.style.top = `${(y(current.score) / height) * box.height}px`;
  });
  svg.querySelector(".hit")!.addEventListener("pointerleave", hide);
}

/** Summary numbers and the change log (the chart's table view). */
export function historySummary(all: HistoryPoint[], range: Range): string {
  const now = Date.now();
  const points = all.filter((point) => now - point.t <= RANGE_MS[range]);
  if (!points.length) return "";
  const scores = points.map((point) => point.score);
  const before = [...all].reverse().find((point) => now - point.t > RANGE_MS[range]);
  const start = before ?? points[0];
  const change = points[points.length - 1].score - start.score;
  const peak = Math.max(...scores);
  const low = Math.min(...scores);
  const changeClass = change > 0 ? "up" : change < 0 ? "down" : "";
  const listFormat = dateFormat({ day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  const rows = [...points]
    .reverse()
    .slice(0, 30)
    .map((point, index, list) => {
      const previous = list[index + 1] ?? (before && index === list.length - 1 ? before : undefined);
      const diff = previous ? point.score - previous.score : 0;
      const rank = point.division && point.score < APEX_BASE ? `${tierName(point.tier)} ${point.division}` : tierName(point.tier);
      return `<tr>
        <td>${esc(listFormat.format(point.t))}</td>
        <td><span class="cell-rank" style="--tier:${palette(point.tier).base}">${esc(rank)}</span></td>
        <td class="num">${point.lp} LP</td>
        <td class="num ${diff > 0 ? "up" : diff < 0 ? "down" : ""}">${diff ? `${diff > 0 ? "+" : ""}${diff}` : "—"}</td>
      </tr>`;
    })
    .join("");
  return `
    <div class="chart-stats">
      <div><span>${esc(t("chart.change"))}</span><strong class="${changeClass}">${change > 0 ? "+" : ""}${change} LP</strong></div>
      <div><span>${esc(t("chart.peak"))}</span><strong>${esc(scoreLabel(peak))}</strong></div>
      <div><span>${esc(t("chart.low"))}</span><strong>${esc(scoreLabel(low))}</strong></div>
      <div><span>${esc(t("chart.points"))}</span><strong>${points.length}</strong></div>
    </div>
    <details class="chart-table">
      <summary>${esc(t("chart.table"))}</summary>
      <table>
        <thead><tr><th>${esc(t("chart.date"))}</th><th>${esc(t("sort.rank"))}</th><th class="num">LP</th><th class="num">±</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>
    </details>`;
}
