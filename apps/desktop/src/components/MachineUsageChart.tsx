import { browserMessages } from "../i18n";
import type { MachineUsageHistoryPoint } from "../hooks/useMachineMonitor";

const text = browserMessages();

export function MachineUsageChart({ history }: { history: MachineUsageHistoryPoint[] }) {
  const width = 800;
  const height = 270;
  const padding = { top: 18, right: 18, bottom: 38, left: 46 };
  const plotWidth = width - padding.left - padding.right;
  const plotHeight = height - padding.top - padding.bottom;
  const hasGpu = history.some((point) => point.gpuPercent !== undefined);
  const series = [
    {
      key: "cpu",
      label: text.machines.cpuUsage,
      color: "#d98938",
      values: history.map((point) => point.cpuPercent),
    },
    {
      key: "memory",
      label: text.machines.memoryUsage,
      color: "#65a9d7",
      values: history.map((point) => point.memoryPercent),
    },
    ...(hasGpu ? [{
      key: "gpu",
      label: text.machines.gpuUsage,
      color: "#b978d1",
      values: history.map((point) => point.gpuPercent ?? 0),
    }] : []),
  ];
  const x = (index: number) => padding.left
    + (history.length <= 1 ? 0 : (index / (history.length - 1)) * plotWidth);
  const y = (value: number) => padding.top
    + ((100 - Math.max(0, Math.min(100, value))) / 100) * plotHeight;
  const firstSample = history[0]?.sampledAtUnixMs;
  const lastSample = history.at(-1)?.sampledAtUnixMs;

  return (
    <section className="performance-chart-card" aria-labelledby="machine-usage-history-title">
      <div className="chart-header">
        <div>
          <h3 id="machine-usage-history-title">{text.machines.chartTitle}</h3>
          <p>{text.machines.chartDescription}</p>
        </div>
        <div className="chart-legend" aria-label={text.machines.chartSeriesLabel}>
          {series.map((item) => (
            <span key={item.key}>
              <i style={{ background: item.color }} />
              {item.label}
              <b>{text.machines.percent(item.values.at(-1) ?? 0)}</b>
            </span>
          ))}
        </div>
      </div>
      {history.length < 2 ? (
        <div className="chart-collecting">
          <span className="model-control-spinner" /> {text.machines.collecting}
        </div>
      ) : (
        <svg
          className="performance-chart"
          viewBox={`0 0 ${width} ${height}`}
          role="img"
          aria-label={text.machines.chartAria}
        >
          {[0, 25, 50, 75, 100].map((value) => (
            <g key={value}>
              <line
                x1={padding.left}
                x2={width - padding.right}
                y1={y(value)}
                y2={y(value)}
                className="chart-grid-line"
              />
              <text
                x={padding.left - 10}
                y={y(value) + 4}
                textAnchor="end"
                className="chart-axis-label"
              >
                {value}%
              </text>
            </g>
          ))}
          {series.map((item) => (
            <polyline
              key={item.key}
              points={item.values.map((value, index) => `${x(index)},${y(value)}`).join(" ")}
              fill="none"
              stroke={item.color}
              className="chart-series-line"
            />
          ))}
          <text x={padding.left} y={height - 10} className="chart-time-label">
            {firstSample ? new Date(firstSample).toLocaleTimeString(text.locale) : ""}
          </text>
          <text
            x={width - padding.right}
            y={height - 10}
            textAnchor="end"
            className="chart-time-label"
          >
            {lastSample ? new Date(lastSample).toLocaleTimeString(text.locale) : ""}
          </text>
        </svg>
      )}
    </section>
  );
}
