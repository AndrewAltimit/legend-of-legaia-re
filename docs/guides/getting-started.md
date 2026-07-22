# Getting started with the release tools

This guide is for someone who downloaded a release archive from the project's
GitHub Releases page: a directory of prebuilt command-line binaries, the two
license files, and a short `README.txt`. No Rust toolchain, no source checkout.

One rule shapes everything here: **the project ships no Sony-owned bytes.**
The archive contains only the project's own compiled tools. Every tool operates
on a disc image *you* supply - a raw Mode 2/2352 `.bin` dump of your own
*Legend of Legaia* (USA) disc, `SCUS-94254`. A `.cue` sheet is accepted
anywhere a disc image is: the referenced `BINARY` track is resolved
automatically.

Commands below use the bare `./tool` form, as run from inside the unpacked
archive directory. If you build from source instead, the same binaries land in
`target/release/` (`cargo build --release`), so substitute
`./target/release/tool`.

## 1. Download and verify the archive

Each tagged release publishes one archive per platform plus a `SHA256SUMS`
manifest (see [releases.md](../tooling/releases.md) for how they are built):

- `legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz` (Linux, glibc 2.28+)
- `legaia-tools-<version>-aarch64-unknown-linux-gnu.tar.gz` (Linux on ARM)
- `legaia-tools-<version>-x86_64-pc-windows-gnu.zip` (Windows)

Verify and unpack:

```bash
sha256sum -c SHA256SUMS --ignore-missing
tar -xzf legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz
cd legaia-tools-<version>-x86_64-unknown-linux-gnu
./legaia-extract --version
```

Every binary answers `--version` and `--help`; subcommand help
(`./asset monster-archive --help`) states where each input comes from.

## 2. Check your disc image

```bash
./disc-extract verify "/path/to/Legend of Legaia (USA).bin"
```

A good USA dump prints:

```
[ok] matches: Legend of Legaia (USA) - SCUS-94254
```

An unrecognized dump (a PAL disc, a re-track, a bad rip) prints an `[unknown]`
line with the computed SHA-256 instead of failing - the extraction tools still
run on it, but the fingerprints, offsets, and the randomizer's patches all
target the USA build. A file that is not a Mode 2/2352 image at all is
rejected up front with `not a Mode2/2352 disc image: <path>`.

## 3. Extract everything in one shot

```bash
./legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

On a modern machine this takes about five seconds and writes roughly 1.1 GB.
The pipeline runs eight steps: disc verify, ISO9660 walk (the disc's 45
files), `PROT.DAT` split (1233 entries, named via `CDNAME.TXT`), per-entry
format categorization, streaming sub-asset extraction, streaming-TIM PNG
conversion, the TIM-catalog TSVs, CD-XA demux to per-channel WAVs (316 of
them), and finally the dialog-font artifacts. Skip flags exist for each slow
or optional step: `--skip-verify`, `--skip-png`, `--skip-xa`,
`--skip-catalog`, `--skip-font`.

What lands where:

```
extracted/
├── PROT.DAT                 - the game's main archive, raw copy
├── CDNAME.TXT               - entry name map
├── SCUS_942.54              - the game executable (data-table source)
├── MOV/                     - FMV movies (MV1.STR ...)
├── PROT/                    - one file per PROT entry, plus categorize.json
├── streaming/               - sub-assets unpacked from streaming containers
├── XA_WAV/                  - streamed audio, one WAV per channel
├── font/                    - dialog-font atlas + widths (engine + viewer read this)
├── prot_tim_catalog.tsv     - texture inventory (raw TIMs)
└── prot_tim_deep_catalog.tsv - texture inventory (TIMs inside LZS)
```

The final summary points at the texture path: bulk texture export is a
separate two-command step (`asset tim-scan` + `tim convert-dir`) covered in
[extracting-assets.md](extracting-assets.md).

Everything downstream - the [asset scenarios](extracting-assets.md), the
[engine and viewers](playing-and-viewing.md), the
[modding tools](modding-and-translation.md) - reads either this `extracted/`
tree or the disc image directly.

## 4. The tools at a glance

Pipeline and per-format extractors:

| Binary | What it's for |
|---|---|
| `legaia-extract` | The one-shot pipeline: disc image → the whole `extracted/` tree. Start here. |
| `disc-extract` | Verify a dump's fingerprint; list / extract the raw ISO9660 files. |
| `prot-extract` | Split `PROT.DAT` into its numbered entries with CDNAME-derived names. |
| `lzs-decode` | Decompress Legaia's LZS streams and `.lzs` containers. |
| `asset` | The format hub: categorize entries, extract sub-assets, dump readable game-data tables, export monsters to glTF. |
| `tim` | TIM textures → PNG. |
| `tmd` | Legaia TMD meshes → Wavefront OBJ. |
| `vab` | VAB sound banks → VAG samples / WAV. |
| `seq` | SEQ music inspector: headers, event disassembly, `find` for wrapped BGM. |
| `xa` | Streamed CD-XA audio → correctly-paced per-channel WAVs. |
| `mdec` | FMV decoder: STR movie data → per-frame PNGs. |
| `font-extract` | Dialog-font atlas + width table, straight from the disc. |

Game-data, playing, and modding:

| Binary | What it's for |
|---|---|
| `legaia-engine` | The clean-room engine: play scenes, FMVs, record/replay - straight from your disc. |
| `asset-viewer` | Windowed browser for textures, meshes, audio banks, and scene demos. |
| `legaia-patcher` | Disc patcher: randomizer, translation toolchain, and manual record edits; emits shareable PPF patches. |
| `save-tool` | PSX memory-card / save inspector: character records, save-block diffs. |
| `gamedata-tool` | Curated game-data lookups (arts, items, shops, enemies, ...); needs no disc at all. |
| `cheat-tool` | GameShark cheat-database parser with the Legaia NTSC-U databases built in. |

Reverse-engineering aids (useful once you go deeper):

| Binary | What it's for |
|---|---|
| `field-disasm` | Field/event-VM script disassembler over `PROT.DAT`. |
| `mes` | MES dialog-container inspector (for readable game text, use `legaia-patcher translate export` instead). |
| `anm` | ANM animation-container inspector. |
| `mdt` | Move-table (Tactical Arts) layout classifier. |
| `art` | Tactical Arts tables: action constants, arts names, Super/Miracle Art triggers. |
| `mednafen-state` | Mednafen save-state inspector: RAM diffs, VRAM dumps, SPU snapshots. |

## Related docs

- [extracting-assets.md](extracting-assets.md) - step-by-step asset scenarios.
- [playing-and-viewing.md](playing-and-viewing.md) - the engine and viewers.
- [modding-and-translation.md](modding-and-translation.md) - randomizer, translation, saves.
- [docs/formats/overview.md](../formats/overview.md) - byte-level format specs behind every parser.
- [docs/tooling/extraction.md](../tooling/extraction.md) - per-stage pipeline reference.
