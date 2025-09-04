import { useEffect, useRef, useState } from "react";
import {
  initAccount,
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
  const binaryPathRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    initAccount().then(setAccount);
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
    const path = binaryPathRef.current?.value || "";
    if (!path) return alert("Select miner binary path");
    await startMiner(chain === "quantus" ? "resonance" : chain, path, []);
    setMining(true);
  }
  async function onStop() {
    await stopMiner();
    setMining(false);
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
          <option value="heisenberg">Heisenberg (testnet)</option>
          <option value="quantus" disabled>
            Quantus (mainnet – disabled)
          </option>
        </select>

        <label className="ml-6">Miner binary</label>
        <input
          ref={binaryPathRef}
          className="border rounded px-2 py-1 flex-1"
          placeholder="/path/to/miner-binary"
        />

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
        <div className="mb-2">Logs</div>
        <pre className="h-64 overflow-auto text-sm leading-snug bg-black/5 p-2 rounded">
          {logs.join("\n")}
        </pre>
      </div>
    </div>
  );
}
