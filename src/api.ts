import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export async function ensureMinerAndAccount(): Promise<{
  minerPath: string;
  accountJsonPath: string;
  account: { address: string };
}> {
  return await invoke("ensure_miner_and_account");
}

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
  return listen<{ source: string; line: string }>("miner:log", (e) =>
    cb(`[${e.payload.source}] ${e.payload.line}`),
  );
}

export async function startMiner(
  chain: "resonance" | "heisenberg",
  rewardsAddress: string,
  binaryPath: string,
  extraArgs: string[] = [],
) {
  return await invoke("start_miner", {
    args: { chain, rewardsAddress, binaryPath, extraArgs },
  });
}
export async function stopMiner() {
  return await invoke("stop_miner");
}
export async function queryBalance(chain: string, address: string) {
  return await invoke("query_balance", { chain, address });
}
