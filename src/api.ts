import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type MinerEvent =
  | { type: "Connected" }
  | { type: "Hashrate"; hps: number }
  | { type: "ShareAccepted" }
  | { type: "FoundBlock"; height?: number; hash?: string }
  | { type: "Error"; message: string };

export function onMinerEvent(cb: (ev: MinerEvent) => void) {
  return listen<MinerEvent>("miner:event", (e) => cb(e.payload));
}
export function onMinerLog(cb: (line: string) => void) {
  return listen<string>("miner:log", (e) => cb(e.payload));
}

export async function initAccount() {
  return await invoke("init_account");
}
export async function readAccount() {
  return await invoke("read_account");
}
export async function startMiner(
  chain: "resonance" | "heisenberg",
  binaryPath: string,
  extraArgs: string[] = [],
) {
  return await invoke("start_miner", {
    args: { chain, binaryPath, extraArgs },
  });
}
export async function stopMiner() {
  return await invoke("stop_miner");
}
export async function queryBalance(chain: string, address: string) {
  return await invoke("query_balance", { chain, address });
}
