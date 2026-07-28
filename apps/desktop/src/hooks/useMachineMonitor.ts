import { useCallback, useEffect, useState } from "react";
import { desktopCommands } from "../commands";
import type { HardwareProfile, MachineUsageSnapshot } from "../types";

export interface MachineUsageHistoryPoint {
  sampledAtUnixMs: number;
  cpuPercent: number;
  memoryPercent: number;
  gpuPercent?: number;
}

interface MachineMonitor {
  hardware?: HardwareProfile;
  hardwareLoading: boolean;
  hardwareError?: string;
  retryHardware: () => void;
  snapshot?: MachineUsageSnapshot;
  history: MachineUsageHistoryPoint[];
  usageLoading: boolean;
  usageError?: string;
}

export function useMachineMonitor(
  targetId: string,
  password: string | undefined,
  passwordRequired: boolean,
  errorFrom: (error: unknown) => string,
): MachineMonitor {
  const [hardware, setHardware] = useState<HardwareProfile>();
  const [hardwareLoading, setHardwareLoading] = useState(false);
  const [hardwareError, setHardwareError] = useState<string>();
  const [hardwareAttempt, setHardwareAttempt] = useState(0);
  const [snapshot, setSnapshot] = useState<MachineUsageSnapshot>();
  const [history, setHistory] = useState<MachineUsageHistoryPoint[]>([]);
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageError, setUsageError] = useState<string>();
  const ready = !passwordRequired || Boolean(password);

  const retryHardware = useCallback(() => {
    setHardwareAttempt((current) => current + 1);
  }, []);

  useEffect(() => {
    setHardware(undefined);
    setHardwareError(undefined);
    if (!ready) {
      setHardwareLoading(false);
      return;
    }

    let disposed = false;
    setHardwareLoading(true);
    void desktopCommands.detectHardware(targetId, password)
      .then((profile) => {
        if (!disposed) {
          setHardware(profile);
          setHardwareError(undefined);
        }
      })
      .catch((failure: unknown) => {
        if (!disposed) setHardwareError(errorFrom(failure));
      })
      .finally(() => {
        if (!disposed) setHardwareLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [errorFrom, hardwareAttempt, password, ready, targetId]);

  useEffect(() => {
    setSnapshot(undefined);
    setHistory([]);
    setUsageError(undefined);
    if (!ready) {
      setUsageLoading(false);
      return;
    }

    let disposed = false;
    let refreshInFlight = false;
    const refresh = async () => {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const nextSnapshot = await desktopCommands.machineUsage(targetId, password);
        if (!disposed) {
          const totalMemory = nextSnapshot.usedMemoryBytes + nextSnapshot.availableMemoryBytes;
          const gpuPercent = nextSnapshot.accelerators
            .map((accelerator) => accelerator.utilizationPercent)
            .find((value): value is number => value !== null);
          const point: MachineUsageHistoryPoint = {
            sampledAtUnixMs: nextSnapshot.sampledAtUnixMs,
            cpuPercent: nextSnapshot.cpuUtilizationPercent,
            memoryPercent: totalMemory > 0
              ? (nextSnapshot.usedMemoryBytes / totalMemory) * 100
              : 0,
            gpuPercent,
          };
          setSnapshot(nextSnapshot);
          setHistory((current) => [
            ...current.filter((item) => item.sampledAtUnixMs !== point.sampledAtUnixMs),
            point,
          ].slice(-60));
          setUsageError(undefined);
        }
      } catch (failure) {
        if (!disposed) setUsageError(errorFrom(failure));
      } finally {
        refreshInFlight = false;
        if (!disposed) setUsageLoading(false);
      }
    };

    setUsageLoading(true);
    void refresh();
    const interval = window.setInterval(() => void refresh(), 2_000);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [errorFrom, password, ready, targetId]);

  return {
    hardware,
    hardwareLoading,
    hardwareError,
    retryHardware,
    snapshot,
    history,
    usageLoading,
    usageError,
  };
}
