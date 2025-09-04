# Quantus Miner – Agents Guide (for contributors and future AIs)

This document captures the operational context and the pragmatic conventions we’ve learned while building and iterating on this PoW Substrate-derived miner with post-quantum keys. It exists to save future humans (and agents) time and prevent regressions that stem from assuming “typical Substrate” behavior.

## TL;DR

- These networks are Substrate-derived but:
  - Proof of Work (not BABE/GRANDPA/PoS).
  - Post-quantum keys (ML-DSA-87). Key sizes differ; do not assume standard SCALE decoding heuristics.
  - Finality trails best by hundreds of blocks — do not rely on finalized height for UX progress.
  - Inter-block times vary widely: milliseconds to minutes. Avoid short timeouts, frequent reconnects, or “idle = error” assumptions.

- Height and progress:
  - Display progress as best (local) / highest (bootnode).
  - Maintain a persistent bootnode WebSocket subscription (chain_subscribeNewHeads) with tolerant timeouts; reconnect only on errors.
  - If subscription yields nothing for a while, fall back to system_syncState or chain_getFinalizedHead → chain_getHeader to obtain a numeric tip — but do not message “finalized” semantics in UI.
  - Track bootnode staleness and surface it in the UI (e.g., “(stale)” or tooltip with last head age).

- Safe sync automation:
  - Heavy ranges (e.g., Resonance 13311–13360) can cause bans if blocks are re-requested during lengthy transfer.
  - When local sync enters such a range, restart with `--max-blocks-per-request 1`.
  - When well past the range (with margin), restart to remove that arg for speed.
  - Use a pending-toggle flag from the log reader, and perform restarts in a Send-safe task (avoid non-Send futures in spawned tasks).
  - Debounce transitions and use a safety margin to prevent flapping.

- Logging and state:
  - Emit `miner:log`, `miner:status`, `miner:state`, `miner:meta`.
  - Always emit status snapshots—even if nothing changed—so UI and agents remain synchronized (especially with high-latency heads).
  - Optional per-run log file path with PID and RFC3339 timestamp in user data dir.

- OS behavior:
  - On desktop, use class-based Tailwind dark mode and keep styles minimal but legible.
  - Use the opener plugin to reveal paths (account JSON, log files).

---

## Architectural overview

Frontend (Vite + React):
- Presents:
  - Status badge (Starting / Syncing / Mining / Repairing / Error)
  - Peers / Best / Highest
  - Balance pill
  - Safe Sync badge (when active)
  - Bootnode connection pill (host, stale indicator)
  - Two-column layout with:
    - Node Info (short) + System details (collapsible)
    - Chain + binary + account JSON + planned command + Start/Stop/Resync/Unlock buttons
  - Console (tail, structured logs)
- Persists:
  - Chain selection (`qm.chain`)
  - Auto-start, line limit, last logs, miner meta, log file path, theme, “was mining” flag

Backend (Tauri + Rust):
- Commands:
  - Start/Stop, ensure miner + account, repair (resync), unlock, get/set safe ranges
- Status/event streams:
  - `miner:state` — running/starting/stopped
  - `miner:status` — peers, best (local), highest (bootnode), is_syncing, safe_mode, bootnode connection + staleness
  - `miner:log` — lines for console and file
  - `miner:meta` — parsed startup details (version, chain spec, role, database path, rpc endpoints, pq info)
- Long-running tasks:
  - Local WS (127.0.0.1:9944): subscribe new heads for best; periodic system_health for peers/isSyncing
  - Bootnode WS (persistent): subscribe new heads for highest, with non-blocking poll (short read timeout per loop), reconnect only on error; fallback to RPC methods when needed

---

## Post-quantum specifics

- PQ keys (ML-DSA-87) impact:
  - Key lengths are significantly larger than ed25519/ecdsa; avoid assumptions about SCALE layouts derived from typical lengths.
  - Keep RPC handling string/hex only (no SCALE parsing in the miner).
- Account JSON:
  - Save and present a “rewards address” for mining payouts.
  - Provide Copy/Open actions to aid user verification.

---

## Syncing and height strategy

- Local best:
  - Subscribe to 127.0.0.1:9944 `chain_subscribeNewHeads`.
  - Treat connection idles as normal. Reconnect on explicit error.
  - Use `system_health` to populate peers/isSyncing.
- Bootnode highest:
  - Maintain a persistent WebSocket subscription to `chain_subscribeNewHeads`.
  - Do not assume a head every few seconds; the network may be quiet for minutes.
  - If no heads for a while, fallback to:
    - `system_syncState` (highestBlock), or
    - `chain_getFinalizedHead` → `chain_getHeader` to get a numeric tip (presented as “highest”, not “finalized”).
  - Track `bootnode_stale_secs` and surface in UI.

- Progress:
  - Present `Syncing #best / #highest (XX%)`.
  - Do not surface finalized height (it trails best by hundreds; not meaningful here).

---

## Safe sync automation (anti-ban workaround)

- Problem:
  - Nodes may be banned by peers when repeatedly requesting a large block still being transferred.
  - Observed on Resonance in 13311–13360 (capacity-fill performance test).
- Solution:
  - Watch stderr logs for `"Importing block #N"`.
  - Entering range → schedule safe mode enable (`--max-blocks-per-request 1`), stop & restart.
  - Past range end + safety margin → schedule safe mode disable, stop & restart.
  - Implementation details:
    - The stderr reader sets a pending flag (`SAFE_MODE_PENDING`) rather than restarting directly (to keep the future Send).
    - The status task consumes the flag and executes restart with updated `extra_args`.
    - Use a safety margin and “in-range only” triggers to avoid flapping.

---

## Resync and unlock flows

- Resync:
  - Stops the node, deletes `{base}/chains/{chain}/db/full`, and restarts from genesis.
  - The UI shows a confirmation explaining this resets state and takes time.
  - Emits `miner:state` “stopped” immediately to flip buttons.

- Unlock:
  - Stops the node, removes `{base}/chains/{chain}/db/full/LOCK`, and restarts.
  - Used when we see “Resource temporarily unavailable” lock errors.
  - Also emits `miner:state` “stopped” immediately.

---

## Events & UI contract

- `miner:state`:
  - `{ running: false, phase: "starting" }` when start() begins (before stop).
  - `{ running: true, phase: "running" }` after process spawns.
  - `{ running: false, phase: "stopped" }` emitted by callers (stop_miner/repair/unlock/safe-mode toggles) before stop, so the UI flips buttons promptly.
- `miner:status` (emitted every loop even if fields unchanged):
  - `{ peers, current_block: best, highest_block: highest, is_syncing, safe_mode, bootnode_connected, bootnode_host, bootnode_stale_secs }`
- `miner:log`:
  - `{ source, line }` — currently UI displays raw line (no prefixes) to maximize width.
  - File logging writes each line as-is.
- `miner:meta`:
  - Contains parsed startup details (version, chain spec, node name, role, db path, local identity, rpc addresses) and the run context (binary, chain, rewards address).

---

## Paths and storage

- Node base path:
  - `{data_dir}/quantus-node` (platform-specific)
- Miner app data:
  - `{app_data_dir}/safe_ranges.json` — optional override for safe ranges (per-chain)
  - `{local_data_dir}/quantus-miner/logs/miner-<pid>-<timestamp>.log` — optional file logs
- Account JSON:
  - `{app_data_dir}/mining-rewards-account.json` — Copy/Open from UI

---

## UI/UX conventions

- Header pills:
  - Status: Starting / Syncing / Mining / Repairing / Error
  - Safe Sync: purple badge when `--max-blocks-per-request 1` is active
  - Bootnode connection: “Connected: host” (or “offline”), tooltip shows last head age
  - Peers / Best / Highest: red/amber/green thresholds on peers
  - Balance: optional pill when known
- Console:
  - Max height ~30vh; line limit adjustable; Clear/Export enabled.
  - Structured events flow to console as plain lines.

---

## Timeouts and reliability

- Do not treat lack of heads as failure; only errors on the WS stream require reconnect.
- Use long/tolerant timeouts (minutes) for read idles; prefer a short poll loop with a 1s read timeout to remain responsive to other duties while keeping the connection open.
- Always emit status snapshots; agents and UI shouldn’t gate on change detection.

---

## Extensibility suggestions (post-demo)

- Settings UI:
  - Manage per-chain safe ranges and safety margins.
  - Configure bootnode endpoint per chain and view connection diagnostics.
  - Toggle persistent logging and log rotation policy.
- Diagnostics:
  - Add a debug panel to show last N JSON RPC messages from bootnode and local node.
  - Expose “stale since” counters for both best and highest.
- Node info:
  - Buttons to reveal DB folder, logs folder, and RPC endpoints.
- Robust restart policy:
  - Backoff on repeated bootnode connect failures and display an inline warning with remediation steps.

---

## Troubleshooting checklist

- No Highest displayed:
  - Check “Connected: …” pill; if offline, look for “Bootnode connect failed: …” in console.
  - Verify rustls roots are available; ensure TLS features are compiled.
  - The endpoint may be quiet (no heads for minutes). Fallbacks should eventually populate Highest — enable debug logging if not.
- Repeated restart loops near heavy ranges:
  - Ensure safe-mode margin is large enough; only enable when actually *in* range and disable only when past *all* ranges + margin.
- White screen / blank UI:
  - Check for type or import errors (e.g., plugin APIs). The app imports Tailwind CSS + minimal App.css; ensure both load.

---

## Code reading map

- Frontend:
  - `src/App.tsx` — main UI (pills, controls, console, Node Info)
  - `src/api.ts` — Tauri invoke + event subscriptions
  - `src/styles/tailwind.css` — Tailwind base/components/utilities
- Backend:
  - `src-tauri/src/miner.rs` — process lifecycle, status loops, safe mode, file logs
  - `src-tauri/src/commands.rs` — Tauri commands and UI-to-backend mapping
  - `src-tauri/src/installer.rs` — binary install/update
  - `src-tauri/src/account_cli.rs` — rewards account generation (via CLI)
  - `src-tauri/src/account_path.rs` — account JSON path helper
  - `src-tauri/src/parse.rs` — lightweight miner event parsing
  - `src-tauri/tauri.conf.json` — window and bundling config

---

## Non-goals (for this phase)

- Relying on finalized height for UX — not representative here (hundreds of blocks behind).
- SCALE decoding of PQ payloads — use RPC JSON only.
- Complex on-chain metrics (peers detail, network graph) — can be added after demo.

---

## Conventions and guardrails

- Always assume heads may be sparse; avoid assumptions about “freshness” without staleness indicators.
- Minimize blocking reads; prefer short poll slices (1s) on persistent streams.
- Emit status snapshots every loop; UIs and agents should react to snapshots, not only deltas.
- When restarting the node for any reason, emit `miner:state` “stopped” first so the UI flips buttons immediately.

Stay pragmatic, ship demo-friendly defaults, and keep the behavior defensive against long idle periods and large blocks.
