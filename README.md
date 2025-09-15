# Quantus Miner

This repository contains a Tauri + React + TypeScript desktop application that wraps a PoW Substrate-derived node/miner with post-quantum keys.

- For contributor and agent notes, see the Agents Guide: [agents.md](./agents.md)

## Running from Source

Developer quickstart:
- From the repo root: `cd miner`
- Install dependencies: `pnpm install`
- Run the Tauri app in dev mode: `pnpm run tauri dev`

Notes:
- The Tauri config runs `pnpm dev` before launching and serves the frontend at http://localhost:1420 during development.
- You need a working Rust toolchain and the Tauri CLI v2. If needed, add the CLI with: `pnpm add -D @tauri-apps/cli`
- Alternatively, you can use the Rust shim: `cargo tauri dev` (from `miner`) if the CLI is installed.
- To build a release bundle: `pnpm run tauri build`
