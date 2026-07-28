import { browserMessages } from "../i18n";
import type { PerformanceHistoryPoint } from "../hooks/usePerformanceMonitor";
import { formatBytes } from "../modelUi";

const text = browserMessages();

export function PerformanceChart({ history }: { history: PerformanceHistoryPoint[] }) {
  const width = 800;
  const height = 270;
  const padding = { top: 18, right: 18, bottom: 38, left: 46 };
  const plotWidth = width - padding.left - padding.right;
  const plotHeight = height - padding.top - padding.bottom;
  const series = [
    { key: "total", label: text.performance.totalMemory, color: "#d98938", values: history.map((point) => point.totalMemoryBytes) },
    { key: "system", label: text.performance.systemMemory, color: "#65a9d7", values: history.map((point) => point.systemMemoryBytes) },
    { key: "vram", label: text.performance.vram, color: "#b978d1", values: history.map((point) => point.vramBytes) },
  ];
  const gibibyte = 1024 ** 3;
  const maximumObserved = Math.max(gibibyte, ...series.flatMap((item) => item.values));
  const maximumBytes = Math.ceil(maximumObserved / gibibyte) * gibibyte;
  const x = (index: number) => padding.left + (history.length <= 1 ? 0 : (index / (history.length - 1)) * plotWidth);
  const y = (value: number) => padding.top + ((maximumBytes - Math.max(0, Math.min(maximumBytes, value))) / maximumBytes) * plotHeight;
  const firstSample = history[0]?.sampledAtUnixMs;
  const lastSample = history.at(-1)?.sampledAtUnixMs;

  return (
    <section className="performance-chart-card" aria-labelledby="performance-history-title">
      <div className="chart-header">
        <div>
          <h3 id="performance-history-title">{text.performance.chartTitle}</h3>
          <p>{text.performance.chartDescription}</p>
        </div>
        <div className="chart-legend" aria-label={text.performance.chartSeriesLabel}>
          {series.map((item) => {
            const current = item.values.at(-1) ?? 0;
            return (
              <span key={item.key}><i style={{ background: item.color }} />{item.label}<b>{formatBytes(current)}</b></span>
            );
          })}
        </div>
      </div>
      {history.length < 2 ? (
        <div className="chart-collecting"><span className="model-control-spinner" /> {text.performance.collecting}</div>
      ) : (
        <svg className="performance-chart" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={text.performance.chartAria}>
          {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
            const value = maximumBytes * ratio;
            return (
              <g key={ratio}>
                <line x1={padding.left} x2={width - padding.right} y1={y(value)} y2={y(value)} className="chart-grid-line" />
                <text x={padding.left - 10} y={y(value) + 4} textAnchor="end" className="chart-axis-label">{formatBytes(value)}</text>
              </g>
            );
          })}
          {series.map((item) => {
            const points = item.values.map((value, index) => `${x(index)},${y(value)}`).join(" ");
            return <polyline key={item.key} points={points} fill="none" stroke={item.color} className="chart-series-line" />;
          })}
          <text x={padding.left} y={height - 10} className="chart-time-label">{firstSample ? new Date(firstSample).toLocaleTimeString(text.locale) : ""}</text>
          <text x={width - padding.right} y={height - 10} textAnchor="end" className="chart-time-label">{lastSample ? new Date(lastSample).toLocaleTimeString(text.locale) : text.performance.now}</text>
        </svg>
      )}
    </section>
  );
}
