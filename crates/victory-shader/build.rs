//! With the `host` feature, this crate's tests create a real D3D12 device
//! (`emit::host::Host::new`) in the test binary's own process and need
//! shader model 6.6 from it. Same situation and same fix as
//! `apps/victory/build.rs`: export `D3D12SDKVersion`/`D3D12SDKPath` from
//! the test binary and stage the DirectX 12 Agility SDK redistributable
//! next to it, if `cargo xtask agility-sdk` has fetched it. Without the
//! fetch, exports still compile and `cargo test` stays green; the tests
//! that need SM 6.6 just keep skipping on hosts whose OS `d3d12.dll` does
//! not provide it (see `emit::host::Host::new`'s callers).

use std::path::{Path, PathBuf};

fn main() {
    if std::env::var_os("CARGO_FEATURE_HOST").is_none() {
        return;
    }
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKVersion,DATA");
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKPath,DATA");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    // OUT_DIR is target/<profile>/build/victory-shader-<hash>/out; test
    // binaries for this crate land in target/<profile>/deps/.
    let deps_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR has the usual cargo-build depth")
        .join("deps");
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/victory-shader lives two levels under the workspace root");
    let cached = workspace_root.join("target/agility-sdk/x64");

    let dest = deps_dir.join("D3D12");
    for name in ["D3D12Core.dll", "d3d12SDKLayers.dll"] {
        if !cached.join(name).is_file() {
            println!(
                "cargo:warning=Agility SDK not fetched (missing {}); run `cargo xtask agility-sdk` \
                 for shader model 6.6 on hosts whose OS d3d12.dll does not provide it.",
                cached.join(name).display()
            );
            return;
        }
    }
    std::fs::create_dir_all(&dest).expect("creating target/<profile>/deps/D3D12");
    for name in ["D3D12Core.dll", "d3d12SDKLayers.dll"] {
        std::fs::copy(cached.join(name), dest.join(name))
            .unwrap_or_else(|e| panic!("copying {name} next to the test binaries: {e}"));
    }
}
