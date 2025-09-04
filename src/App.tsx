import { useEffect, useState } from "react";
import {
  ensureMinerAndAccount,
  startMiner,
  stopMiner,
  onMinerEvent,
  onMinerLog,
  queryBalance,
} from "./api";
import { celebrate } from "./celebrate";

type Chain = "resonance" | "heisenberg" | "quantus";

export default function App() {
  const [account, setAccount] = useState<any>(null);
  const [chain, setChain] = useState<Chain>("resonance");
  const [logs, setLogs] = useState<string[]>([]);
  const [hps, setHps] = useState<number>(0);
  const [mining, setMining] = useState(false);
  const [balance, setBalance] = useState<string>("—");
  const [minerPath, setMinerPath] = useState<string>("");
  const [accountJsonPath, setAccountJsonPath] = useState<string>("");
  const [toast, setToast] = useState<string>("");

  function showToast(msg: string) {
    setToast(msg);
    // auto-hide after 4 seconds
    setTimeout(() => setToast(""), 4000);
  }

  useEffect(() => {
    ensureMinerAndAccount().then(({ minerPath, accountJsonPath, account }) => {
      setMinerPath(minerPath);
      setAccountJsonPath(accountJsonPath);
      setAccount(account); // shows ss58
    });
  }, []);

  useEffect(() => {
    const un1 = onMinerEvent((ev) => {
      if (ev.type === "Hashrate") setHps(ev.hps);
      if (ev.type === "FoundBlock") celebrate();
    });
    const un2 = onMinerLog((line) => {
      setLogs((prev) =>
        (prev.length > 400 ? prev.slice(-400) : prev).concat(line),
      );
    });
    return () => {
      un1.then((u) => u());
      un2.then((u) => u());
    };
  }, []);

  async function onStart() {
    const c = chain === "quantus" ? "resonance" : chain;
    if (!account || !minerPath) {
      showToast("Miner not ready yet. Please wait for installer/account.");
      return;
    }
    try {
      await startMiner(c, account.address, minerPath, []);
      setMining(true);
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
    } catch (err: any) {
      showToast(
        err?.message
          ? `Stop failed: ${err.message}`
          : `Stop failed: ${String(err)}`,
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

  return (
    <div className="p-6 max-w-3xl mx-auto font-sans">
      <h1 className="text-2xl font-bold mb-2">Quantus Miner (Demo)</h1>
      <p className="opacity-70 mb-6">
        Creates a local account and wraps the CLI miner.
      </p>

      <div className="rounded-2xl shadow p-4 mb-4 border">
        <div className="mb-2">Account Address</div>
        <div className="font-mono break-all">{account?.address ?? "…"}</div>
      </div>

      <div className="rounded-2xl shadow p-4 mb-4 border flex gap-3 items-center">
        <label>Chain</label>
        <select
          className="border rounded px-2 py-1"
          value={chain}
          onChange={(e) => setChain(e.target.value as Chain)}
        >
          <option value="resonance">Resonance (testnet)</option>
          <option value="heisenberg" disabled>
            Heisenberg (testnet – disabled, requires quantus-node
            0.1.6-98ceb8de72a)
          </option>
          <option value="quantus" disabled>
            Quantus (mainnet – disabled)
          </option>
        </select>

        <div className="ml-6 text-sm">
          <div className="opacity-70">Binary</div>
          <div className="font-mono break-all">
            {minerPath || "installing…"}
          </div>
        </div>
        <div className="text-sm">
          <div className="opacity-70">Account JSON</div>
          <div className="font-mono break-all">{accountJsonPath || "…"}</div>
        </div>
        <div className="text-sm">
          <div className="opacity-70">Planned command</div>
          <div className="font-mono break-all">
            {`${minerPath} --chain ${chain === "quantus" ? "live_resonance" : chain === "resonance" ? "live_resonance" : chain} --rewards-address ${account?.address ?? ""}`}
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
          onClick={refreshBalance}
        >
          Refresh Balance
        </button>
        <div className="ml-auto">
          Hashrate: <b>{hps ? `${hps.toFixed(0)} H/s` : "—"}</b>
        </div>
      </div>

      <div className="rounded-2xl shadow p-4 mb-4 border">
        <div className="mb-2">Balance</div>
        <div className="font-mono">{balance}</div>
      </div>

      <div className="rounded-2xl shadow p-4 border">
        <div className="mb-2">Console</div>
        <pre className="h-48 overflow-auto text-xs leading-tight bg-black text-green-300 p-3 rounded-md">
          {logs.join("\n")}
        </pre>
      </div>
      {toast && (
        <div className="fixed bottom-4 right-4 z-50 rounded px-3 py-2 bg-red-600 text-white shadow">
          {toast}
        </div>
      )}
    </div>
  );
}
