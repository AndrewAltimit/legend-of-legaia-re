# Reusing this repo for other PS1 games

Most of this repository describes one game, but a fair share of it describes
the PlayStation: the disc sector layout, Sony's file formats (TIM, VAB, SEQ,
XA, STR), the GTE and SPU hardware, emulator save states. This page sorts the
crates and scripts by how far they carry to another title, names every Legaia
assumption that would trip a second game, and then lays out the working method
the project runs on, which transfers whole.

The classification below is read off each crate's source, not its README. Three
classes:

| Class | Means |
|---|---|
| **Generic PS1** | Works on any PSX disc or any file of that format. |
| **Generic, Legaia quirks** | The format or hardware is Sony's, but the code bakes in something Legaia does. Each quirk is named below. |
| **Legaia-only** | Describes Legaia's own containers, data or runtime. Useful as a worked example, not as a dependency. |

## Depending on a crate

Nothing here is published to crates.io. Take a crate as a git dependency, and
turn off its default `cli` feature so the command-line dependencies (`clap`,
and for some crates other Legaia crates) stay out of your build:

```toml
[dependencies]
legaia-tim = { git = "https://github.com/AndrewAltimit/legend-of-legaia-re", default-features = false }
legaia-iso = { git = "https://github.com/AndrewAltimit/legend-of-legaia-re", default-features = false }
```

Every crate is `MIT OR Unlicense`. The CLI binaries build with
`cargo build --release -p <crate>` and land in `target/release/`. None of them
embeds Sony bytes, and none needs a Legaia disc to run on another game's files
unless noted.

## The crates at a glance

| Crate | Class | Entry point | CLI (`cli` feature) |
|---|---|---|---|
| [`legaia-iso`](../../crates/iso/README.md) | Generic PS1 | `raw::RawDisc::open`, `iso9660::{read_volume, walk_files}`, `image::walk_iso_files`, `write::encode_mode2_form1_sector` | `disc-extract list` / `extract` |
| [`legaia-xa`](../../crates/xa/README.md) | Generic PS1 | `demux::demux_raw_sectors`, `decode`, `StreamingDecoder`, `write_wav` | `xa` |
| [`legaia-bytes`](../../crates/bytes/README.md) | Generic PS1 | `u16_le` / `u32_le` / ... returning `Option` | - |
| [`legaia-tim`](../../crates/tim/README.md) | Generic, Legaia quirks | `parse`, `parse_strict`, `decode_rgba8`, `encode::encode_replacement`, `vram::Vram` | `tim info` / `convert` / `convert-dir` |
| [`legaia-vab`](../../crates/vab/README.md) | Generic, Legaia quirks | `parse`, `find_vabs`, `decode_vag`, `write_wav` | `vab list` / `extract` |
| [`legaia-seq`](../../crates/seq/README.md) | Generic, Legaia quirks | `Seq::parse`, `parse_header_with_len`, `read_vlq` | `seq info` / `events` / `json` / `find` |
| [`legaia-lzs`](../../crates/lzs/README.md) | Generic, Legaia quirks | `decompress`, `compress`, `compress_optimal` | `lzs-decode raw` / `scan` / ... |
| [`legaia-mdec`](../../crates/mdec/README.md) | Generic, Legaia quirks | `MdecDecoder::decode_frame`, `str_sector::StrFrameAssembler`, `str_av::demux_str_av` | `mdec scan-str` / `decode-str` / ... |
| [`legaia-mednafen`](../../crates/mednafen/README.md) | Generic, Legaia quirks | `SaveState::from_path`, `main_ram`, `gpu::PsxGpu`, `spu::PsxSpu` | `mednafen-state` |
| [`legaia-pcsx-redux`](../../crates/pcsx-redux/README.md) | Generic, Legaia quirks | `SaveState::from_path_structural`, `gpu::PcsxGpu::from_path`, `fixture::SyntheticState` | `pcsxr-state --game generic` |
| [`legaia-ram-image`](../../crates/ram-image/README.md) | Generic, Legaia quirks | `diff::diff_ram`, `bisect::bisect_first_bad`, `prim_pool::decode`, `ram_slice`, `GameProfile` | - |
| [`legaia-engine-audio`](../../crates/engine-audio/README.md) | Generic, Legaia quirks | `spu::Spu`, `Sequencer::new(seq, bank)`, `vab_bind::VabBank::upload` | `note-trace` |
| [`legaia-render-kernels`](../../crates/render-kernels/README.md) | Generic module in a Legaia crate | `gte::Gte`, `gte::Camera`, `gte::gte_divide`, `vram_capture` | - |
| [`legaia-tmd`](../../crates/tmd/README.md) | Legaia-only | `parse` (Legaia variant only) | `tmd` |
| [`legaia-container-formats`](../../crates/container-formats/README.md) | Legaia-only | `parse_streaming`, `pack`; `tim_scan` is generic | - |

Four more crates carry one generic module each inside a Legaia crate: the
memory-card walker `legaia_save::card` (a 128 KiB PSX card's directory frames
and block chains), the PPF 3.0 writer and applier `legaia_disc_patch::ppf`
(`diff_runs`, `write_ppf3`, `apply_ppf3`), the R3000 instruction encoders and
small simulator `legaia_code_hooks::{mips, mips_sim}`, and the GameShark text
and Mednafen `.cht` parsers in `legaia-cheats` (its address classifier is
Legaia's RAM map). `disc-patch` and `code-hooks` depend on Legaia format
crates, so for those two, copying the module is lighter than depending on the
crate.

## Generic PS1 crates

### `legaia-iso`

A raw Mode 2/2352 `.bin` (or its `.cue`) reader, an ISO9660 walker that
resolves names the way the console does (case-insensitive, `;1` stripped), and
the write side: `encode_mode2_form1_sector` recomputes EDC and P/Q ECC by the
ECMA-130 math, `encode_mode2_form2_sector` the Form 2 EDC, and
`patch_file_logical` rewrites bytes inside a file and re-encodes every touched
sector. `region::parse` reads the product code off `SYSTEM.CNF`; `sniff`
explains why a picked file is not a raw dump (`.iso`, `.chd`, `.pbp`, ECM).
`image` is the borrowed-slice reader with no `std::fs`, so it builds for
`wasm32`.

Four things name Legaia and are easy to step around:

- `image::extract_prot_dat`, `extract_cdname_txt` and `extract_scus` are
  shorthands for Legaia's root files. Use `iso9660::read_file_in_image(image,
  name)` or `find_path_in_image(image, "DIR/FILE.EXT")` instead.
- `disc-extract verify` knows only Legaia's image hash; pass `--expected`.
- `relayout::grow_prot_dat` grows one file by whole sectors and renumbers every
  ISO9660 reference after it. The mechanics are generic, but its safety rests
  on a fact proved for Legaia only: no executable carries an absolute LBA for a
  file after the grown one ([`disc.md`](../formats/disc.md#full-iso-relayout)).
  A game that seeks by hard-coded LBA breaks when its files move. Search its
  executable for the LBAs first.
- `grow_prot_dat_by_name` looks up `PROT.DAT` by name.

### `legaia-xa`

CD-XA ADPCM: the 128-byte sound group, 4- and 8-bit units, mono and stereo,
37.8 / 18.9 kHz, decoded bit-identically to jPSXdec's reference decode.
`demux::demux_raw_sectors` splits interleaved raw sectors by file / channel
from their subheaders, `StreamingDecoder` decodes incrementally. The one
Legaia-shaped helper is `demux_disc_all`, which only visits files whose names
end in `.XA`; a game that interleaves its XA inside `.STR` files or a big
archive should feed `demux_raw_sectors` or `demux_disc_range` the raw sectors
itself.

### `legaia-bytes`

Bounds-checked little-endian readers that return `Option`. No dependencies.

## Generic crates with Legaia quirks

### `legaia-tim`

The TIM parser, PNG export and PNG-to-TIM encoder are Sony's format end to end.
`parse_strict` reproduces jPSXdec's TIM detector, which makes it a good carving
filter for any disc. One Legaia behaviour lives in the software VRAM model:

- **`Vram::upload_tim` lands a CLUT block as a linear run along row `fb_y`**,
  not as a `w x h` rectangle: palette `i` of a multi-row CLUT goes to
  `(fb_x + 16*i, fb_y)`. That is what Legaia's loader does, pinned against
  retail VRAM; a stock `LoadImage` of the same block writes a rectangle. For
  another game, upload its CLUTs with `Vram::write_block` until a capture says
  which layout its loader uses.
- `parse_strict` bounds-checks the image rectangle but not the CLUT one,
  because Legaia parks NPC palettes at the framebuffer's bottom edge. That only
  makes the filter more permissive.

### `legaia-vab`

`parse(buf, offset)` reads a standard, contiguous VAB (`pBAV`, version 7,
128 program slots, 16 tones per program, the 1-indexed VAG size table) and its
VAG bodies; `decode_vag` decodes SPU ADPCM. The quirk is the carriage, not the
format: on Legaia's disc every VAB is split across two chunks of a DATA_FIELD
stream, with a 4-byte chunk header between the header part and the bodies.
`parse_in_stream` and `vag_body_origin` exist for that. On a standalone VAB,
`parse` is the right call and the stream helpers report an error, which is the
signal to keep `parse`'s spans. A game that ships separate `.VH` / `.VB` files
needs them concatenated first ([`vab.md`](../formats/vab.md)).

### `legaia-seq`

SEQ is libsnd's sequence format. Two behaviours differ from the shape the
public PsyQ notes describe, and both break a MIDI-minded parser:

- the header's version field is a `u32` big-endian (15-byte header);
  `parse_header_with_len` detects it (`u32 == 1`) and falls back to the 13-byte
  `u16` shape otherwise;
- meta events carry no length byte: tempo is `FF 51 tt tt tt`, end of track is
  `FF 2F`, and a meta latches `FF` as running status. An unknown meta type
  stops the track.

Both are what the libsnd SsAPI reader statically linked into `SCUS_942.54`
consumes (event decoder `FUN_80063CEC`), so other titles built on libsnd most
likely ship the same shape. No other title's SEQ is tested here. A file that
does carry SMF-style length bytes would read its `03` as the first tempo byte;
check one tempo event by hand before trusting a parse
([`seq.md`](../formats/seq.md)).

### `legaia-lzs`

The decompressor is the classic LZSS family (4096-byte ring, 18-byte maximum
match, threshold 2, write cursor starting at `0xFEE`, LSB-first flag bits, a
12-bit absolute ring position and a 4-bit length), traced from `FUN_8001A55C`.
What is Legaia's:

- the ring starts **zero-filled**; the textbook LZSS initialises it to spaces
  (`0x20`). A game whose decoder pre-fills spaces decodes differently in the
  first 4 KB;
- no length prefix or end marker: the output size comes from the caller;
- the `.lzs` container header (`decompress_container`) is Legaia's.

Compare the target's decompressor against these parameters before reusing the
crate. Because the zero ring turns most random input into plausible output,
"decompresses without error" proves nothing: magic-check the decoded bytes
([`lzs.md`](../formats/lzs.md)). `compress` / `compress_optimal` emit streams
any decoder with these parameters accepts.

### `legaia-mdec`

`str_sector` parses the standard STR video-sector header (`0x0160` / `0x8001`)
and assembles multi-sector frames; `str_av::demux_str_av` pairs the frames with
the interleaved XA; `analyze_str_timing` recovers the frame rate from the
sector stride at the 2x CD rate. Those carry to any STR. The decoder does not:

- `MdecDecoder::decode_frame` decodes the **Iki** bitstream (an LZSS-packed
  per-block qscale/DC table ahead of an AC-only bitstream), which only a
  handful of titles use. Most PSX movies are STRv2/v3.
- A STRv2 frame shares Iki's `0x3800` word at `+2`, so `decode_frame` does not
  reject it: it reads the version field as an LZSS size and returns a blank or
  garbage frame without an error. Check `frame[6..8]` (2 or 3 means STRv2/v3)
  before calling it.
- `strv2_decode::decode_frame` does decode STRv2/v3, but against the VLC
  lookup table `strv2_table` unpacks out of Legaia's STR overlay (PROT 0970).
  Another game needs its own copy of that table.
- `st_ring` / `str_player` model how Legaia drives the `St` streaming library
  (ring size, segment windows of `MV3.STR`).

### Emulator save states: `legaia-mednafen`, `legaia-pcsx-redux`, `legaia-ram-image`

The three crates split into a generic reader and a Legaia anchor layer.

- **Mednafen.** `SaveState::from_path` walks the `MDFNSVST` section index of
  any PSX state; `main_ram_structured()` returns `MAIN.MainRAM.data8`, and
  `PsxGpu` (VRAM, display registers) and `PsxSpu` (SPU RAM, voices, reverb)
  read their sections structurally. `main_ram()` falls back to a search for
  Legaia's strings when the entry is missing; `main_ram_with_anchors(exe,
  profile)` is that fallback with another game's executable and strings. The
  CLI binary, unlike the library, pulls in `legaia-actor-formats` and
  `legaia-cheats`, and its `identify`, `display-list`, `write-taxonomy`,
  `clut-trace`, `prim-dispatch-*` and `world-map-camera` subcommands are
  Legaia's; `extract`, `diff`, `bisect`, `trace`, `watch`, `vram-dump`, `spu`
  and `prim-trace` (given an explicit `--pool-base` / `--pool-end`) are not.
- **PCSX-Redux.** Main RAM, the GPU snapshot (`gpu::PcsxGpu`: VRAM plus the
  GP1 control log) and the hardware blob behind `SaveState::scratchpad` are
  all found by walking the state's protobuf for members of known length: RAM
  is the first member of at least 2 MiB in the one submessage that also holds
  the 64 KiB hardware region. That reads only the emulator's layout, so any
  game's state opens with no executable (`SaveState::from_path_structural`,
  or `pcsxr-state --game generic`). The Legaia anchor search survives as the
  fallback for a payload the walk does not recognise, and a disc-gated test
  checks that the two land on the same offset for every library state.
  `SaveState::from_sstate_bytes_with_anchors(bytes, exe, profile)` is that
  fallback for another game. The scene / mode / player accessors read
  Legaia's globals; `identity_with(profile)` asks a different profile.
- **`legaia-ram-image`.** `diff::diff_ram` (the watchpoint-equivalent diff of
  two RAM images), `bisect::bisect_first_bad`, `ram_slice` and
  `prim_pool::decode` / `chain_walk` (the libgpu primitive pool and ordering
  table) take any 2 MiB image. What a reader needs to know about the game is
  a `profile::GameProfile`: the anchor strings for the fallback RAM search,
  how to identify a state from its globals, and the scenario manifest's
  default save-file name and directory variable. `GenericGame` knows none of
  them; `game_anchors::Legaia` is this project's, and every API that predates
  profiles uses it, so Legaia callers see no change.
  `extract::main_ram_via_exe_anchors(payload, exe, strings)` takes any game's
  strings and reads the load address from the `PS-X EXE` header, and
  `ScenarioManifest::parse_toml_for(text, profile)` fills an absent
  `[defaults]` field from the profile (a manifest may also set
  `mcs_dir_env` itself). `prim_dispatch` and `source_hunt` stay Legaia's.

### A worked example

[`crates/pcsx-redux/examples/bring_your_own_game.rs`](../../crates/pcsx-redux/examples/bring_your_own_game.rs)
runs the generic crates on inputs built in code, none of them from a game:
a 4-bit TIM with its CLUT through `parse_strict`, `decode_rgba8` and
`write_png`; a SEQ through `Seq::parse`; a two-directory ISO9660 image
mastered as Mode 2/2352 sectors with `write::encode_mode2_form1_sector` and
read back by both the in-memory walker and `RawDisc`; and a made-up game's
PCSX-Redux and mednafen states, opened structurally, diffed with `diff_ram`
and bisected for the capture where a value changed. `cargo run -p
legaia-pcsx-redux --example bring_your_own_game -- <out_dir>` writes the PNG
and the `.bin`; the image lists and extracts under `xorriso` once its user
data is pulled out, so it is a real ISO9660 volume, not one only this reader
accepts. The test `tests/bring_your_own_game.rs` runs the same code in CI.

The SEQ step is the one to read closely. It builds the shape `legaia-seq`
parses (a 13-byte header with a `u16` version, meta events without a length
byte), which is what libsnd's reader in Legaia's executable consumes. No
other title's SEQ file is tested in this repository, so the step is not
evidence that a second game ships this shape. It also demonstrates the trap:
an SMF-style tempo event (`FF 51 03 tt tt tt`) does not read as the right
tempo, so check one tempo event of your own files by hand first.

### `legaia-engine-audio`

`spu::Spu` is a from-scratch SPU: 24 voices, 512 KiB SPU RAM with the transfer
engine, ADSR, Gaussian interpolation, the register-driven reverb network with
the libspu presets, CD input mixing. It does not model pitch modulation, noise
or FM, because Legaia never enables them; a game that does needs those added.
`Sequencer::new(seq, bank)` plays one libsnd SEQ against one `VabBank`
(`vab_bind::VabBank::upload` puts the VAG bodies into SPU RAM), with libsnd's
loop markers (controller 99, values 20 / 30) read from the stream, on an
integer clock. Those are libsnd-shaped, not Legaia-shaped.

The rest of the crate is Legaia's runtime: `battle_voice`, `shout`,
`xa_clip_bank`, `duck`, `bgm_tail`, `seq_slots`, `sfx_ring`, `spu_layout`. The
crate also lists `legaia-prot` as a plain dependency, although only the
`note-trace` binary names it, so a library build still compiles the PROT
reader. `AudioOut` is the cpal output; `WebAudioOut` sits behind the
`audio-webaudio` feature on `wasm32`.

### `legaia-render-kernels::gte`

`gte::Gte` is a register-level GTE: data and control registers with `mtc2` /
`mfc2` / `ctc2` / `cfc2` / `lwc2` / `swc2`, every command (`RTPS`, `RTPT`,
`NCLIP`, `MVMVA`, `NCS`, `NCT`, `NCDS`, `NCDT`, `NCCS`, `NCCT`, `CC`, `CDP`,
`DPCS`, `DPCT`, `DCPL`, `INTPL`, `SQR`, `OP`, `GPF`, `GPL`, `AVSZ3`, `AVSZ4`),
the FLAG bits, per-command cycle costs, and the Unsigned Newton-Raphson
perspective divide (`gte::divide`) the hardware really runs. `psx_sin` /
`psx_cos` reproduce the q3.12 trig values without the lookup table the executable carries.
`vram_capture` quantises an RGBA8 frame to BGR555 and blits it into a
`legaia_tim::Vram` rect.

The module is generic; the crate is not. `render-kernels` depends on Legaia's
camera, VM and format crates, and `gte::camera_relative` uses Legaia's camera.
Vendoring `src/gte/` without `camera_relative.rs` is the practical route.

## Legaia-only crates

- **`legaia-tmd`** reads Legaia's TMD variant, not Sony's. `parse` requires
  bit 31 of the `id` word (Legaia's `0x80000002`) and rejects a stock
  `0x00000041` TMD, whose offset mode lives in the `flags` word instead; and
  the primitive walker `legaia_prims` reads Legaia's grouped primitives (an
  8-byte group header and a per-mode descriptor table), not stock packets. The
  object table and `SVECTOR` layout are shared, so the crate is a starting
  point for a stock reader, not one ([`tmd.md`](../formats/tmd.md)).
- **`legaia-container-formats`** is Legaia's container layer: the asset-type
  dispatcher, DATA_FIELD chunk streams, the pack format, the sound bundles. Its
  `tim_scan` (carve every TIM out of a buffer through `parse`) is generic but
  brings the crate's Legaia dependencies with it.
- `prot`, `mes`, `anm`, `mdt`, `art`, `font`, the format-family crates and
  every `engine-*` crate describe Legaia's own data and runtime.

## Emulator automation scripts

**PCSX-Redux probes** ([`pcsx-redux-automation.md`](pcsx-redux-automation.md)).
`scripts/pcsx-redux/lib/probe/` is a Lua library over PCSX-Redux's Lua API with
no Legaia knowledge in most of its modules: `mem` (RAM reads), `bp`
(breakpoints), `pad` (button overrides), `sstate` (load gzipped or bare
states), `sm` (the boot / arm / capture / quit driver every probe runs),
`step` (instruction trace and write attribution), `watch`, `csv`, `snapshot`,
`spu`, `prim_scan`, and `spec` / `toml` (declarative `.probe.toml` probes). The
Legaia parts are `version` (fingerprints USA `SCUS_942.54` code before a probe
arms), `battle_state`, the `symbols` table generated from this project's Ghidra
database, and the `LEGAIA_*` environment-variable names. `run_probe.sh`
launches the emulator with a BIOS, an image and an autorun script, and stages
the image in a private directory so a `.ppf` beside it is never applied by
accident; its defaults point at Legaia paths. The `autorun_*.lua` probes are
Legaia-specific worked examples of the pattern.

**Mednafen** ([`mednafen-automation.md`](mednafen-automation.md)).
`run-mednafen.sh` (launch with a state and an input movie),
`state-diff.py` and `watchpoint-bisect.py` wrap the generic `mednafen-state`
subcommands. The rest of `scripts/mednafen/` reads Legaia structures.

The `version` guard's idea carries further than its code: a probe that
hard-codes addresses should check a few words of always-resident code before it
arms anything, because on the wrong build or region it otherwise returns a CSV
of plausible garbage with no error.

## The transferable method

The parts of this project most worth taking to another game are not crates but
practices. [The retail-oracle method](retail-oracle-method.md) writes them up
engine-neutral, in the order to adopt them, with this project only as worked
examples. Each is documented for Legaia; the pages below are the reference,
and nothing in them depends on Legaia beyond the examples.

### Measure against retail RAM and VRAM

[`retail-compare.md`](retail-compare.md) is the corpus oracle: for every
catalogued retail save state, read the observables out of the state's own RAM
(scene, camera, party, flags, battle state) and the displayed rectangle out of
its VRAM, seed the reimplementation into the same situation, tick it until it
settles, and score each channel in `0..1`. A committed baseline ratchets the
scores, so a change that loses accuracy fails. Nothing runs an emulator: the
save state is the oracle. The time-axis sibling,
[`cast-sequence-oracle.md`](cast-sequence-oracle.md), records one sequence on
both sides and scores it frame by frame; [`ratchet-integrity.md`](ratchet-integrity.md)
covers keeping a ratchet honest (a baseline may not loosen silently, and a
skipped test is not a passing one).

### Grade every claim by its evidence

Every answered question carries one of four grades: `disassembly`, `capture`,
`decompiled-C` or `inference` ([what a grade means](../reference/open-rev-eng-threads.md#what-an-evidence-grade-means),
[the settled register](../reference/re-settled-threads.md#the-evidence-column)).
`decompiled-C` marks a claim nobody has confirmed against instructions, which
makes the grade a re-audit worklist rather than an insult.

### Keep the falsified readings

[`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md) records the
plausible readings of the bytes that turned out wrong, with the reasoning
intact. On a format with no documentation the same wrong reading is attractive
to every newcomer; the register is what stops it being re-derived.

### Take the denominator from the disc

Coverage counted over the functions someone already dumped always looks good.
[`disc-coverage.md`](disc-coverage.md) counts each code image's bytes covered
by dumped functions, and [`byte-accounting.md`](byte-accounting.md) counts, per
archive entry, which bytes any parser consumes and what shape the rest has, so
an unwalked format shows up as a run at the top of a list.
[`address-reference-scan.md`](address-reference-scan.md) turns "no caller
found" into "no reference exists" by searching every encoding of an address
(literal word, `lui` pair, `jal`, `j`, branch).

### Make runs reproducible

[`determinism-replay.md`](determinism-replay.md): a small text pad script
(`j-replay-v1`) recorded from a session replays to bit-identical state traces,
which turns every bug report into a regression test.
[`soak-harness.md`](soak-harness.md) generates such scripts from seeded random
play and reduces each finding to the input that matters.

### Do not trust the decompiler's C

[`ghidra.md` § decompiler artifacts](ghidra.md#decompiler-artifacts-that-have-produced-false-claims)
lists the ways Ghidra's C has put false claims into these docs: intra-function
labels printed as calls, dropped register arguments, reordered or missing
stores, dumps taken at the wrong load base, annotations read as evidence. It is
MIPS- and Ghidra-specific, not Legaia-specific.
[`call-target-integrity.md`](call-target-integrity.md) and
[`dump-corpus-integrity.md`](dump-corpus-integrity.md) cover what a wrong
overlay base does to `jal` targets and to printed addresses.

### Traps any PS1 project meets

- A save state replays the RAM of the disc that booted it, so a state loaded
  onto a patched image still runs the old code and data; verify disc edits from
  a cold boot or a memory-card load.
- PCSX-Redux applies a `.ppf` sitting beside the image it is given, without
  asking ([`pcsx-redux-automation.md`](pcsx-redux-automation.md#patched-disc-taint)).
- Ghidra does not resolve MIPS `lui` / `addiu` pairs as references, so an xref
  query returns nothing for a heavily used global ([`ghidra.md`](ghidra.md)).
- "No static caller in the executable" means "look in the overlays", not "dead
  code": most game logic on the PSX lives in RAM overlays
  ([`overlay-capture.md`](overlay-capture.md)).
- Shell observers that watch themselves: a pipe's exit status read off the
  wrong stage, `pgrep -f` matching its own caller
  ([`shell-observer-traps.md`](shell-observer-traps.md)).
