# Finding what makes a frame slow

A way to go from "the frame takes a second" to the few programs that take
the time, and to why they are slow on a given card. Every step runs on your
own machine. Everything it saves (logs, shaders, compiler output) is data
derived from the title: keep it outside the repository and out of issues
and pull requests ([asset-policy.md](asset-policy.md)). Name programs by
what you measured about them, not by their hashes.

## 1. CPU or GPU?

A release build logs a frame-rate line every five seconds:

```
0.9 fps, 1101.7 ms/frame (112.3 preparing: copies 38.1 ms 138.0 MB, textures 1.8 ms 7.8 MB; 960.6 on the GPU; 963.8 copying back 65.0 MB), ...
```

- `preparing` is the submitting thread's own work for a frame: copies of
  title memory, texture refreshes, recording.
- `on the GPU` is the time that thread stood waiting for the card.
- `copying back` is what shaders wrote, brought back to title memory. With
  the relay (the default since c96b4f2) a thread of its own does this
  beside the submitting thread, so it overlaps the rest and is not part of
  the frame's sum.

The line `the card's clock per frame: nothing of ours queued N ms (the card
about P% busy by this count) ...` says how long the card had nothing to do.
When the card is close to fully busy and `preparing` is a small part of the
frame, the GPU's work is the limit, and the following steps apply.

Stand still in one place for every measurement and use the median of the
last minute of lines: the first minutes after loading still translate
shaders and make pipelines (`dropped N without a host pipeline (M being
made)` goes to `(0 being made)` once they are done).

## 2. Which commands take the card's time

Use the diagnostics build
(`cargo build --profile diag -p victory --features diagnostics`) with

```
VICTORY_GPU_TIMING=1
VICTORY_D3D12_DRED=1
```

Both are needed. `VICTORY_GPU_TIMING` runs and times each list of a
submission on its own; the commands inside a list are only timed when they
are noted, and they are noted only when device-removed data is on
(`VICTORY_D3D12_DRED`). Submissions are no longer sent ahead in this run,
so its frame time is not comparable with a normal run; the card's times
are.

Every five seconds the copy statistics then include:

- `the card's time by command (timing run), all N kinds together: T ms a
  frame; by first word: indirect ..., dispatch ..., mesh ...`: the card's
  time per frame by kind of command;
- up to forty lines `the card's time by command (timing run): T ms a frame
  in N commands a frame, the longest L ms: <command> [<pipeline>]`, the
  heaviest first.

Commands that run once a frame and take tens of milliseconds stand out
from those that run thousands of times for a few microseconds each.

Lists over 50 ms are also logged as `slow host list`, with their six
slowest commands and, for lists with indirect dispatches,
`indirect_groups_asked` (the largest group count asked for, x by y by z),
`indirect_groups_run` (the groups run, after the argument clamp) and
`indirect_bounded` (dispatches the clamp changed). A dispatch the clamp
bounds was asked for more work than the host allows, usually because its
arguments were not written. A dispatch it never bounds asks for that much
work on purpose.

## 3. How much there is to gain

`VICTORY_SKIP_PIPELINES=<hash>,...` (diagnostics build) leaves out every
draw and dispatch whose pipeline holds one of the listed shaders. The
frame time without them is an upper bound on what making them faster can
give. The picture is wrong in such a run: what those programs compute is
missing, and what reads it draws differently. Do not judge anything else
from it.

## 4. Why a program is slow on this card

`VICTORY_SHADER_SAVE=<folder outside the repository>` saves each shader
as the title gave it (`<hash>.<stage>.cdxil`) and as translated
(`<hash>.<stage>.dxil`) the first time it is used.

`victory translate <file.cdxil> --validate --list` translates a saved
shader again offline and prints what the translation found: IR
instructions, vector instructions, guarded instructions, branches per
lane, descriptors loaded. `victory shader-info <file.cdxil> --verbose`
prints the console program's size in instructions. Translate with
`VICTORY_HOST_MAX_WAVE` set to the card's widest wave (the log line `host
wave lanes min=.. max=..`) to get the form the runtime uses on that card,
and with `VICTORY_EMIT_DUMP=<folder>` to write the result.

What the card's own compiler makes of the translated program can be read
offline with AMD's Radeon GPU Analyzer (RGA, GPUOpen, MIT licence) on AMD
cards. RGA needs a root signature for DXIL that has none. For the runtime's
compute programs this one is close enough to read register use:

```
#define RS "RootFlags(CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED | SAMPLER_HEAP_DIRECTLY_INDEXED), RootConstants(num32BitConstants=<N>, b0), StaticSampler(s0), ..., StaticSampler(s<K>)"
```

`<N>` is the length of the `%UserData` array and `s0`..`s<K>` are the
samplers in the program's resource bindings (both in
`dxc -dumpbin <file.dxil>`). A program that also binds root constant
buffers or root SRVs needs those added in their register spaces. RGA's
own root-signature compiler refuses the heap flags, so compile the
signature with DXC and pass the binary:

```
dxc -T rootsig_1_1 -E RS rs.hlsl -Fo rs.bin
rga -s dx12 -c <target, e.g. gfx1032> --cs-blob <file.dxil> --rs-bin rs.bin --isa out.isa -a out.csv --livereg out.live.txt
```

In `out.csv`, `USED_VGPRs` at the card's maximum (256) together with a
non-zero `SCRATCH_MEM` means the program spills registers to memory and
runs with few waves in flight. `out.live.txt` shows the live registers at
each instruction. The highest register a console program uses
(`shader-info --disasm`) is the comparison: what the console runs in.

## Results on one machine

Windows 11, AMD Ryzen 7 9700X, AMD Radeon RX 6600 8 GB (gfx1032, driver
32.0.21045.1000), 32 GB. Commit c96b4f2, standing still in the first
gameplay scene. Medians of the last minute.

| Run | ms per frame |
|-----|-------------:|
| defaults (relay stage 2, wide copy-back, card copies 1-1,024 MB, 256 lists in the air) | 1,100 |
| the defaults before c96b4f2 (`VICTORY_RELAY=0 VICTORY_WIDE_BACK=0 VICTORY_CARD_BUFFERS_MIN_MB=16 VICTORY_CARD_BUFFERS_MB=512 VICTORY_LATE_WAIT_LISTS=128`) | 1,385 |
| `VICTORY_SHADER_LOOP_LIMIT=256` | 1,160 |
| four compute programs left out (`VICTORY_SKIP_PIPELINES`; picture wrong) | 345 |

With the defaults the submitting thread prepared for about 112 ms a frame
and waited for the card about 960 ms; the card was about 90% busy.

The timing run counted 800-840 ms of the card's time a frame: about 575 ms
in about 5,900 indirect commands and 210 ms in about 3,700 dispatches.
Four compute programs, each run once a frame, took about 668 ms of it
(82%): three indirect dispatches of about 290, 110 and 90 ms and a direct
dispatch of 3,600 groups of about 177 ms. The indirect ones asked for about
9,000 groups each, about 50,000 a list, and the clamp never bounded them.

RGA for gfx1032 gives the four translated programs 256 VGPRs each and
2,080 to 5,632 bytes of scratch per thread (thousands of scratch loads and
stores; the largest also spills SGPRs into VGPR lanes). The console
programs themselves use 49 to 128 VGPRs (their highest register). The
register pressure comes from the translation: in the program looked at
closely, the card's code at its peaks copies dozens of values between
registers before each branch, and about 160 VGPRs are live on average. Marking the values of scalar
memory loads as wave-uniform (`WaveReadLaneFirst`) and turning the
translator's clean-up passes off (`VICTORY_SHADER_OPT=0`) changed neither
the registers nor the scratch.
