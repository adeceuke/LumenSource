import { useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import { formatBytes } from "../modelUi";
import type { PerformanceTestResult, RunningModelEntry } from "../types";

interface PerformanceTestPanelProps {
  model: RunningModelEntry;
  onSaved: (model: RunningModelEntry) => void;
  onReviewAlternative?: (modelId: string) => void;
}

export function PerformanceTestPanel({ model, onSaved, onReviewAlternative }: PerformanceTestPanelProps) {
  const [result, setResult] = useState<PerformanceTestResult | undefined>(model.performanceTest);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const supported = model.runtimeId === "ollama" && Boolean(model.runtimeModelId);

  const run = async () => {
    if (!model.runtimeModelId || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      const measured = await desktopCommands.runPerformanceTest(
        model.id,
        model.modelId,
        model.runtimeModelId,
        model.targetId,
      );
      setResult(measured);
      onSaved({ ...model, performanceTest: measured });
    } catch (testError) {
      setError(messageFromError(testError, "The performance test could not be completed."));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="detail-section performance-test-section" aria-labelledby="performance-test-title">
      <div className="detail-section-header">
        <div>
          <h2 id="performance-test-title">Performance Test</h2>
          <p>Optional local calibration. It does not affect installation validity or change the selected model.</p>
        </div>
        <button className="primary-button" type="button" disabled={!supported || busy} onClick={() => void run()}>
          {busy ? "Testing…" : result ? "Run test again" : "Run performance test"}
        </button>
      </div>

      {!supported && (
        <div className="details-empty full-detail-empty">
          Standardized performance testing is currently available for managed Ollama chat models.
        </div>
      )}
      {error && <div className="inline-error" role="alert">{error}</div>}
      {busy && (
        <div className="performance-test-progress" role="status">
          <span className="model-control-spinner" />
          <div><strong>Running a warm-up and two measured requests…</strong><small>This can take several minutes on a CPU-only model.</small></div>
        </div>
      )}

      {supported && !result && !busy && (
        <div className="performance-test-intro">
          <div className="performance-grid full-performance-grid">
            <MetricCard label="First visible token" value="Target ≤ 20 s" hint="Delay before answer text appears" />
            <MetricCard label="Generation" value="Target ≥ 5 tok/s" hint="Output tokens generated per second" />
            <MetricCard label="Prompt processing" value="Measured" hint="No universal interactive target" />
            <MetricCard label="Allocation" value="Observed" hint="CPU, mixed, or GPU model placement" />
          </div>
          <p className="performance-test-note">The workload is private and synthetic. Overall CPU percentage is intentionally omitted: CPU inference may use only a subset of available cores, so whole-machine utilization can be misleading.</p>
        </div>
      )}

      {result && !busy && <PerformanceTestResults result={result} onReviewAlternative={onReviewAlternative} />}
    </section>
  );
}

export function PerformanceTestResults({
  result,
  compact = false,
  onReviewAlternative,
}: {
  result: PerformanceTestResult;
  compact?: boolean;
  onReviewAlternative?: (modelId: string) => void;
}) {
  const visible = result.firstVisibleTokenMillis ?? result.firstTokenMillis;
  return (
    <div className={`performance-test-results ${compact ? "compact" : ""}`}>
      <div className={`performance-test-verdict ${result.assessment}`}>
        <strong>{assessmentLabel(result.assessment)}</strong>
        <span>{result.summary}</span>
        <small>Measured {new Date(result.testedAt).toLocaleString()}</small>
      </div>
      <div className="performance-grid full-performance-grid">
        <MetricCard label="First visible token" value={formatMillis(visible)} hint={`Target ≤ 20 s · runtime token ${formatMillis(result.firstTokenMillis)}`} />
        <MetricCard label="Generation" value={`${result.generationTokensPerSecond.toFixed(1)} tok/s`} hint={`Target ≥ 5 tok/s · ${result.generatedTokenCount} tokens/run`} />
        <MetricCard label="Prompt processing" value={`${result.promptTokensPerSecond.toFixed(1)} tok/s`} hint={`${result.promptTokenCount} input tokens per run`} />
        <MetricCard label="Model load" value={formatMillis(result.loadMillis)} hint="Warm-up load time; near zero if already resident" />
        <MetricCard label="Model allocation" value={formatBytes(result.allocatedMemoryBytes)} hint={`${formatBytes(result.allocatedVramBytes)} in VRAM`} />
        <MetricCard label="Execution" value={allocationLabel(result.accelerator)} hint={result.accelerator === "cpu" ? "Whole-machine CPU usage is omitted because the runtime may use only part of the CPU." : "Observed from the runtime's model allocation."} />
      </div>
      <div className="performance-test-details">
        <p><strong>Evidence:</strong> {result.evidence}</p>
        <p><strong>Workload:</strong> {result.workload}</p>
        <p><strong>Hardware:</strong> {result.hardwareSummary}</p>
        {typeof result.expectedGenerationTokensPerSecond === "number" && (
          <p><strong>Catalog generation measurement:</strong> {result.expectedGenerationTokensPerSecond.toFixed(1)} tok/s</p>
        )}
      </div>
      {result.recommendation && (
        <aside className={`performance-recommendation ${result.recommendation.direction}`}>
          <span>{result.recommendation.direction === "faster" ? "Faster alternative" : "Quality-oriented alternative"}</span>
          <h3>{result.recommendation.modelName}</h3>
          <p>{result.recommendation.reason}</p>
          <dl>
            <div><dt>Estimated first token</dt><dd>{result.recommendation.estimatedFirstVisibleTokenMillis ? formatMillis(result.recommendation.estimatedFirstVisibleTokenMillis) : "Unavailable"}</dd></div>
            <div><dt>Estimated generation</dt><dd>{result.recommendation.estimatedGenerationTokensPerSecond.toFixed(1)} tok/s</dd></div>
          </dl>
          {onReviewAlternative && (
            <button className="secondary-button" type="button" onClick={() => onReviewAlternative(result.recommendation!.modelId)}>Review alternative</button>
          )}
          <small>This is a suggestion only. Lumen Source will not download, replace, or remove a model automatically.</small>
        </aside>
      )}
    </div>
  );
}

function MetricCard({ label, value, hint }: { label: string; value: string; hint: string }) {
  return <div className="performance-card"><span>{label}</span><strong>{value}</strong><small>{hint}</small></div>;
}

function formatMillis(milliseconds: number): string {
  if (milliseconds < 1_000) return `${Math.round(milliseconds)} ms`;
  return `${(milliseconds / 1_000).toFixed(milliseconds < 10_000 ? 1 : 0)} s`;
}

function assessmentLabel(assessment: PerformanceTestResult["assessment"]): string {
  if (assessment === "slow") return "Slow for interactive use";
  if (assessment === "fast") return "Highly responsive";
  return "Balanced performance";
}

function allocationLabel(accelerator: PerformanceTestResult["accelerator"]): string {
  if (accelerator === "cpu") return "CPU only";
  if (accelerator === "gpu") return "GPU";
  if (accelerator === "mixed") return "Mixed CPU/GPU";
  return "Unavailable";
}
