# legaia-asset

The home of the `asset` command-line tool, and nothing else: the package has
no library target, so no crate can depend on it. Each subcommand is a thin
driver over the crate that owns the format it reads.

| Crate | Holds |
|---|---|
| [`legaia-container-formats`](../container-formats/README.md) | Asset-type dispatcher, descriptor, DATA_FIELD stream, pack, container-level detectors and scanners, the sound containers. |
| [`legaia-scene-formats`](../scene-formats/README.md) | Scene bundles, MAN, the field-VM disassembler, field objects and maps, world-map formats. |
| [`legaia-minigame-formats`](../minigame-formats/README.md) | The minigames' tables, art packs and scene. |
| [`legaia-ui-formats`](../ui-formats/README.md) | Title / system-UI packs, glyph atlas, save icons, screen-element and widget tables. |
| [`legaia-actor-formats`](../actor-formats/README.md) | Monster archive, player battle files, the field character pack, their animation and glTF export. |
| [`legaia-game-tables`](../game-tables/README.md) | Decoded data tables from `SCUS_942.54` and the overlays. |
| [`legaia-overlay-images`](../overlay-images/README.md) | Code images: detection, load bases, slot-B module layout. |
| [`legaia-disc-audit`](../disc-audit/README.md) | Byte accounting, classification, TIM catalogs, censuses. |

Each of those READMEs carries the per-module reference for what it owns:
entry points, the `asset` subcommand that drives it, and the format page.

The static-overlay map `data/static-overlays.toml` stays in this crate - the
analysis scripts and the docs open it by that path - and is embedded by
`legaia-overlay-images`.

The package's tests are the ones that spawn the `asset` binary; a format's
own tests sit in the crate that owns the format.

## CLI

```bash
asset describe         <input>            # parse + print descriptor
asset decode           <input> --type-size 0xTTSSSSSS --offset 0xNN [--mode lzs|raw] [-o <out>]
asset scan             <PROT dir>         # entries that fully decode as player.lzs containers
asset stream / scan-stream                # streaming-container chunk list / PROT-wide sweep
asset categorize       <PROT dir> [--cdname <CDNAME.TXT>]   # e.g. extracted/PROT
asset account          <entry.BIN|index> [--funcs <dir>] [--depth N] [--json]
asset disc-map         <disc.bin> [--out <f.json>]   # layout metadata for the site's disc map (site/disc-map.json)
asset find-overlay     <PROT dir>         # MIPS-code candidate scan
asset overlay          list|extract|verify|ghidra|scan|find-sig|generate   # static overlay pipeline
asset tim-scan         <PROT dir>         # locate embedded TIMs (per-entry, lenient)
asset tim-catalog      <PROT.DAT>         # flat strict TIM catalog (--out f.tsv|f.json, --rollup)
asset tim-deep-catalog <PROT.DAT>         # TIMs inside LZS-compressed sections (--out, --rollup)
asset battle-texture-catalog <PROT.DAT>   # headerless 4bpp battle art in PROT 863..866 (--scus for names, --out, --rollup)
asset tim-render-distinct <PROT.DAT> --out <dir>  # decode each distinct TIM to PNG (local only; drives tim_labels)
asset tmd-scan         <PROT dir>         # locate embedded TMDs
asset clut-finder     <x> <y>            # which entry's TIM supplies a VRAM CLUT cell
asset stage / stage-scan
asset field-pack / field-pack-scan
asset effect-bundle / effect-bundle-scan
asset battle-data-pack / battle-data-pack-scan
asset befect-cluster   <PROT.DAT> --cdname <CDNAME.TXT> --out <dir>
asset monster-archive  <PROT 0867 .BIN> [--id N --obj/--texture-png/--anim/--glb/--dump-block/--write-block]
asset character-pack / battle-char-pack / field-char-tex
asset player-anm / player-anm-scan
asset scene-v12 / scene-v12-scan
asset man / man-scan                      # MAN multi-section walker (--with-encounter)
asset field-disasm / field-op-census      # field-VM disassembler + disc-wide opcode census
asset boot-overlay     [--prot-dir extracted/PROT]  # which entry each boot loader param names
asset slot-scene       <PROT 0975 .BIN> [--art <PROT 1200 .BIN>]  # reel cylinder + dot marquee
asset slot-art         <PROT 0975 .BIN> <PROT 1200 .BIN> [--out <dir>]  # slot art + data as PNGs + JSON (VRChat kit)
asset kingdom-slot / slot4-png            # world-map kingdom bundles
asset summon-overlay   <PROT 0905 .BIN>
asset summon-readef    <PROT 0893|0894 .BIN> [--texture-png-dir <dir>]   # side-band streaming slots
asset move-power / element-affinity / seru-side-effect   # PROT 0898 battle-overlay tables (--json)
asset formation-census <PROT.DAT|PROT dir> [--json]   # per-monster formation rows, scripted vs random
asset summon-creatures                    # summon -> creature map (--scus, --json)
asset mode-table / worldmap-menu / item-tables   # SCUS_942.54 static tables
asset shop-stock                          # every town gold shop's stock (MAN op-0x49 sites) joined to item names + prices
asset spell-names / steal-table / accessory-passive   # more SCUS_942.54 tables (--json)
asset sfx-table / new-game / level-up             # more SCUS_942.54 tables (--json)
asset extract          <entry.BIN> --out <dir>   # unpack a streaming entry's TIM_LIST / TMD chunks
asset validate         <PROT dir>         # strict checks on each CDNAME block's first entry
```

`asset --help` lists the rest. `categorize` is the one most other tools
key off - its JSON output drives the asset-viewer's "browseable PROT
entry list" and the cross-reference scripts under [`scripts/`](../../scripts/).

## See also

- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  - full asset-loader chain from disc to dispatch.
- [`docs/formats/`](../../docs/formats/overview.md) - per-format byte-level
  specs the format crates implement.
