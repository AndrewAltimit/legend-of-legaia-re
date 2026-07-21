# Extracting assets: step-by-step scenarios

Every scenario below assumes the one-shot pipeline has run
([getting-started.md](getting-started.md)):

```bash
./legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

Commands use the bare `./tool` form from a release archive; source builds live
at `target/release/`. Paths like `extracted/...` resolve against the current
directory - run from the same place each time.

## Textures → PNG

The bulk texture path is a two-command chain: sweep every PROT entry for TIM
images (raw and inside LZS compression), then convert the hits.

```bash
./asset tim-scan extracted/PROT --out extracted/tim_scan
./tim convert-dir extracted/tim_scan
```

`tim_scan/` gets one directory per PROT entry holding files named
`raw_off<HEX>.tim` (found in the raw bytes) or `lzs<i>_off<HEX>.tim` (found
inside LZS section `i`); `convert-dir` writes a `.png` next to each. For a
single texture:

```bash
./tim convert extracted/tim_scan/<entry>/raw_off<HEX>.tim -o out.png
./tim convert <file>.tim --all-cluts        # one PNG per palette row
```

The filename is how a texture-catalog label maps to a file on disk. A label
like `PROT 46 · LZS sec 0 +0x13368 · 128×128 4bpp · foliage` reads as: PROT
entry 46 (`0046_vell`), inside LZS section 0, at offset `0x13368`. So after
the sweep it lands at `extracted/tim_scan/0046_vell/lzs0_off13368.tim` -
convert that one file. `tim-scan` does the LZS decompression itself, so this
is the whole texture path; you never need to decompress sections by hand
(the manual-chain section below is for the rarer non-texture cases).

Many TIMs carry several palettes (CLUTs) - a character atlas decoded with the
wrong row looks like static, so try `--all-cluts` before assuming a texture is
broken. The strict per-texture inventories the pipeline already wrote
(`prot_tim_catalog.tsv`, `prot_tim_deep_catalog.tsv`) map every TIM to its
owning entry and offset. Format: [tim.md](../formats/tim.md).

## 3D models → OBJ

Legaia's meshes are a custom TMD variant ([tmd.md](../formats/tmd.md)). Sweep
and export:

```bash
./asset tmd-scan extracted/PROT --out extracted/tmd_scan
./tmd dump-obj extracted/tmd_scan/<entry>/raw_off<HEX>.tmd --out mesh
```

`dump-obj` writes `mesh_obj0.obj` etc. - vertices *and* faces, decoded through
the Legaia-specific primitive walker. Note: `tmd info` prints a
`psx-walk: FAIL` line on every valid Legaia TMD - that is the diagnostic
confirming the file is the Legaia variant rather than a standard PsyQ TMD, not
an error.

## Monsters → glTF (opens in Blender)

The monster archive (PROT entry `0867_battle_data`) carries each enemy's mesh,
texture page, and battle animations. One command bundles all three into a
`.glb`:

```bash
./asset monster-archive extracted/PROT/0867_battle_data.BIN          # list all 186
./asset monster-archive extracted/PROT/0867_battle_data.BIN --id 1 --glb monster1.glb
```

The listing prints every monster's name and battle stats; the `.glb` drags
straight into Blender (or any glTF viewer) with geometry, material, and
animation tracks intact. `--obj` / `--texture-png` export the pieces
separately; `--anim` lists the decoded actions. Formats:
[battle-data-pack.md](../formats/battle-data-pack.md),
[monster-animation.md](../formats/monster-animation.md).

Enemy stats are **not** LZS-compressible text you can spot in a hex editor,
and they are **not** in the player-battle files `0865`/`0866`. If you open
those in a hex editor you will see enemy names like `Evil Fly` near the end -
that is an *over-read* (see below), not their real home. Always read enemy
data through `asset monster-archive` on entry `0867`, which starts each
monster's LZS stream at the right slot base; a raw `lzs-decode` of the whole
file only expands the first stream it meets.

## Streamed audio (voice + ambience) → WAV

The pipeline already demuxes this into `extracted/XA_WAV/`. To run just that
step - it reads the raw disc, not the extracted tree:

```bash
./xa demux-disc-all "/path/to/Legend of Legaia (USA).bin" --out extracted/XA_WAV
```

About three seconds on a modern machine for the disc's 34 `.XA` files → 316
per-channel WAVs, each decoded at the true sample rate and stereo mode read
from its CD-XA subheaders. Prefer this over `xa convert`, which operates on
Form-1 dumps and has to guess the rate. Format: [xa.md](../formats/xa.md).

## Sound banks (instruments + SFX) → WAV

VAB banks hold the sampled instruments and sound effects
([vab.md](../formats/vab.md)):

```bash
./vab list    extracted/PROT/<entry>.BIN
./vab extract extracted/PROT/<entry>.BIN --out vab_out --wav
```

On the wrapped BGM entries (the `music_01` block) the scan warns about a
truncated trailing header after the real banks extract - that is expected on
those entries, and the exit code stays 0.

## Background music (SEQ) - the wrapped-BGM story

Retail BGM is not a bare SEQ file: each music PROT entry wraps it as
`[chunk header][VAB][chunk header][SEQ]`, so the SEQ sits at a non-zero
offset. `seq find` locates it:

```bash
./seq find extracted/PROT/0990_music_01.BIN
./seq info extracted/PROT/0990_music_01.BIN --offset 0x<from-find>
./seq events <file> --offset 0x<N>       # full event disassembly
./seq json   <file> --offset 0x<N>       # machine-readable parse
```

`find` scans for `pQES` magics and reports each candidate offset with whether
the full event stream parses. Without `--offset`, `info`/`events`/`json` try
offset 0 and then fall back to the first parseable magic automatically - so
`./seq info extracted/PROT/0990_music_01.BIN` alone also works; the explicit
offset just makes the choice visible. Format: [seq.md](../formats/seq.md);
which track is which: [music-tracks.md](../reference/music-tracks.md).

## Movies → PNG frames

The disc's FMVs land in `extracted/MOV/`. Decode them frame-by-frame:

```bash
./mdec scan-str   extracted/MOV/MV1.STR                  # list frames + dimensions
./mdec decode-str extracted/MOV/MV1.STR -o frames/       # frame_0001.png, ...
```

PNG is the default output (`--format ppm` for the raw variant);
`--max-frames N` stops early. To *watch* a movie with its audio instead, use
`legaia-engine play-str` ([playing-and-viewing.md](playing-and-viewing.md)).
Background: [cutscene.md](../subsystems/cutscene.md).

## Game-data tables from the executable

The game executable `extracted/SCUS_942.54` carries the static stat tables.
The `asset` hub prints them as readable, joined listings:

```bash
./asset item-tables extracted/SCUS_942.54     # item names + effects/stat bonuses
./asset spell-names extracted/SCUS_942.54     # spells/arts, MP cost, target
./asset steal-table extracted/SCUS_942.54     # what each monster can have stolen
./asset new-game    extracted/SCUS_942.54     # starting party + inventory
```

Siblings: `accessory-passive`, `sfx-table`, `level-up`, `worldmap-menu`.
Formats: [item-table.md](../formats/item-table.md),
[spell-table.md](../formats/spell-table.md),
[steal-table.md](../formats/steal-table.md),
[new-game-table.md](../formats/new-game-table.md).

## The dialog font

The pipeline's final step writes `extracted/font/` (atlas PNG, tile-page
sheet, widths CSV, metadata) - the engine and asset-viewer load text from it.
To rebuild it alone, no emulator needed:

```bash
./font-extract --disc "/path/to/Legend of Legaia (USA).bin" --out extracted/font
```

`--disc` also accepts an already-extracted `PROT.DAT`. The alternative
`--save` mode reads a mednafen save state's live VRAM instead. Format:
[dialog-font.md](../formats/dialog-font.md).

## The manual chain (when you want one stage at a time)

`legaia-extract` is these stages composed; each also runs standalone. The
first two build the `extracted/` tree; the rest operate on one PROT entry at
a time:

```bash
./disc-extract extract "/path/to/disc.bin" extracted/
./prot-extract extract extracted/PROT.DAT extracted/PROT --cdname extracted/CDNAME.TXT
./asset categorize extracted/PROT                            # classify every entry (run this first)
./lzs-decode probe     extracted/PROT/<entry>.BIN            # is it an LZS container?
./lzs-decode container extracted/PROT/<entry>.BIN out_dir/   # decompress every section
./asset decode  extracted/PROT/<entry>.BIN --type-size 0x.. --offset 0x.. --mode lzs  # one section
./asset stream  extracted/PROT/<entry>.BIN                   # list a streaming container's chunks
./asset extract extracted/PROT/<entry>.BIN --out out_dir/    # unpack a streaming container's sub-assets
```

### Which `<entry>` do I pass?

`<entry>` is a filename from `extracted/PROT/`, and every file there is named
`NNNN_<name>.BIN` where `NNNN` is the zero-padded PROT index. A catalog or
site label that reads `PROT 46` means entry **46**, i.e. `0046_vell.BIN` -
not `0446`. The four-digit prefix *is* the index; don't add or drop digits.
When a label and a filename disagree on the `<name>` half, trust the number:
CDNAME names are shifted +2 against the `#define` numbers
([cdname.md](../formats/cdname.md#numbering-space)).

### Which command for which entry?

Run `asset categorize` first - it prints each entry's format class, so you
know whether an entry is an LZS container, a streaming/pack bundle, a raw
scene bundle, or overlay code before you pick a decoder. Then:

- **LZS container** - `lzs-decode container` dumps *every* section to
  `out_dir/`. `asset decode` pulls a *single* section instead: take the
  `--type-size` and `--offset` from an `asset describe` row of the same
  file, and pass `--mode raw` for the small uncompressed control sections
  (`--mode lzs` for the compressed ones). Reach for `decode` when you want
  one section, or when the container header table doesn't auto-walk cleanly.
- **Streaming/pack bundle** - `asset stream` lists its chunks and
  `asset extract` unpacks the TIM/TMD sub-assets. Both expect the DATA_FIELD
  chunk format; pointed at a flat blob (e.g. a decompressed LZS section)
  they misread the leading bytes as a chunk header and print
  `magic MISMATCH` on every "chunk" - that means it is the wrong tool for
  that input, not a corrupt file.
- **Textures, in any container** - skip the section plumbing and use
  `asset tim-scan` ([Textures → PNG](#textures--png)); it decompresses LZS
  sections itself and writes each TIM as `lzs<i>_off<HEX>.tim`.

One trap worth knowing: LZS "decompresses without error" is **not** a
validity signal - the decoder's ring buffer initialises to zeros, so most
random input decodes to plausible-looking bytes. Always check the *decoded*
output for the expected magic ([lzs.md](../formats/lzs.md)). This is why
`lzs-decode probe` reports `no plausible sections` on a non-container entry
rather than emitting garbage - a clean "this isn't LZS" is the useful answer.

### The over-read trap, and which file owns a byte offset

Some PROT entries declare a TOC window far larger than their real on-disc
footprint (the sector gap to the next entry). `prot-extract` writes the full
declared window, so those `.BIN` files carry the *next* entry's bytes in their
tail. The clearest case is the battle-data cluster: entries `0865` (Gala) and
`0866` (Terra) each declare ~16 MB but really own `0x6F000` / `0x17800`, and
their oversized windows both spill into the monster archive `0867`. That is
why enemy text shows up at the end of a player-battle file.

`prot-extract list` now flags every such entry with an `OVR` column and shows
its true `footprint` next to the declared `decl_size`. To resolve a specific
offset - "which file really owns the bytes my hex editor is showing at
`0x17855` of `0866`?" - use `locate`:

```bash
prot-extract locate extracted/PROT.DAT 0x17855 --in-entry 866 --cdname extracted/CDNAME.TXT
```

It translates the in-file offset to an absolute PROT.DAT offset, names the
**true owning entry** (here `0867`), and warns when the offset lands in an
over-read tail rather than the entry's own data. Drop `--in-entry` to pass an
absolute PROT.DAT offset directly.

To avoid the trap entirely, re-extract with `prot-extract extract
--clamp-footprint`: every `.BIN` is trimmed to its true footprint, so no file
carries a neighbour's bytes (trailing overlays are kept - they sit inside the
footprint). The default stays un-clamped because in-`.BIN` offsets then match
the TOC-declared windows that `locate --in-entry` and older notes assume.

## Related docs

- [docs/tooling/extraction.md](../tooling/extraction.md) - the full per-stage reference.
- [docs/formats/overview.md](../formats/overview.md) - every format spec, with confidence levels.
- [playing-and-viewing.md](playing-and-viewing.md) - browse what you extracted, interactively.
