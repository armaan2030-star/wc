//! Fetches the DirectX 12 Agility SDK redistributable (`D3D12Core.dll`,
//! `d3d12SDKLayers.dll`) into `target/agility-sdk/x64/`, where
//! `apps/victory/build.rs` picks it up and copies it next to the built
//! `victory.exe`.
//!
//! Why this exists: shader model 6.6 (required by this title) is not
//! necessarily available from the OS's in-box `d3d12.dll`. On a Windows 10
//! 22H2 host tested for this fork, `CheckFeatureSupport` capped the
//! hardware adapter at SM 6.5 and WARP at SM 6.2, so every victory-shader
//! GPU test that needs SM 6.6 skipped on both, and `victory-gpu`'s own
//! mesh/world path (same SM 6.6 gate, `crates/victory-gpu/src/host.rs`)
//! would have been gated off at runtime too. The Agility SDK is Microsoft's
//! own redistributable mechanism for exactly this: the app ships a copy of
//! the D3D12 core runtime and exports `D3D12SDKVersion`/`D3D12SDKPath` so
//! `D3D12CreateDevice` uses it instead of the OS's version. It is MIT/
//! Microsoft-redistributable-licensed and meant to be shipped by
//! applications; it is not game data, so it does not go through
//! `cargo xtask policy`, but it is still fetched into `target/` (gitignored)
//! rather than committed, matching this repo's practice of not committing
//! binaries.
//!
//! This step is opt-in and separate from a plain `cargo build` so the tree
//! keeps building green with no network access: run it once, then build.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

/// Microsoft.Direct3D.D3D12 1.620.2 (DirectX 12 Agility SDK), the latest
/// stable (non-preview) release at the time this was pinned (2026-11-24).
const VERSION: &str = "1.620.2";
const URL: &str =
    "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.d3d12/1.620.2/microsoft.direct3d.d3d12.1.620.2.nupkg";
/// SHA-256 of the exact `.nupkg` above, recorded when it was first fetched.
const NUPKG_SHA256: &str = "70489669c2bbeaf318b62d72b0ac3e753d521387bfc848c9763791c4e920bfcc";

/// The SDK version number the headers in this package declare
/// (`D3D12_SDK_VERSION` in `d3d12.h`). `apps/victory/src/main.rs` exports
/// this same number as `D3D12SDKVersion`; keep the two in sync by hand if
/// `VERSION` above is ever bumped.
const SDK_VERSION: u32 = 620;

pub fn run(root: &Path, force: bool) -> Result<()> {
    let dest_dir = root.join("target/agility-sdk/x64");
    let core = dest_dir.join("D3D12Core.dll");
    let layers = dest_dir.join("d3d12SDKLayers.dll");
    if !force && core.is_file() && layers.is_file() {
        println!("already fetched: {}", dest_dir.display());
        return Ok(());
    }

    std::fs::create_dir_all(&dest_dir).context("creating target/agility-sdk/x64")?;
    let nupkg = root.join("target/agility-sdk/_download.nupkg");

    println!("downloading Microsoft.Direct3D.D3D12 {VERSION} (SDK version {SDK_VERSION})...");
    let status = Command::new(r"C:\Windows\System32\curl.exe")
        .args(["-sSL", "--fail", "-o"])
        .arg(&nupkg)
        .arg(URL)
        .status()
        .context("running curl.exe")?;
    ensure!(status.success(), "curl.exe failed to download {URL}");

    let bytes = std::fs::read(&nupkg).context("reading downloaded nupkg")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let got = hex(&hasher.finalize());
    if !got.eq_ignore_ascii_case(NUPKG_SHA256) {
        bail!("sha256 mismatch for {URL}\n  expected {NUPKG_SHA256}\n  got      {got}");
    }
    println!("sha256 verified");

    let status = Command::new(r"C:\Windows\System32\tar.exe")
        .arg("-xf")
        .arg(&nupkg)
        .arg("-C")
        .arg(&dest_dir)
        .args([
            "build/native/bin/x64/D3D12Core.dll",
            "build/native/bin/x64/d3d12SDKLayers.dll",
        ])
        .status()
        .context("running tar.exe to extract the nupkg (it is a zip file)")?;
    ensure!(status.success(), "tar.exe failed to extract the nupkg");

    // tar recreated the nupkg's internal path; move the two DLLs up and
    // drop the now-empty scaffolding.
    let extracted = dest_dir.join("build/native/bin/x64");
    for name in ["D3D12Core.dll", "d3d12SDKLayers.dll"] {
        std::fs::rename(extracted.join(name), dest_dir.join(name))
            .with_context(|| format!("moving {name} into place"))?;
    }
    std::fs::remove_dir_all(dest_dir.join("build")).context("cleaning up extraction scaffold")?;
    std::fs::remove_file(&nupkg).context("removing downloaded nupkg")?;

    println!("fetched: {}", dest_dir.display());
    println!("rebuild victory to pick it up: cargo build --release -p victory");
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}
