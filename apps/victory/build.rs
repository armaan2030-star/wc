//! Exports `D3D12SDKVersion`/`D3D12SDKPath` from `victory.exe` (see
//! `src/main.rs`) so `D3D12CreateDevice` can pick up a redistributed DirectX
//! 12 Agility SDK core instead of whatever shader model the OS's own
//! `d3d12.dll` happens to support. Copies the SDK's two DLLs next to the
//! built exe if `cargo xtask agility-sdk` has fetched them into
//! `target/agility-sdk/x64/`; if it has not, the exports still compile and
//! build but `D3D12CreateDevice` falls back to the OS's in-box runtime, as
//! it already did before this file existed. A plain `cargo build` with no
//! network access stays green either way.

use std::path::{Path, PathBuf};

fn main() {
    // DATA exports from an EXE need an explicit linker directive; a plain
    // `#[no_mangle] pub static` is not enough to put them in the export
    // table the way it would be for a cdylib.
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKVersion,DATA");
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKPath,DATA");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    // OUT_DIR is target/<profile>/build/victory-<hash>/out; the exe lands
    // three levels up, at target/<profile>/.
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR has the usual cargo-build depth")
        .to_path_buf();
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/victory lives two levels under the workspace root");
    let cached = workspace_root.join("target/agility-sdk/x64");

    let dest = profile_dir.join("D3D12");
    for name in ["D3D12Core.dll", "d3d12SDKLayers.dll"] {
        let src = cached.join(name);
        if !src.is_file() {
            println!(
                "cargo:warning=Agility SDK not fetched (missing {}); run `cargo xtask agility-sdk` \
                 for shader model 6.6 on hosts whose OS d3d12.dll does not provide it. \
                 Falling back to the OS runtime for now.",
                src.display()
            );
            return;
        }
    }
    std::fs::create_dir_all(&dest).expect("creating target/<profile>/D3D12");
    for name in ["D3D12Core.dll", "d3d12SDKLayers.dll"] {
        std::fs::copy(cached.join(name), dest.join(name))
            .unwrap_or_else(|e| panic!("copying {name} next to victory.exe: {e}"));
    }
    println!("cargo:warning=Agility SDK staged at {}", dest.display());
}
