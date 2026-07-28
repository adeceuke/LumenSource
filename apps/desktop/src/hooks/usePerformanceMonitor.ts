import { useEffect, useState } from "react";
import { desktopCommands } from "../commands";
import type { PerformanceSnapshot, RunningModelEntry } from "../types";

export interface PerformanceHistoryPoint {
  sampledAtUnixMs: number;
  totalMemoryBytes: number;
  systemMemoryBytes: number;
  vramBytes: number;
}

interface PerformanceMonitor {
  snapshot?: PerformanceSnapshot;
  history: PerformanceHistoryPoint[];
  loading: boolean;
  error?: string;
}

export function usePerformanceMonitor(
  model: RunningModelEntry | undefined,
  errorFrom: (error: unknown) => string,
): PerformanceMonitor {
  const [snapshot, setSnapshot] = useState<PerformanceSnapshot>();
  const [history, setHistory] = useState<PerformanceHistoryPoint[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!model?.runtimeModelId) return;

    let disposed = false;
    let refreshInFlight = false;
    const modelId = model.modelId;
    const runtimeModelId = model.runtimeModelId;
    const refresh = async () => {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const nextSnapshot = await desktopCommands.performance(modelId, runtimeModelId, model.targetId);
        if (!disposed) {
          setSnapshot(nextSnapshot);
          const historyPoint: PerformanceHistoryPoint = {
            sampledAtUnixMs: nextSnapshot.sampledAtUnixMs,
            totalMemoryBytes: nextSnapshot.allocatedMemoryBytes,
            systemMemoryBytes: nextSnapshot.allocatedSystemMemoryBytes,
            vramBytes: nextSnapshot.allocatedVramBytes,
          };
          setHistory((current) => {
            const withoutDuplicate = current.filter((point) => point.sampledAtUnixMs !== historyPoint.sampledAtUnixMs);
            return [...withoutDuplicate, historyPoint].slice(-60);
          });
          setError(undefined);
        }
      } catch (performanceFailure) {
        if (!disposed) setError(errorFrom(performanceFailure));
      } finally {
        refreshInFlight = false;
        if (!disposed) setLoading(false);
      }
    };

    setSnapshot(undefined);
    setHistory([]);
    setError(undefined);
    setLoading(true);
    void refresh();
    const interval = window.setInterval(() => void refresh(), 2_000);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [errorFrom, model?.modelId, model?.runtimeModelId, model?.targetId]);

  return { snapshot, history, loading, error };
}
