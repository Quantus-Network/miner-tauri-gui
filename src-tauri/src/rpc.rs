use anyhow::Result;
use serde::{Deserialize, Serialize};

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
pub async fn fetch_balance(ws_url: &str, address: &str) -> Result<BalanceView> {
    // Resonance-only: use Subsquid GraphQL (https://gql.res.fm/graphql)
    if ws_url.contains("res.fm") {
        #[derive(Deserialize)]
        struct AccountById {
            free: Option<String>,
            reserved: Option<String>,
        }
        #[derive(Deserialize)]
        struct Data {
            #[serde(rename = "accountById")]
            account_by_id: Option<AccountById>,
        }
        #[derive(Deserialize)]
        struct GraphQLResponse {
            data: Option<Data>,
        }

        let client = reqwest::Client::builder()
            .user_agent("quantus-miner/0.1")
            .build()?;

        let query = r#"query Account($accountId: String!){ accountById(id: $accountId){ id free reserved } }"#;
        let body = serde_json::json!({
            "query": query,
            "variables": { "accountId": address }
        });

        let resp: GraphQLResponse = client
            .post("https://gql.res.fm/graphql")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let free = resp
            .data
            .and_then(|d| d.account_by_id)
            .and_then(|a| a.free)
            .unwrap_or_else(|| "0".to_string());

        return Ok(BalanceView {
            address: address.to_string(),
            free,
        });
    }

    // Fallback for other chains (heisenberg/mainnet TBD)
    Ok(BalanceView {
        address: address.to_string(),
        free: "0".into(),
    })
}
