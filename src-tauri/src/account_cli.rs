use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountJson {
    pub address: String, // ss58 (qzo…)
    pub secret_phrase: Option<String>,
    pub seed: Option<String>,
    pub pub_key: Option<String>,
}

impl AccountJson {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let txt = fs::read_to_string(path)?;
        let acct: AccountJson = serde_json::from_str(&txt)?;
        Ok(acct)
    }
}

pub async fn ensure_account_json(
    _app: &AppHandle,
    quantus_node_path: &PathBuf,
    out_path: &PathBuf,
) -> Result<AccountJson> {
    if out_path.exists() {
        // accept existing file if it has address/ss58
        let txt = fs::read_to_string(out_path)?;
        if let Ok(a) = serde_json::from_str::<AccountJson>(&txt) {
            if !a.address.is_empty() {
                return Ok(a);
            }
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(addr) = v
                .get("address")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("ss58").and_then(|x| x.as_str()))
            {
                return Ok(AccountJson {
                    address: addr.to_string(),
                    secret_phrase: None,
                    seed: None,
                    pub_key: None,
                });
            }
        }
    }

    // generate new one via CLI
    let out = tokio::process::Command::new(quantus_node_path)
        .args(["key", "quantus"])
        .output()
        .await?;
    if !out.status.success() {
        return Err(anyhow!(
            "keygen failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);

    // extract the block between the two X-lines
    let re_block =
        Regex::new(r"X{15,}\s*Quantus Account Details\s*X{15,}\s*(?P<body>[\s\S]*?)\s*X{15,}")
            .unwrap();
    let body = re_block
        .captures(&stdout)
        .ok_or_else(|| anyhow!("couldn't find Quantus Account Details block"))?
        .name("body")
        .unwrap()
        .as_str();

    let address = capture(body, r"Address:\s*([^\s]+)")?;
    let secret_phrase = capture_opt(body, r"Secret phrase:\s*(.+)");
    let seed = capture_opt(body, r"Seed:\s*([0-9a-fx]+)");
    let pub_key = capture_opt(body, r"Pub key:\s*([0-9a-fx]+)");

    let acct = AccountJson {
        address,
        secret_phrase,
        seed,
        pub_key,
    };
    fs::write(out_path, serde_json::to_vec_pretty(&acct)?)?;
    Ok(acct)
}

fn capture(s: &str, pat: &str) -> Result<String> {
    let re = Regex::new(pat).unwrap();
    let c = re
        .captures(s)
        .ok_or_else(|| anyhow!("missing field: {pat}"))?;
    Ok(c.get(1).unwrap().as_str().trim().to_string())
}
fn capture_opt(s: &str, pat: &str) -> Option<String> {
    let re = Regex::new(pat).ok()?;
    let c = re.captures(s)?;
    Some(c.get(1)?.as_str().trim().to_string())
}
