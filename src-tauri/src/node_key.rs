use anyhow::{anyhow, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Returns the base data directory used by `quantus-node`.
/// Examples:
/// - Linux:   /home/you/.local/share/quantus-node
/// - macOS:   /Users/you/Library/Application Support/quantus-node
/// - Windows: C:\Users\you\AppData\Roaming\quantus-node
pub fn node_base_path() -> Result<PathBuf> {
    let data = dirs::data_dir().ok_or_else(|| anyhow!("no data_dir available"))?;
    Ok(data.join("quantus-node"))
}

/// Maps the UI chain selector to the on-disk chain identifier used in paths.
/// For Resonance the chain id is "resonance".
/// For other chains, we currently pass them through unchanged.
pub fn chain_id_for_ui(chain_ui: &str) -> &str {
    match chain_ui {
        "resonance" => "resonance",
        "heisenberg" => "heisenberg",
        "quantus" => "quantus",
        other => other,
    }
}

/// Computes the node key file path for a given chain id.
/// Path: {base_path}/chains/{chain_id}/network/secret_dilithium
pub fn node_key_file_path(chain_id: &str) -> Result<PathBuf> {
    Ok(node_base_path()?
        .join("chains")
        .join(chain_id)
        .join("network")
        .join("secret_dilithium"))
}

/// Computes the node key file path using a UI chain string ("resonance", etc.).
pub fn node_key_file_path_for_chain_ui(chain_ui: &str) -> Result<PathBuf> {
    node_key_file_path(chain_id_for_ui(chain_ui))
}

/// Ensures the node key exists for the given UI chain, generating it with the provided
/// `quantus-node` binary if missing.
///
/// This runs:
///   quantus-node key generate-node-key --file {base_path}/chains/{chain_id}/network/secret_dilithium
///
/// Returns the absolute path to the node key file.
pub async fn ensure_node_key_for(chain_ui: &str, quantus_node_path: &Path) -> Result<PathBuf> {
    let chain_id = chain_id_for_ui(chain_ui);
    let key_path = node_key_file_path(chain_id)?;
    let parent = key_path
        .parent()
        .ok_or_else(|| anyhow!("invalid key path parent"))?;

    // Ensure parent directories exist
    fs::create_dir_all(parent)
        .with_context(|| format!("creating node key parent dir: {}", parent.display()))?;

    // If the key already exists, nothing to do
    if key_path.exists() {
        return Ok(key_path);
    }

    // Generate a new node key
    let out = tokio::process::Command::new(quantus_node_path)
        .args([
            "key",
            "generate-node-key",
            "--file",
            key_path.to_string_lossy().as_ref(),
        ])
        .output()
        .await
        .with_context(|| "spawning quantus-node to generate node key")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(anyhow!(
            "node key generation failed (status={}): stderr=`{}` stdout=`{}`",
            out.status,
            stderr.trim(),
            stdout.trim()
        ));
    }

    // Double-check the file now exists
    if !key_path.exists() {
        return Err(anyhow!(
            "node key generation reported success but file not found at {}",
            key_path.display()
        ));
    }

    Ok(key_path)
}
