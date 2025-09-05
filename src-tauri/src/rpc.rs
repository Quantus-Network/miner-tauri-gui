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

#[derive(Debug, Clone, Serialize)]
pub struct BalanceView {
    pub address: String,
    pub free: String,   // raw string value (chain units, e.g., plancks)
    pub symbol: String, // e.g., "RES"
    pub decimals: u32,  // e.g., 12
}

// Structure used to decode system_properties
#[derive(Debug, Deserialize)]
struct SystemProperties {
    #[serde(default, rename = "tokenSymbol")]
    token_symbol: Option<serde_json::Value>, // may be string or array
    #[serde(default, rename = "tokenDecimals")]
    token_decimals: Option<serde_json::Value>, // may be number or array
}

// Safe extractors for potential string/array forms
fn extract_symbol(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(arr) => {
            arr.first().and_then(|x| x.as_str()).map(|s| s.to_string())
        }
        _ => None,
    }
}
fn extract_decimals(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().map(|x| x as u32),
        serde_json::Value::Array(arr) => arr.first().and_then(|x| x.as_u64()).map(|x| x as u32),
        _ => None,
    }
}

/// Query local RPC for system properties (symbol/decimals)
async fn fetch_local_chain_properties() -> (String, u32) {
    #[derive(Deserialize)]
    struct RpcResp {
        result: Option<serde_json::Value>,
    }

    let client = match reqwest::Client::builder()
        .user_agent("quantus-miner/0.1")
        .build()
    {
        Ok(c) => c,
        Err(_) => return ("".into(), 12),
    };

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "system_properties",
        "params": []
    });

    if let Ok(resp) = client
        .post(local_ws_endpoint().replace("ws://", "http://"))
        .json(&body)
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        if let Ok(r) = resp.json::<RpcResp>().await {
            if let Some(result) = r.result {
                let props: SystemProperties =
                    serde_json::from_value(result).unwrap_or(SystemProperties {
                        token_symbol: None,
                        token_decimals: None,
                    });
                let symbol = props
                    .token_symbol
                    .as_ref()
                    .and_then(extract_symbol)
                    .unwrap_or_else(|| "RES".to_string());
                let decimals = props
                    .token_decimals
                    .as_ref()
                    .and_then(extract_decimals)
                    .unwrap_or(12);
                return (symbol, decimals);
            }
        }
    }

    // Fallback defaults
    ("RES".into(), 12)
}

/// Fetch balance using network-specific strategy.
/// For Resonance testnet we use Subsquid GraphQL.
/// For other chains we return "0" until endpoints exist.
pub async fn fetch_balance(ws_url: &str, address: &str) -> Result<BalanceView> {
    let (symbol, decimals) = fetch_local_chain_properties().await;

    // Resonance-only: use Subsquid GraphQL (https://gql.res.fm/graphql)
    if ws_url.contains("res.fm") {
        #[derive(Deserialize)]
        struct AccountById {
            free: Option<String>,
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
            symbol,
            decimals,
        });
    }

    // Fallback for other chains (heisenberg/mainnet TBD)
    Ok(BalanceView {
        address: address.to_string(),
        free: "0".into(),
        symbol,
        decimals,
    })
}
