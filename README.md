# VIctory fork (work in progress)

A work-in-progress fork of [VIctory](https://github.com/Marvisak/VIctory), a
compatibility layer that runs an Xbox Series title natively on Windows from
files the user supplies. This fork follows upstream
`main` and adds fixes aimed at one goal: getting the game's world to render
correctly and playably on one particular PC.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here: the loader, the runtime
  services, the Direct3D 12 back end and the shader recompiler. Licensed
  GPL-3.0-or-later; so is this fork. Upstream's history is kept intact below
  the single commit that carries this fork's changes.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors
  (GPL-2.0-only) was used as a reference for how the same title behaves under
  another layer. Fixes inspired by it were written anew in VIctory's code.
- This fork's changes were developed with [Claude Code](https://claude.com/claude-code).

## Status of this fork (2026-11-25)

**Not playable yet: 2.5 to 4 frames per second, and shadowed areas are black.**
Tested on one machine: Windows 11, Intel i7-14700KF, NVIDIA RTX 4070 12 GB,
32 GB RAM. The Series X profile is used throughout.

| Stage | State in this fork |
|-------|--------------------|
| Boot, menus, start screen | works, 30 fps (as upstream) |
| Opening cutscene | plays from start to finish, in order, lit: title cards, credits cards, night and dusk shots |
| Saves | the game autosaves a checkpoint and loads it again |
| First gameplay scene (the prologue) | renders: characters, textures, sky, sunlit exterior; HUD, pause menu and pad input work |
| Light in shadow | missing: the game's ray-traced bounce light and ray-traced lamp and sun shadows do not run yet, so anything the sun does not hit directly is black |
| Frame rate in the world | 2-4 fps: 420-450 ms per frame by default, 240-350 ms with the newer switches (`VICTORY_RELAY=2`, or `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024`); it started at 3-6 seconds per frame |
| Stability | no driver hang or device loss in normal runs (a hang introduced by the wave64 update was found and fixed: its barrier form for 64-lane programs is now off unless `VICTORY_WAVE_WIDE=1`) |

### What this fork changed (summary)

Rendering:
- Meshlet culling read its scene buffers as the wrong kind of view, so the world drew almost nothing; fixed.
- Per-draw descriptor tables could run out, after which draws used slots never made for them and the card hung; fixed.
- Light packed by the game's compute shaders was read back through empty copies (sky, probes, fog); fixed for every mip level and layer.
- The fog reconstruction, shadow-mask, hair, blend-shape skinning, fire and several material programs did not translate; fixed.
- Gathers at a named mip level (with and without an offset) read level 0; fixed.
- Implicit-level samples outside pixel shaders produced invalid DXIL in 730 compute programs; fixed.
- Rectangle lists, depth and stencil twins for depth textures written by compute, texture read-backs.
- Ray tracing, in progress and off by default: the game's own structure-builder programs translate, title memory can be read by raw address from shaders, the hardware ray/box and ray/triangle test is emulated in DXIL (checked against a reference on the GPU), and 59 of the 64 trace programs translate. Nothing is lit by it yet. See [docs/fork-ray-tracing.md](docs/fork-ray-tracing.md).

Speed and memory:
- Waits for memory values, write-watch queries and page queries no longer walk page tables per call.
- Large title buffers get a copy in card memory, with shader writes found by hashing on the card.
- Submissions can be sent ahead of the wait for the card (`VICTORY_LATE_WAIT`), with a relay thread that lands results as the card finishes (`VICTORY_RELAY`): in a first five-minute run the card's idle time behind the submitting thread fell from about 136 to 30 ms per frame.
- Title memory can be opened as a graphics heap so that buffers are used in place instead of copied (`VICTORY_INPLACE`, first stage only; a probe confirmed the approach on the test GPU).
- The bindless texture queue no longer uploads stale views; the view table no longer grows without bound; the pipeline library is mapped instead of read into memory.
- An intermittent device loss was traced to the pipeline library file being replaced while mapped; fixed.

- Translated shaders get clean-up passes over the finished module (dead code, constant folding, reuse of values, branch facts); the pack's host instruction count falls by about 43 %, worth about 20 ms of GPU time per frame on the test machine (`VICTORY_SHADER_OPT=0` turns them off).

Most of the newer speed changes sit behind environment switches that are off
by default (`VICTORY_LATE_WAIT`, `VICTORY_RELAY`, `VICTORY_WIDE_BACK`,
`VICTORY_CARD_BUFFERS_MIN_MB`, `VICTORY_INPLACE`, `VICTORY_BVH_BUILDER`, ...).
Each is described in a comment where it is read.

### Known problems

- Everything in shadow is black (see above).
- The interior of the first gameplay room is sometimes not drawn after the cutscene hands over to gameplay.
- Around 24 GB of memory is in use during play.
- A few style lints are switched off for this fork in the workspace `Cargo.toml` until the newer code is tidied.
- Many changes here are specific to this title and this GPU, and some are marked `shortcut:` in the code where they approximate what the console does.

### justlovett0 fork — Windows 10 / RTX 3080 verification (2026-11-25)

Verified on a second machine: **Windows 10 22H2 (19044), Ryzen 9 7900X,
NVIDIA RTX 3080 10 GB, driver 621.24 (32.0.16.2124)**, Series X profile.

**Windows 10 launch.** The inbox D3D12 runtime reports SM 6.5, so the fork's
SM 6.6 shaders need the **Agility SDK 1.620.2** (upstream PR #175) beside the
executable (`cargo xtask agility`; the exe exports `D3D12SDKVersion` /
`D3D12SDKPath`). With it the title boots, plays the intro and enters the world
without the init crash the SM 6.5 runtime causes.

**Best verified configuration** — textures, world geometry and mostly-correct
static lighting, ~4 fps (230-250 ms per frame):

```bash
VICTORY_BVH_BUILDER=1 VICTORY_BVH_QUARANTINE=1 \
VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 \
VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024 \
VICTORY_TEXTURE_MIP_DROP=1 VICTORY_RELAY=1 \
  victory.exe run "<game>\game_scarlett_master.exe" --execute
```

Findings on this machine:

- **Static ray-traced lighting works** with `BVH_BUILDER=1 QUARANTINE=1`: the
  trace/shading programs read the title's pre-built static acceleration
  structures. The dynamic builder stays off (3 of its 53 programs fail a
  divergent cross-half first-lane read), so shadowed areas are still darker than
  sunlit ones.
- **Texture mip-drop** — a port of upstream VIctory #1272, adapted to this fork's
  lazy host-copy path (`Hosted`/`Made`/`Twin`, idle eviction).
  `VICTORY_TEXTURE_MIP_DROP=1` drops the top mip of pure sampled textures. It
  barely moves the frame rate (the world is not texture-bandwidth bound) but
  **vastly improves lighting**, apparently by keeping irradiance/lighting textures
  resident under lower VRAM pressure. Unset (default) is bit-identical to before;
  render targets, depth and MSAA are exempt; allocation retries on
  `E_OUTOFMEMORY`. Adds SRV mip-window re-basing, subresource remapping for
  copies/resolves, and per-subresource host-written tracking.
- **`VICTORY_RELAY=1`** (ordered relay worker) gives ~230-250 ms vs ~260-270 ms
  without it, overlapping copy-back/merge with the next frame's preparation.
  **`VICTORY_RELAY=2`** (precise per-memory waits) is *slower* here (250-270 ms).
- **The frame rate is latency/synchronisation bound, not throughput bound**: CPU
  and GPU usage are low during play. With minimal rendering (looking straight up)
  the frame is ~150 ms; a full scene adds ~80-100 ms. The ~150 ms floor is GPU
  read-back fetch latency plus the title's own frame pacing, not compute — so more
  GPU/CPU horsepower does not help; overlap does.

Negative results on this machine (do not use):

- `VICTORY_INPLACE=1` and `VICTORY_TEXTURE_NO_WAIT=1` both **break the RT
  lighting**: the flight tracking knows shader writes only through declared copy
  ranges, so bypassing copies leaves stale/empty lighting reads.
- `VICTORY_WIDE_REFRESH=1`, `VICTORY_CARD_REHASH_RUNS=1`,
  `VICTORY_UNWATCHED_FRAMES=2`: no frame-rate gain (the refresh copies are kernel
  page-fault bound, not thread bound).
- The newer `fxceless/victory-fork` **shader clean-up optimizer renders the scene
  black** here, and its **#11 lane-mask pair-restore removes world geometry**
  (floor/walls) on this base — those shader-side changes do not transfer to this
  fork's wave64 emitter unchanged. **Even a "safe" optimizer subset** (dead code,
  folding, reuse, load-CSE = `VICTORY_SHADER_OPT=23`, excluding branch facts and
  the wave64 first-lane exchange) renders black with missing geometry at
  500-700 ms/frame: the optimizer is written for victory-fork's different emitter
  and is unusable on this base in any combination.

### A four-idea latency plan, tried and measured (2026-11-25)

The frame is **latency-bound, not throughput-bound** (CPU/GPU usage low): a full
scene is ~172 ms GPU rendering + ~64 ms CPU prep on the critical path, with the
~86 ms GPU→CPU read-back fetch overlapped by `RELAY=1`. Four ideas from a faster
reference were implemented behind switches and A/B tested in the same scene:

- **VRAM residency** (`VICTORY_SCRATCH_VRAM=1`, kept, default off): CPU-written
  scratch in CPU-visible VRAM via Resizable BAR (`Host::gpu_upload_buffer`,
  CUSTOM + WRITE_COMBINE + L1, probed by `Host::rebar`, budget-guarded).
  **Inactive on this box — the ReBAR probe is false** (Win10/RTX 3080); needs
  Resizable BAR enabled. Falls back to system-RAM scratch cleanly.
- **Batched indirect barriers** (`VICTORY_INDIRECT_BATCH`): the existing
  `VICTORY_INDIRECT_WINDOW` alone gives no gain (260 ms), and deferring the
  per-command return-to-COMMON barrier across window hits dropped world
  geometry. The per-command barriers are not on this title's critical path;
  abandoned.
- **Parallel CPU prep** (`VICTORY_WIDE_PREP`): parallel `GetWriteWatch` over
  disjoint runs (local journals, single asking-number merge under the held
  lock) dropped world geometry — concurrent kernel write-watch RESET calls race
  and miss writes. Reverted.
- **CPU/GPU pipelining** (serve the title's GPU-memory reads card-side so frame
  N+1 overlaps N): the safe forms already exist (`late_met`, `relay_exact`);
  the full form is the `VICTORY_INPLACE` class that breaks RT lighting, so not
  attempted enabled.

Net: the safe ceiling on this machine is ~230 ms (`RELAY=1` + mip-drop=1). The
reference's 45-90 ms relies on the pipelining bypass and a ReBAR-enabled host.

The intro pre-rendered video logo jitters in every mode (a frame-token pacing
issue in `frame.rs`, not the relay or the shaders); the world itself presents in
order with `RELAY=1`.

Everything below this line is upstream VIctory's own README, unchanged; its
status section describes upstream, not this fork.

---

# VIctory

Runs the title (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

On Windows 10 the in-box D3D12 runtime stops at shader model 6.5, so the host
refuses every translated pipeline. `cargo xtask agility` fetches Microsoft's
D3D12 Agility SDK runtime (a pinned, hash-checked NuGet package) into
`target\<profile>\D3D12\`, where `victory.exe` makes D3D12 load it. Windows 11
needs nothing.

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo xtask agility                      # Windows 10 only: newer D3D12 runtime
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | reduce the per-frame wait for background pipeline creation from the default of 500 ms. A draw can be omitted until its pipeline is ready |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
