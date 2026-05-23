# Sound-driver path-string cluster

The SCUS string cluster at RAM `0x8007B380` (file offset `0x6BB80`) holds the file extensions the sound subsystem appends to scene-asset paths. The dispatch chain *into* these formats is fully traced; the byte-level layout of the individual files is still TBD.

## Cluster layout

| Offset | Bytes | Meaning |
|---|---|---|
| `0x8007B380` | 12 bytes | Per-extension flag/mode metadata table: `00 01 FF FF FF FF 00 00 00 02 00 00`. |
| `0x8007B38C` | `"sound\"` | Path prefix for streaming-asset loads. |
| `0x8007B394` | `".spk"` | SPU sample bank. |
| `0x8007B39C` | `".LZS"` | Compressed wrapper (per-file). |
| `0x8007B3A4` | 8 bytes | Two 4-byte mode descriptors: `00 00 00 00` and `03 02 03 00 03 05 03 00`. Indexed by `FUN_8001EBEC` to pick which extension a given mode uses. |
| `0x8007B3AC` | `"bse.dat"` | Master sound-bank file name (loaded once at sound-init). |
| `0x8007B3B4` | `".dpk"` | Per-scene sound pack - the format `FUN_8001FA88` loads. |
| `0x8007B3BC` | `".MAP"` | Sound bank map (PsyQ SoundArtist output). |
| `0x8007B3C4` | `".PCH"` | Patch / instrument data (PsyQ output). |
| `0x8007B3CC` | `".LZS"` | Duplicate, fallback. |
| `0x8007B3D4` | `".pac"` | Per-scene generic pack. |
| `0x8007B3DC` | `"STR"` | Streaming audio (raw PSX `.STR` containers). |

## Consumers

Three SCUS functions touch the cluster (located via [`ghidra/scripts/find_sound_path_builders.py`](../../ghidra/scripts/find_sound_path_builders.py)):

| Function | Role | Touch points |
|---|---|---|
| `FUN_8001FA88` | **Sound subsystem init / `.dpk` loader.** Allocates a 0x1800-byte buffer at `_DAT_8007B8D0`. Loads `bse.dat` via the path-based opener. Then assembles `h:\main\bg\domepack\<name>.dpk` (template at `0x800105C8` + extension at `0x8007B3B4`) and opens it. | `0x8007B3AC` (`bse.dat`), `0x8007B3B4` (`.dpk`) |
| `FUN_8001FC00` | **Streaming-asset loader.** Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. | `0x8007B38C` (`sound\`) |
| `FUN_8001EBEC` | **Mode-aware extension dispatcher.** Reads `_DAT_8007B824` as a mode index, then uses the small per-mode tables at `0x8007B3A4` / `0x8007B3A8` to pick which file-extension entry to hit. | `0x8007B3A4`, `0x8007B3A8` |

## Dev-build vs retail-build paths

Both `FUN_8001FA88` and `FUN_8001FC00` carry a `_DAT_8007B8C2` (debug-flag) carve-out:

- **Dev branch** (`_DAT_8007B8C2 != 0`) loads sound data via PROT indices. `FUN_8001FA88`'s dev branch loads index `0x37A` (= `sound_data2`) plus `param_1 + 5` for per-scene variations, via the index-based loader (`FUN_8003EB98`).
- **Retail branch** (`_DAT_8007B8C2 == 0`) loads via dev-style paths through the path-based opener (`FUN_8003E6BC`), which resolves `h:\main\bg\domepack\…` into the appropriate PROT entry through the CDNAME-driven name map.

Both paths land at the same files; only the indirection differs. The same dev/retail split appears in [`FUN_800255B8`](../subsystems/asset-loader.md), so it's a pattern that repeats across asset-loading subsystems.

## What's left to format-spec

The byte-level layouts of the individual files (`.MAP` / `.PCH` / `.spk` / `.dpk` / `.pac`) are still TBD. With consumers identified, the next move is to read the body of `FUN_8001FA88` (specifically the field accesses on `_DAT_8007B8D0` after the path-based opener returns) for the `.dpk` byte layout - `_DAT_8007B8D0 + 2` is read as a `ushort` and used as a divisor (almost certainly a record count).

The eventual home is a `crates/sound` companion to `crates/vab`.

## See also

- [VAB sound bank](vab.md) - the instrument-bank format these drivers emit.
- [SEQ sequence](seq.md) - the sequenced-music format played against the banks.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ audio stack.
