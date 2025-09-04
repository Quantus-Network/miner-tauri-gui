import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ensureMinerAndAccount,
  startMiner,
  stopMiner,
  onMinerEvent,
  onMinerLog,
  queryBalance,
  onMinerStatus,
  onMinerMeta,
  onMinerState,
  type MinerStatus,
  type MinerMeta,
} from "./api";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { celebrate } from "./celebrate";

type Chain = "resonance" | "heisenberg" | "quantus";

export default function App() {
  const [account, setAccount] = useState<any>(null);
  const [chain, setChain] = useState<Chain>(() => {
    const s = localStorage.getItem("qm.chain");
    return (s as Chain) || "resonance";
  });
  const [logs, setLogs] = useState<string[]>(() => {
    try {
      const s = localStorage.getItem("qm.logs");
      const arr = s ? JSON.parse(s) : [];
      return Array.isArray(arr) ? arr.slice(-1000) : [];
    } catch {
      return [];
    }
  });
  const [hps, setHps] = useState<number>(0);
  const [mining, setMining] = useState(false);
  const [balance, setBalance] = useState<string>("—");
  const [minerPath, setMinerPath] = useState<string>("");
  const [accountJsonPath, setAccountJsonPath] = useState<string>("");
  const [toast, setToast] = useState<string>("");
  const [status, setStatus] = useState<
    "Idle" | "Starting" | "Syncing" | "Mining" | "Repairing" | "Error"
  >("Idle");
  const [syncBlock, setSyncBlock] = useState<number | null>(null);
  const [peers, setPeers] = useState<number | null>(null);
  const [best, setBest] = useState<number | null>(null);
  const [highest, setHighest] = useState<number | null>(null);
  const [safeMode, setSafeMode] = useState<boolean>(false);
  const [meta, setMeta] = useState<Partial<MinerMeta>>(() => {
    try {
      const s = localStorage.getItem("qm.meta");
      return s ? (JSON.parse(s) as Partial<MinerMeta>) : {};
    } catch {
      return {};
    }
  });
  const [lineLimit, setLineLimit] = useState<number>(() => {
    const v = parseInt(localStorage.getItem("qm.lineLimit") || "", 10);
    return Number.isFinite(v) && v > 0 ? v : 400;
  });
  // simple derived display for balance pill
  const balanceDisplay = balance && balance !== "—" ? balance : null;
  const [autoStart, setAutoStart] = useState<boolean>(
    () => localStorage.getItem("qm.autoStart") === "1",
  );
  const [logToFile, setLogToFile] = useState<boolean>(
    () => localStorage.getItem("qm.logToFile") === "1",
  );
  const [logFilePath, setLogFilePath] = useState<string>(
    () => localStorage.getItem("qm.logFilePath") || "",
  );
  type ThemeMode = "system" | "light" | "dark";
  const [theme, setTheme] = useState<ThemeMode>(
    () => (localStorage.getItem("qm.theme") as ThemeMode) || "system",
  );
  const lineLimitRef = useRef(lineLimit);
  useEffect(() => {
    const apply = (mode: ThemeMode) => {
      const root = document.documentElement;
      if (mode === "dark") {
        root.classList.add("dark");
      } else if (mode === "light") {
        root.classList.remove("dark");
      } else {
        const mql = window.matchMedia("(prefers-color-scheme: dark)");
        if (mql.matches) root.classList.add("dark");
        else root.classList.remove("dark");
      }
    };
    apply(theme);
    try {
      localStorage.setItem("qm.theme", theme);
    } catch {}
    if (theme === "system") {
      const mql = window.matchMedia("(prefers-color-scheme: dark)");
      const handler = () => apply("system");
      if (mql.addEventListener) mql.addEventListener("change", handler);
      else mql.addListener(handler);
      return () => {
        if (mql.removeEventListener) mql.removeEventListener("change", handler);
        else mql.removeListener(handler);
      };
    }
    return;
  }, [theme]);

  function showToast(msg: string) {
    setToast(msg);
    // auto-hide after 4 seconds
    setTimeout(() => setToast(""), 4000);
  }

  useEffect(() => {
    ensureMinerAndAccount().then(
      async ({ minerPath, accountJsonPath, account }) => {
        setMinerPath(minerPath);
        setAccountJsonPath(accountJsonPath);
        setAccount(account); // shows ss58

        // Auto-start scaffold: resume mining if previously active and auto-start is enabled
        const wasMining = localStorage.getItem("qm.wasMining") === "1";
        if (autoStart && wasMining && account && minerPath) {
          const c = chain === "quantus" ? "resonance" : chain;
          try {
            setStatus("Starting");
            setSyncBlock(null);
            await startMiner(c, account.address, minerPath, [], logToFile);
            setMining(true);
          } catch {
            // error visibility handled by existing toast/log logic
          }
        }
      },
    );
  }, [autoStart, chain]);
  useEffect(() => {
    lineLimitRef.current = lineLimit;
    localStorage.setItem("qm.lineLimit", String(lineLimit));
    setLogs((prev) => {
      const limit = Math.max(0, lineLimit);
      if (limit > 0 && prev.length > limit) {
        return prev.slice(-limit);
      }
      return prev;
    });
  }, [lineLimit]);

  useEffect(() => {
    const un1 = onMinerEvent((ev) => {
      if (ev.type === "Hashrate") {
        setHps(ev.hps);
        if (ev.hps > 0) {
          setStatus("Mining");
          setSyncBlock(null);
        }
      }
      if (ev.type === "FoundBlock") {
        // A block was actually accepted (strong signal) – celebrate.
        celebrate();
        setStatus("Mining");
        setSyncBlock(null);
      }
      // If we later add a "PreparedBlock" event (pre-proposal), only show a subtle "Maybe" signal.
      // For now this is a no-op until backend emits such an event.
    });
    const un2 = onMinerLog((line) => {
      const l = line.toLowerCase();

      // Tone down celebration for prepared blocks: show a soft "Maybe" status.
      // Typical substrate-ish logs: "prepared a block for proposing", "block proposal prepared", etc.
      if (
        l.includes("prepared a block for proposing") ||
        l.includes("block proposal prepared") ||
        l.includes("prepared block") ||
        l.includes("pre-proposing block")
      ) {
        // Do not trigger celebrate(); briefly indicate MAYBE by setting Syncing (neutral) and a toast.
        setStatus((prev) => (prev === "Mining" ? prev : "Syncing"));
        showToast("Block prepared (maybe) – waiting for acceptance…");
      }

      // Status inference:
      // - Repair loop messages from backend "ui" source
      if (
        l.includes("detected rocksdb corruption") ||
        l.includes("repairing database") ||
        l.includes("database wiped") ||
        l.includes("repair restart")
      ) {
        setStatus("Repairing");
      } else if (l.includes("repair complete")) {
        // After repair, node restarts and will begin syncing
        setStatus("Syncing");
      } else if (l.includes("importing block")) {
        const mBlock = line.match(/importing block\s+#(\d+)/i);
        if (mBlock) setSyncBlock(Number(mBlock[1]));
        setStatus("Syncing");
      } else if (l.includes("total chain work")) {
        setStatus("Syncing");
      } else if (l.includes("error")) {
        setStatus("Error");
      }

      setLogs((prev) => {
        const limit = Math.max(0, lineLimitRef.current || 0);
        const base =
          limit > 0 && prev.length > limit ? prev.slice(-limit) : prev;
        const next = base.concat(line);
        try {
          localStorage.setItem("qm.logs", JSON.stringify(next.slice(-1000)));
        } catch {}
        return next;
      });
    });
    const un3 = onMinerStatus((s: MinerStatus & { safe_mode?: boolean }) => {
      if (typeof s.peers === "number") setPeers(s.peers);
      if (typeof s.current_block === "number") {
        setBest(s.current_block);
        // In case RPC provides better signal, prefer RPC over log parsing.
        setStatus("Syncing");
      }
      if (typeof s.highest_block === "number") setHighest(s.highest_block);
      if (typeof s.is_syncing === "boolean" && !s.is_syncing) {
        // If RPC says not syncing and we have hashrate elsewhere, UI will move to Mining.
        setSyncBlock(null);
      }
      if (typeof s.safe_mode === "boolean") {
        setSafeMode(s.safe_mode);
      }
    });
    const un4 = onMinerMeta((m: MinerMeta) => {
      setMeta((prev) => {
        const merged = { ...prev, ...m };
        try {
          localStorage.setItem("qm.meta", JSON.stringify(merged));
        } catch {}
        return merged;
      });
      // capture logfile path if backend emitted it via miner:logfile
      if ((m as any)?.path) {
        const p = (m as any).path as string;
        setLogFilePath(p);
        try {
          localStorage.setItem("qm.logFilePath", p);
        } catch {}
      }
    });
    const un5 = onMinerState((s) => {
      if (typeof s.running === "boolean") {
        setMining(s.running);
        try {
          localStorage.setItem("qm.wasMining", s.running ? "1" : "0");
        } catch {}
      }
      if (s.phase === "starting") {
        setStatus("Starting");
      } else if (s.phase === "running") {
        // leave status inference to events/logs; ensure not idle
        if (status === "Idle") setStatus("Syncing");
      } else if (s.phase === "stopped") {
        setStatus("Idle");
      }
    });
    return () => {
      un1.then((u) => u());
      un2.then((u) => u());
      un3.then((u) => u());
      un4.then((u) => u());
      un5.then((u) => u());
    };
  }, []);

  async function onStart() {
    const c = chain === "quantus" ? "resonance" : chain;
    if (!account || !minerPath) {
      showToast("Miner not ready yet. Please wait for installer/account.");
      return;
    }
    try {
      setStatus("Starting");
      setSyncBlock(null);
      await startMiner(c, account.address, minerPath, []);
      setMining(true);
      try {
        localStorage.setItem("qm.wasMining", "1");
      } catch {}
    } catch (err: any) {
      showToast(
        err?.message
          ? `Start failed: ${err.message}`
          : `Start failed: ${String(err)}`,
      );
    }
  }
  async function onStop() {
    try {
      await stopMiner();
      setMining(false);
      try {
        localStorage.setItem("qm.wasMining", "0");
      } catch {}
      setStatus("Idle");
      setSyncBlock(null);
    } catch (err: any) {
      showToast(
        err?.message
          ? `Stop failed: ${err.message}`
          : `Stop failed: ${String(err)}`,
      );
    }
  }

  async function onRepair() {
    setStatus("Repairing");
    try {
      await invoke("repair_miner");
      showToast("Repair initiated. Node will restart and resync.");
      // Status will transition to Syncing as logs come in; keep as Repairing for now.
    } catch (err: any) {
      setStatus("Error");
      showToast(
        err?.message
          ? `Repair failed: ${err.message}`
          : `Repair failed: ${String(err)}`,
      );
    }
  }

  async function refreshBalance() {
    if (!account) return;
    // mainnet disabled; if picked, fall back to resonance
    const c = chain === "quantus" ? "resonance" : chain;
    const res: any = await queryBalance(c, account.address);
    setBalance(res.free);
  }

  const progressPct =
    typeof best === "number" && typeof highest === "number" && highest > 0
      ? Math.max(0, Math.min(100, Math.floor((best / highest) * 100)))
      : 0;

  return (
    <div className="p-6 max-w-6xl mx-auto font-sans">
      <div className="fixed top-4 right-4 z-40 flex flex-col items-end gap-1">
        <div className="flex items-center gap-2">
          <div
            className={`rounded-full px-3 py-1 text-xs font-semibold shadow ${
              status === "Mining"
                ? "bg-green-600 text-white"
                : status === "Syncing"
                  ? "bg-amber-500 text-black"
                  : status === "Starting"
                    ? "bg-blue-600 text-white"
                    : status === "Repairing"
                      ? "bg-purple-600 text-white"
                      : status === "Error"
                        ? "bg-red-600 text-white"
                        : "bg-gray-500 text-white"
            }`}
            title="Miner status"
          >
            {status === "Syncing"
              ? typeof best === "number" &&
                typeof highest === "number" &&
                highest > 0
                ? `Syncing #${best} / #${highest} (${Math.floor(
                    (best / highest) * 100,
                  )}%)`
                : syncBlock
                  ? `Syncing #${syncBlock}`
                  : "Syncing"
              : status}
          </div>
          {safeMode && (
            <div
              className="rounded-full px-3 py-1 text-xs font-semibold shadow bg-purple-700 text-white"
              title="Safe Sync mode (--max-blocks-per-request 1) is active"
            >
              Safe Sync
            </div>
          )}
          <div
            className={`rounded-full px-3 py-1 text-xs font-semibold shadow ${
              typeof peers !== "number"
                ? "bg-gray-600 text-white"
                : peers >= 3
                  ? "bg-green-600 text-white"
                  : peers >= 1
                    ? "bg-amber-500 text-black"
                    : "bg-red-600 text-white"
            }`}
            title="Peers / Best / Highest (RPC)"
          >
            {typeof peers === "number" ? `${peers} peers` : "— peers"} ·{" "}
            {typeof best === "number" ? `#${best}` : "#—"} /{" "}
            {typeof highest === "number" ? `#${highest}` : "#—"}
          </div>
          <label
            className="ml-2 flex items-center gap-1 text-xs font-semibold bg-black/80 text-white rounded-full px-3 py-1 shadow"
            title="Auto-start miner on launch if previously running"
          >
            <input
              type="checkbox"
              className="accent-blue-600"
              checked={autoStart}
              onChange={(e) => {
                const v = e.target.checked;
                setAutoStart(v);
                try {
                  localStorage.setItem("qm.autoStart", v ? "1" : "0");
                } catch {}
              }}
            />
            Auto-start
          </label>
          <select
            className="ml-2 border rounded px-2 py-1 text-xs"
            value={theme}
            onChange={(e) =>
              setTheme(e.target.value as "system" | "light" | "dark")
            }
            title="Theme"
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </div>
        {balanceDisplay && (
          <div className="pill bg-black/80 text-white" title="Balance">
            Balance: {balanceDisplay}
          </div>
        )}
        <div className="ml-2 flex items-center gap-2">
          <label
            className="text-xs opacity-70 flex items-center gap-1"
            title="Also write logs to a file"
          >
            <input
              type="checkbox"
              className="accent-blue-600"
              checked={logToFile}
              onChange={(e) => {
                const v = e.target.checked;
                setLogToFile(v);
                try {
                  localStorage.setItem("qm.logToFile", v ? "1" : "0");
                } catch {}
              }}
            />
            Log to file
          </label>
          {logFilePath ? (
            <>
              <span className="text-xs opacity-70">Log:</span>
              <button
                className="rounded px-2 py-1 border text-xs"
                title={logFilePath}
                onClick={() => revealItemInDir(logFilePath)}
              >
                Open
              </button>
            </>
          ) : null}
        </div>
        <div
          className="w-80 h-2 rounded bg-black/20 overflow-hidden"
          title="Sync progress"
          aria-label="Sync progress"
        >
          <div
            className={`h-full ${status === "Mining" ? "bg-green-600" : "bg-amber-500"}`}
            style={{ width: `${progressPct}%` }}
          />
        </div>
      </div>
      <h1 className="text-2xl font-bold mb-2">Quantus Miner (Demo)</h1>
      <p className="opacity-70 mb-6">
        Creates a local account and wraps the CLI miner.
      </p>

      <div className="grid gap-4 md:grid-cols-2 items-start">
        <div className="rounded-2xl shadow p-4 mb-4 border">
          <div className="mb-2 flex items-center gap-3">
            <span>Node Info</span>
            <button
              className="rounded px-2 py-1 border text-xs"
              onClick={() => {
                try {
                  navigator.clipboard.writeText(
                    JSON.stringify(meta ?? {}, null, 2),
                  );
                  showToast("Copied node info to clipboard");
                } catch {}
              }}
            >
              Copy
            </button>
            <button
              className="rounded px-2 py-1 border text-xs"
              title="Clear captured node info (kept across restarts)"
              onClick={() => {
                try {
                  localStorage.removeItem("qm.meta");
                } catch {}
                setMeta({});
                showToast("Node info reset");
              }}
            >
              Reset
            </button>
          </div>
          <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
            {meta?.version && (
              <div>
                <span className="opacity-70">Version</span>
                <div className="font-mono break-all">{meta.version}</div>
              </div>
            )}
            {meta?.chain_spec && (
              <div>
                <span className="opacity-70">Chain Spec</span>
                <div className="font-mono break-all">{meta.chain_spec}</div>
              </div>
            )}
            {meta?.node_name && (
              <div>
                <span className="opacity-70">Node name</span>
                <div className="font-mono break-all">{meta.node_name}</div>
              </div>
            )}
            {meta?.role && (
              <div>
                <span className="opacity-70">Role</span>
                <div className="font-mono break-all">{meta.role}</div>
              </div>
            )}
            {meta?.database && (
              <div className="col-span-2">
                <span className="opacity-70">Database</span>
                <div className="font-mono break-all">{meta.database}</div>
              </div>
            )}
            {meta?.local_identity && (
              <div className="col-span-2">
                <span className="opacity-70">Local identity</span>
                <div className="font-mono break-all">{meta.local_identity}</div>
              </div>
            )}
            {meta?.jsonrpc_addr && (
              <div>
                <span className="opacity-70">JSON-RPC</span>
                <div className="font-mono break-all">{meta.jsonrpc_addr}</div>
              </div>
            )}
            {meta?.prometheus_addr && (
              <div>
                <span className="opacity-70">Prometheus</span>
                <div className="font-mono break-all">
                  {meta.prometheus_addr}
                </div>
              </div>
            )}
            {typeof meta?.highest_known_block === "number" && (
              <div>
                <span className="opacity-70">Highest known</span>
                <div className="font-mono break-all">
                  #{meta.highest_known_block}
                </div>
              </div>
            )}
            <div className="col-span-2">
              <span className="opacity-70">Rewards address</span>
              <div className="font-mono break-all">
                {meta.rewards_address ?? account?.address ?? "…"}
              </div>
            </div>
          </div>

          {/* Collapsible advanced system details */}
          <details className="mt-3">
            <summary className="cursor-pointer text-xs opacity-70 hover:opacity-100">
              System details
            </summary>
            <div className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
              {meta?.os && (
                <div>
                  <span className="opacity-70">OS</span>
                  <div className="font-mono break-all">{meta.os}</div>
                </div>
              )}
              {meta?.arch && (
                <div>
                  <span className="opacity-70">Arch</span>
                  <div className="font-mono break-all">{meta.arch}</div>
                </div>
              )}
              {meta?.target && (
                <div>
                  <span className="opacity-70">Target</span>
                  <div className="font-mono break-all">{meta.target}</div>
                </div>
              )}
              {meta?.cpu && (
                <div className="col-span-2">
                  <span className="opacity-70">CPU</span>
                  <div className="font-mono break-all">{meta.cpu}</div>
                </div>
              )}
              {typeof meta?.cpu_cores === "number" && (
                <div>
                  <span className="opacity-70">CPU cores</span>
                  <div className="font-mono break-all">{meta.cpu_cores}</div>
                </div>
              )}
              {meta?.memory && (
                <div>
                  <span className="opacity-70">Memory</span>
                  <div className="font-mono break-all">{meta.memory}</div>
                </div>
              )}
              {meta?.kernel && (
                <div>
                  <span className="opacity-70">Kernel</span>
                  <div className="font-mono break-all">{meta.kernel}</div>
                </div>
              )}
              {meta?.distro && (
                <div className="col-span-2">
                  <span className="opacity-70">Distro</span>
                  <div className="font-mono break-all">{meta.distro}</div>
                </div>
              )}
              {meta?.vm && (
                <div>
                  <span className="opacity-70">VM</span>
                  <div className="font-mono break-all">{meta.vm}</div>
                </div>
              )}
            </div>
          </details>
        </div>

        <div className="rounded-2xl shadow p-4 mb-4 border flex flex-wrap gap-3 items-start">
          <label>Chain</label>
          <select
            className="border rounded px-2 py-1"
            value={chain}
            onChange={(e) => {
              const c = e.target.value as Chain;
              setChain(c);
              try {
                localStorage.setItem("qm.chain", c);
              } catch {}
            }}
          >
            <option value="resonance">Resonance (testnet)</option>
            <option value="heisenberg" disabled>
              Heisenberg (testnet – disabled)
            </option>
            <option value="quantus" disabled>
              Quantus (mainnet – disabled)
            </option>
          </select>

          <div className="basis-full text-sm">
            <div className="opacity-70 flex items-center gap-2">
              <span>Binary</span>
              <button
                className="rounded px-2 py-0.5 border text-xs"
                onClick={() => {
                  try {
                    navigator.clipboard.writeText(minerPath || "");
                    showToast("Copied");
                  } catch {}
                }}
              >
                Copy
              </button>
            </div>
            <div className="font-mono break-all whitespace-pre-wrap">
              {minerPath || "installing…"}
            </div>
          </div>
          <div className="basis-full text-sm">
            <div className="opacity-70 flex items-center gap-2">
              <span>Account JSON</span>
              <button
                className="rounded px-2 py-0.5 border text-xs"
                onClick={() => {
                  try {
                    navigator.clipboard.writeText(accountJsonPath || "");
                    showToast("Copied");
                  } catch {}
                }}
              >
                Copy
              </button>
              <button
                className="rounded px-2 py-0.5 border text-xs"
                onClick={async () => {
                  try {
                    if (accountJsonPath) {
                      await revealItemInDir(accountJsonPath);
                    } else {
                      showToast("Account path not available yet");
                    }
                  } catch (e) {
                    console.error("open account json failed", e);
                  }
                }}
                title="Open the account JSON file in your system file manager"
              >
                Open
              </button>
            </div>
            <div className="font-mono break-all whitespace-pre-wrap">
              {accountJsonPath || "…"}
            </div>
          </div>
          <div className="basis-full text-sm">
            <div className="opacity-70 flex items-center gap-2">
              <span>Planned command</span>
              <button
                className="rounded px-2 py-0.5 border text-xs"
                onClick={() => {
                  try {
                    navigator.clipboard.writeText(
                      `${minerPath} --chain ${chain === "quantus" ? "live_resonance" : chain === "resonance" ? "live_resonance" : chain} --rewards-address ${account?.address ?? ""}`,
                    );
                    showToast("Copied");
                  } catch {}
                }}
              >
                Copy
              </button>
            </div>
            <div className="font-mono break-all whitespace-pre-wrap">
              {`${minerPath} --chain ${
                chain === "quantus"
                  ? "live_resonance"
                  : chain === "resonance"
                    ? "live_resonance"
                    : chain
              } --rewards-address ${account?.address ?? ""}`}
            </div>
          </div>

          {!mining ? (
            <button className="rounded-xl px-3 py-2 border" onClick={onStart}>
              Start
            </button>
          ) : (
            <button className="rounded-xl px-3 py-2 border" onClick={onStop}>
              Stop
            </button>
          )}

          <button
            className="rounded-xl px-3 py-2 border"
            onClick={() => {
              const ok = confirm(
                "Resync will stop the node, delete the local chain database, and restart syncing from genesis. This may take a long time. Continue?",
              );
              if (ok) {
                onRepair();
              }
            }}
            title="Stops the node, wipes the database, and restarts from genesis"
          >
            Resync
          </button>

          <button
            className="rounded-xl px-3 py-2 border"
            onClick={async () => {
              const ok = confirm(
                "Unlock will stop the node, remove the leftover DB LOCK file, and restart the node. Use this if you saw a 'Resource temporarily unavailable' lock error. Continue?",
              );
              if (ok) {
                try {
                  // invoke backend unlock command
                  const { invoke } = await import("@tauri-apps/api/core");
                  await invoke("unlock_miner");
                } catch (e) {
                  console.error("unlock_miner failed", e);
                }
              }
            }}
            title="Stops the node, removes leftover DB lock, and restarts"
          >
            Unlock
          </button>
          <button
            className="rounded-xl px-3 py-2 border"
            onClick={refreshBalance}
          >
            Refresh Balance
          </button>
          <div className="ml-auto">
            Hashrate: <b>{hps ? `${hps.toFixed(0)} H/s` : "—"}</b>
          </div>
        </div>

        <div className="rounded-2xl shadow p-4 border md:col-span-2">
          <div className="mb-2 flex items-center gap-3">
            <span>Console</span>
            <span className="text-xs opacity-70">Lines</span>
            <input
              type="number"
              className="border rounded px-2 py-1 w-24"
              min={50}
              max={5000}
              step={50}
              value={lineLimit}
              onChange={(e) => setLineLimit(Number(e.target.value) || 0)}
            />
            <button
              className="rounded px-2 py-1 border text-xs"
              title="Clear console and forget stored lines"
              onClick={() => {
                setLogs([]);
                try {
                  localStorage.removeItem("qm.logs");
                } catch {}
              }}
            >
              Clear
            </button>
            <button
              className="rounded px-2 py-1 border text-xs"
              title="Export console to a text file"
              onClick={() => {
                try {
                  const data = logs.join("\n");
                  const blob = new Blob([data], { type: "text/plain" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  a.download = `quantus-logs-${new Date()
                    .toISOString()
                    .replace(/[:.]/g, "-")}.txt`;
                  document.body.appendChild(a);
                  a.click();
                  a.remove();
                  URL.revokeObjectURL(url);
                } catch (e) {
                  console.error("Export logs failed", e);
                }
              }}
            >
              Export
            </button>
          </div>
          <pre className="max-h-[30vh] overflow-auto text-xs leading-tight bg-black text-green-300 p-3 rounded-md scrollbar-thin">
            {logs.join("\n")}
          </pre>
        </div>
        {toast && (
          <div className="fixed bottom-4 right-4 z-50 rounded px-3 py-2 bg-red-600 text-white shadow">
            {toast}
          </div>
        )}
      </div>
    </div>
  );
}
