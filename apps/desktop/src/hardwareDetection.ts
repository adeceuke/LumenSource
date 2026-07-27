import { invoke } from "@tauri-apps/api/core";
import type { HardwareProfile } from "./types";

export function detectHardware(targetId: string): Promise<HardwareProfile> {
  return invoke<HardwareProfile>("detect_hardware", { targetId });
}
