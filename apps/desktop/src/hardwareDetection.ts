import { invoke } from "@tauri-apps/api/core";
import type { HardwareProfile } from "./types";

export function detectHardware(): Promise<HardwareProfile> {
  return invoke<HardwareProfile>("detect_hardware");
}
