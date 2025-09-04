use anyhow::{anyhow, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

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
