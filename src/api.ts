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
    cb(e.payload.line),
  );
}

export type MinerStatus = {
  peers?: number | null;
  current_block?: number | null;
  highest_block?: number | null;
  is_syncing?: boolean | null;
};
export type MinerState = {
  running?: boolean;
  phase?: "starting" | "running" | "stopped";
};
export function onMinerState(cb: (s: MinerState) => void) {
  return listen<MinerState>("miner:state", (e) => cb(e.payload));
}

export function onMinerStatus(cb: (s: MinerStatus) => void) {
  return listen<MinerStatus>("miner:status", (e) => cb(e.payload));
}

export type MinerMeta = {
  binary?: string | null;
  chain?: string | null;
  rewards_address?: string | null;

  version?: string | null;
  chain_spec?: string | null;
  node_name?: string | null;
  role?: string | null;
  database?: string | null;
  local_identity?: string | null;
  jsonrpc_addr?: string | null;
  prometheus_addr?: string | null;
  highest_known_block?: number | null;

  os?: string | null;
  arch?: string | null;
  target?: string | null;
  cpu?: string | null;
  cpu_cores?: number | null;
  memory?: string | null;
  kernel?: string | null;
  distro?: string | null;
  vm?: string | null;
};

export function onMinerMeta(cb: (m: MinerMeta) => void) {
  return listen<MinerMeta>("miner:meta", (e) => cb(e.payload));
}

/**
 * Subscribe to logfile events.
 * Payload may include a 'kind' field:
 *  - "ext" for external miner
 *  - "node" for quantus-node
 * Prefer external miner path when available; otherwise fall back to node log path.
 */
export function onMinerLogFile(
  cb: (path: string, kind: "ext" | "node") => void,
) {
  return listen<{ path: string; kind?: "ext" | "node" }>(
    "miner:logfile",
    (e) => {
      const kind = e.payload.kind || "node";
      cb(e.payload.path, kind);
    },
  );
}

export async function startMiner(
  chain: "resonance" | "heisenberg",
  rewardsAddress: string,
  binaryPath: string,
  extraArgs: string[] = [],
  logToFile: boolean = false,
  externalNumCores?: number,
  externalPort?: number,
) {
  try {
    return await invoke("start_miner", {
      args: {
        chain,
        rewards_address: rewardsAddress,
        binary_path: binaryPath,
        extra_args: extraArgs,
        log_to_file: logToFile,
        external_num_cores: externalNumCores,
        external_port: externalPort,
      },
    });
  } catch (err) {
    console.error("start_miner failed", err);
    throw err;
  }
}
export async function stopMiner() {
  return await invoke("stop_miner");
}
export async function queryBalance(chain: string, address: string) {
  return await invoke("query_balance", { chain, address });
}
