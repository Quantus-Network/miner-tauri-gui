use anyhow::Result;
use serde::Serialize;

// Minimal shape to send to UI. Replace with real RPC later.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceView {
    pub address: String,
    pub free: String, // string for simplicity (e.g., "123.456 RES")
}

// Stub: depending on your chain, you might query via WebSocket or an explorer REST.
// For the demo you can leave this returning "0" and wire up later.
pub async fn fetch_balance(_ws_url: &str, address: &str) -> Result<BalanceView> {
    Ok(BalanceView {
        address: address.to_string(),
        free: "0".into(),
    })
}
