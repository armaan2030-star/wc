# Running victory on Linux under Proton

The Linux host runs the tools, the tests and the analysis commands natively.
Actually *running the title* needs a Windows process environment with real
D3D12 — which Proton provides: Wine's loader plus dxvk/vkd3d-proton giving
the GPU stack. So the supported Linux path is: **build `victory.exe` here,
run it inside a Proton prefix.** The title then loads inside that process
exactly as it does on Windows.

Everything below is the full command list for an Arch box; adapt package
names for your distro.

## 1. Build the Windows binary on Linux

```bash
# toolchains
sudo pacman -S --needed rustup mingw-w64-gcc mingw-w64-winpthreads

# the toolchain pinned in rust-toolchain.toml, plus the Windows targets
rustup toolchain install 1.98.1
rustup target add x86_64-pc-windows-gnu --toolchain 1.98.1   # builds and links victory.exe

# build
cargo win        # alias: build -p victory --target x86_64-pc-windows-gnu --release
                 # lands at target/x86_64-pc-windows-gnu/release/victory.exe
```

`cargo win-check` (`check --workspace --all-targets --target
x86_64-pc-windows-gnu`) type-checks the whole Windows tree — tests included
— on the Linux box. A true MSVC-target check needs `clang`/`lld` (or a
Windows box), so the Windows CI job remains that check.

## 2. Run it under Proton

Install a Proton build under Steam's compatibility tools (GE-Proton works
well here), then:

```bash
proton/run.sh config                       # smoke test: prints resolved paths
proton/run.sh --z /mnt/games inspect "Z:\\games\\scripts\\x64\\final.js.c.c.gmpc"
proton/run.sh --z /mnt/games run "Z:\\games\\x64\\final.exe" --execute
```

`proton/run.sh` picks the newest `GE-Proton*` it can find, keeps its prefix
at `$XDG_DATA_HOME/victory/proton` (idempotent), and takes `--prefix`,
`--exe`, `--proton` (a specific Proton dir or plain `wine`), `--z`,
`--config` and `--texture-no-wait`. `--z DIR` links a folder into the
prefix's Z: drive, so a game directory stays wherever it lives while the
config can name it `Z:\games`.

Two environment levers decide whether the GPU path works, and `run.sh`
sets the first for you:

- **Native d3d12/dxgi.** Wine's built-in `d3d12` stub answers
  `E_INVALIDARG` and draws nothing; the GPU path needs the dxvk and
  vkd3d-proton copies Proton syncs into the prefix. Recent Proton forces
  this itself; `run.sh` defaults `WINEDLLOVERRIDES=d3d12=n;d3d12core=n;dxgi=n`
  when you haven't set it, so a hand-made prefix behaves the same. (The
  same requirement, with RADV frame measurements, is in the upstream
  Linux/Proton notes in fxceless/victory-fork#41.)
- **A Win8.1/10-era native `d3dcompiler_47`.** Do not trust `winetricks
  d3dcompiler_47`: it installs the fixed June-2010 redistributable, which
  predates D3D12 — it compiles `[RootSignature(...)]` sources without
  complaint but embeds no root signature, and `CreateRootSignature` then
  fails with a bare `E_FAIL`. Copy `d3dcompiler_47.dll` from your own
  Windows 10/11 `C:\Windows\System32` into the prefix's `system32` (it is
  your data: the asset policy keeps it out of the repo either way) and
  keep the `d3dcompiler_47=n` override.

## 3. The config for a Proton run

`victory.toml` is read from the process' working directory, and Wine
resolves the Unix cwd through its own Z: mapping — so relative config names
work on both sides. Windows paths inside the file, however, must be Windows
paths. Keep a second, git-ignored config:

```bash
cp victory.example.toml victory.proton.toml
```

```toml
[paths]
game_dir = 'Z:\games'
# cache_dir/data_dir default to the prefix's LOCALAPPDATA, which is fine;
# or point them at Z:\somewhere for easy browsing from the Linux side.
```

and pass `--config victory.proton.toml`. The asset-policy rules are
unchanged: data lives outside the repo, and `cargo xtask policy` still
insists on it.

## 4. What works, what is Wine-dependent

Verified 2026-11-24 on GE-Proton11-11 (Wine 11.0 staging, dxvk/vkd3d-proton
synced into the prefix as native DLLs, `ntsync` kernel driver), with a
Win10-era native `d3dcompiler_47` as above:

- `proton/smoke.sh` passes: `config` resolves and prints the `host Wine`
  banner, the title directory is reachable through the prefix, and `census`
  runs against it.
- The whole `victory-gpu` test suite runs as a Windows binary under the same
  prefix: **109 of 111 pass**, covering device creation, the newer device
  levels, fences, heaps, copies, queue submission and the indirect-argument
  clamp — i.e. Proton really does provide the D3D12 ground victory hosts on.
  The one failure:
  - `pipeline::library::tests::a_rejected_blob_gives_an_empty_library`:
    vkd3d-proton errors on `OpenPipelineLibrary` of a garbage blob where
    real D3D12 hands back an empty library. A platform expectation, not a
    regression — it fails the same on plain `main` (fxceless/victory-fork#64).

  With the June-2010 winetricks `d3dcompiler_47` instead, four fail: the
  three that compile `*_5_1` inline shaders (the 2010 build refuses the
  5_1 profile — `X3506` — and even at 5_0 embeds no root signature), plus
  the library test. `crate::host::compile` retries 5_1 at 5_0 for compilers
  too old for the 5_1 profile, which helps Win8-era copies; it cannot rescue
  the 2010 one, so get the Win10-era DLL.

Known gaps and unverified areas (file an issue if you hit one):

- **Shader validation**: `DxcCreateInstance` needs dxcompiler — Wine has no
  DXC. Translation output skips validation as it does on Windows without
  `dxil.dll`; treat translation results under Wine as unvalidated.
- **Diagnostics build** (`--features diagnostics`, `--diagnostics-impls`):
  the write-watch queries and the debug layer have had no Wine testing.
- **Input/audio**: hle's gamepad and audio paths were built against Windows
  behaviour; under Wine they ride whatever Wine presents. Unverified.
- **HDR**: with HDR on, vkd3d-proton present throttles to ~400 ms a frame
  and the 30 s watchdog ends a run within the first minute — keep HDR off
  on the display for Proton runs (#41 measures the same on RADV). 10-bit
  output remains untested.
- A full `victory run` under Proton has been shown possible on AMD/RADV:
  menus, intro and world streaming at 1 800–5 300 draws a frame with the
  two levers above (#41). It has not been run on this box yet.
- **Texture-lane waits**: under Wine the texture lane's remap-lock wait is
  a mapping lock that costs up to seconds per window; `VICTORY_TEXTURE_NO_WAIT=1`
  (flag `--texture-no-wait` in `run.sh`) takes it to zero at the cost of up
  to two textures a frame deferred to the next one — measurements in
  fxceless/victory-fork#178.

## 5. Tests and CI

The Ubuntu job runs the native Linux test suite; the Windows job runs the
same suite plus clippy. `cargo test -p xtask` checks the launch scripts
parse and their usage text holds; `proton/smoke.sh` is the Proton-path
smoke test — run it after touching `proton/`, and paste its output into
bug reports.
