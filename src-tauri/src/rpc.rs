use anyhow::Result;
use serde::Serialize;

/// Central place to resolve chain endpoints used across the app.
pub fn bootnode_ws_for_chain(chain: &str) -> Option<&'static str> {
    match chain {
        // testnets
        "resonance" => Some("wss://a.t.res.fm"),
        "heisenberg" => Some("wss://a.i.res.fm"),
        // mainnet (placeholder – disabled in UI for now)
        "quantus" => None,
        _ => None,
    }
}

/// Local node JSON-RPC endpoint (substrate default).
pub fn local_ws_endpoint() -> &'static str {
    "ws://127.0.0.1:9944"
}

// Minimal shape to send to UI. Replace with real RPC later.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceView {
    pub address: String,
    pub free: String, // string for simplicity (e.g., "123.456 RES")
}

/// Stub balance fetcher.
/// For now, the frontend passes a WS URL. Callers should prefer using
/// `bootnode_ws_for_chain(chain)` and pass the returned URL when available.
pub async fn fetch_balance(_ws_url: &str, address: &str) -> Result<BalanceView> {
    Ok(BalanceView {
        address: address.to_string(),
        free: "0".into(),
    })
}
