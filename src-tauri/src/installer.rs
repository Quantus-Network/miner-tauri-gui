use anyhow::{anyhow, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;

pub fn user_bin_dir() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let d = dirs::home_dir()
            .ok_or_else(|| anyhow!("no home"))?
            .join(".local/bin");
        fs::create_dir_all(&d)?;
        Ok(d)
    }
    #[cfg(target_os = "macos")]
    {
        let d = dirs::home_dir()
            .ok_or_else(|| anyhow!("no home"))?
            .join("bin");
        fs::create_dir_all(&d)?;
        Ok(d)
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("LOCALAPPDATA missing"))?;
        let d = base.join("Programs").join("Quantus").join("bin");
        fs::create_dir_all(&d)?;
        Ok(d)
    }
}

use serde::Deserialize;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Debug)]
pub struct ExternalMinerConfig {
    pub num_cores: usize,
    pub port: u16,
}

#[derive(Debug)]
pub struct ExternalMinerHandle {
    pub binary_path: PathBuf,
    pub port: u16,
    pub num_cores: usize,
    pub child: tokio::process::Child,
}

#[derive(Debug)]
struct Target {
    os_tag: &'static str,
    arch_tag: &'static str,
    ext: &'static str,
}
#[cfg(target_os = "linux")]
fn target() -> Target {
    Target {
        os_tag: "unknown-linux-gnu",
        arch_tag: "x86_64",
        ext: ".tar.gz",
    }
}
#[cfg(target_os = "macos")]
fn target() -> Target {
    if cfg!(target_arch = "aarch64") {
        Target {
            os_tag: "apple-darwin",
            arch_tag: "aarch64",
            ext: ".tar.gz",
        }
    } else {
        Target {
            os_tag: "apple-darwin",
            arch_tag: "x86_64",
            ext: ".tar.gz",
        }
    }
}
#[cfg(target_os = "windows")]
fn target() -> Target {
    Target {
        os_tag: "pc-windows-msvc",
        arch_tag: "x86_64",
        ext: ".zip",
    }
}

pub async fn ensure_quantus_node_installed() -> Result<PathBuf> {
    let bin_dir = user_bin_dir()?;
    let dest = bin_dir.join(exe_name());
    if dest.exists() {
        return Ok(dest);
    }

    let client = reqwest::Client::builder()
        .user_agent("quantus-miner/0.1")
        .build()?;
    let rel: Release = client
        .get("https://api.github.com/repos/Quantus-Network/chain/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tgt = target();
    let wanted_prefix = format!(
        "quantus-node-{}-{}-{}",
        rel.tag_name, tgt.arch_tag, tgt.os_tag
    );
    let asset = rel
        .assets
        .iter()
        .find(|a| a.name.starts_with(&wanted_prefix) && a.name.ends_with(tgt.ext))
        .ok_or_else(|| anyhow!("no asset for target: {wanted_prefix}{}", tgt.ext))?;

    let tmp = tempfile::Builder::new().prefix("quantus-node-").tempdir()?;
    let archive_path = tmp.path().join(&asset.name);

    let mut resp = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?;
    let mut file = tokio::fs::File::create(&archive_path).await?;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    if tgt.ext == ".tar.gz" {
        extract_tar_gz(&archive_path, &bin_dir)?;
    } else {
        extract_zip(&archive_path, &bin_dir)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&dest) {
            let mut p = meta.permissions();
            p.set_mode(0o755);
            let _ = fs::set_permissions(&dest, p);
        }
    }

    Ok(dest)
}

/// External miner

fn miner_exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "quantus-miner.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "quantus-miner"
    }
}

fn miner_target() -> Target {
    #[cfg(target_os = "linux")]
    {
        Target {
            os_tag: "unknown-linux-gnu",
            arch_tag: "x86_64",
            ext: ".tar.gz",
        }
    }
    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            Target {
                os_tag: "apple-darwin",
                arch_tag: "aarch64",
                ext: ".tar.gz",
            }
        } else {
            Target {
                os_tag: "apple-darwin",
                arch_tag: "x86_64",
                ext: ".tar.gz",
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        Target {
            os_tag: "pc-windows-msvc",
            arch_tag: "x86_64",
            ext: ".zip",
        }
    }
}

/// Ensure external parallel miner is installed (downloads from GitHub releases)
pub async fn ensure_external_miner_installed() -> Result<PathBuf> {
    let bin_dir = user_bin_dir()?;
    let dest = bin_dir.join(miner_exe_name());
    if dest.exists() {
        return Ok(dest);
    }

    let client = reqwest::Client::builder()
        .user_agent("quantus-miner/0.1")
        .build()?;
    // fetch latest release (same mechanism as quantus-node)
    let rel: Release = client
        .get("https://api.github.com/repos/Quantus-Network/quantus-miner/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Current release assets are plain binaries named like:
    //  - quantus-miner-linux-x86_64
    //  - quantus-miner-macos-aarch64
    //  - quantus-miner-windows-x86_64.exe
    // So we match by platform-friendly substrings instead of archived names.
    #[cfg(target_os = "linux")]
    let want_os = "linux";
    #[cfg(target_os = "macos")]
    let want_os = "macos";
    #[cfg(target_os = "windows")]
    let want_os = "windows";

    #[cfg(target_arch = "x86_64")]
    let want_arch = "x86_64";
    #[cfg(target_arch = "aarch64")]
    let want_arch = "aarch64";

    let is_windows = cfg!(target_os = "windows");

    let name_matches = |n: &str| {
        let nl = n.to_lowercase();
        nl.starts_with("quantus-miner")
            && nl.contains(want_os)
            && nl.contains(want_arch)
            && (!is_windows || nl.ends_with(".exe"))
            && (is_windows || !nl.ends_with(".exe"))
    };

    let asset = rel
        .assets
        .iter()
        .find(|a| name_matches(&a.name))
        .ok_or_else(|| {
            anyhow!(
                "no external miner asset for target (os={}, arch={}): available={:?}",
                want_os,
                want_arch,
                rel.assets.iter().map(|a| &a.name).collect::<Vec<_>>()
            )
        })?;

    // Download to a temp path
    let tmp = tempfile::Builder::new()
        .prefix("quantus-miner-")
        .tempdir()?;
    let download_path = tmp.path().join(&asset.name);

    let mut resp = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?;
    let mut file = tokio::fs::File::create(&download_path).await?;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    // If the asset is a plain binary, move it into place and make it executable.
    // If it's ever distributed as an archive again, extend this logic accordingly.
    // For now, releases list plain binaries; just place them as miner_exe_name().
    let final_dest = bin_dir.join(miner_exe_name());
    // On Windows, keep .exe; on others, remove any suffix and rename to the expected name
    std::fs::copy(&download_path, &final_dest)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&dest) {
            let mut p = meta.permissions();
            p.set_mode(0o755);
            let _ = fs::set_permissions(&dest, p);
        }
    }

    Ok(dest)
}

/// Spawn the external miner with provided config and return a handle
pub async fn spawn_external_miner(cfg: ExternalMinerConfig) -> Result<ExternalMinerHandle> {
    let bin = ensure_external_miner_installed().await?;
    let mut args: Vec<String> = vec![
        "--num-cores".into(),
        cfg.num_cores.to_string(),
        "--port".into(),
        cfg.port.to_string(),
    ];

    let mut cmd = Command::new(&bin);
    // Ensure the external miner emits logs
    cmd.env("RUST_LOG", "info")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn()?;

    Ok(ExternalMinerHandle {
        binary_path: bin,
        port: cfg.port,
        num_cores: cfg.num_cores,
        child,
    })
}

fn exe_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "quantus-node.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "quantus-node"
    }
}

fn extract_tar_gz(archive: &Path, out_dir: &Path) -> Result<()> {
    let f = fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    ar.unpack(out_dir)?;
    Ok(())
}
fn extract_zip(archive: &Path, out_dir: &Path) -> Result<()> {
    let f = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(f)?;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i)?;
        let out = out_dir.join(e.name());
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut of = std::fs::File::create(&out)?;
            std::io::copy(&mut e, &mut of)?;
        }
    }
    Ok(())
}
