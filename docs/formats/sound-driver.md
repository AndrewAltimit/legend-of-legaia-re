# Sound-driver path-string cluster

The SCUS string cluster at RAM `0x8007B380` (file offset `0x6BB80`) holds the file extensions the sound subsystem appends to scene-asset paths. The dispatch chain *into* these formats is fully traced; the byte-level layout of the individual files is still TBD.

## Cluster layout

| Offset | Bytes | Meaning |
|---|---|---|
| `0x8007B380` | 12 bytes | Per-extension flag/mode metadata table: `00 01 FF FF FF FF 00 00 00 02 00 00`. |
| `0x8007B38C` | `"sound\"` | Path prefix for streaming-asset loads. |
| `0x8007B394` | `".spk"` | SPU sample bank. |
| `0x8007B39C` | `".LZS"` | Compressed wrapper (per-file). |
| `0x8007B3A4` | 8 bytes | **Not sound data.** This window holds the per-character equipment-swap selector bytes (3 equip-condition offsets at `0x8007B3A4` + 3 group indices at `0x8007B3A8`) consumed by the graphics-side `FUN_8001EBEC` - see [character-mesh.md § equipment-conditional group-transform swap](character-mesh.md#10-group-cap--equipment-conditional-swap). Adjacent to the path cluster in BSS but unrelated to it. |
| `0x8007B3AC` | `"bse.dat"` | Master sound-bank file name (loaded once at sound-init). |
| `0x8007B3B4` | `".dpk"` | Per-scene sound pack - the format `FUN_8001FA88` loads. |
| `0x8007B3BC` | `".MAP"` | Sound bank map (PsyQ SoundArtist output). |
| `0x8007B3C4` | `".PCH"` | Patch / instrument data (PsyQ output). |
| `0x8007B3CC` | `".LZS"` | Duplicate, fallback. |
| `0x8007B3D4` | `".pac"` | Per-scene generic pack. |
| `0x8007B3DC` | `"STR"` | Streaming audio (raw PSX `.STR` containers). |

## Consumers

Two SCUS functions touch the cluster (located via [`ghidra/scripts/find_sound_path_builders.py`](../../ghidra/scripts/find_sound_path_builders.py); a third candidate the locator flagged, `FUN_8001EBEC`, turned out not to be a sound consumer - see the correction below):

| Function | Role | Touch points |
|---|---|---|
| `FUN_8001FA88` | **Sound subsystem init / `.dpk` loader.** Allocates a 0x1800-byte buffer at `_DAT_8007B8D0`. Loads `bse.dat` via the path-based opener. Then assembles `h:\main\bg\domepack\<name>.dpk` (template at `0x800105C8` + extension at `0x8007B3B4`) and opens it. | `0x8007B3AC` (`bse.dat`), `0x8007B3B4` (`.dpk`) |
| `FUN_8001FC00` | **Streaming-asset loader.** Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. | `0x8007B38C` (`sound\`) |

> **Correction:** `FUN_8001EBEC` was previously listed here as a "mode-aware extension dispatcher" reading `_DAT_8007B824` + tables at `0x8007B3A4`/`0x8007B3A8`. The decomp shows it is **not** part of the sound subsystem at all - it is the character-TMD equipment-conditional group-transform swap (it reads the loaded battle-character TMD pointers from `DAT_8007C018[_DAT_8007B824 + 0..2]`, where `_DAT_8007B824` is the party base index, and copies group transforms). The `0x8007B3A4`/`0x8007B3A8` bytes are its per-character selector tables, not sound-extension descriptors. See [character-mesh.md](character-mesh.md#10-group-cap--equipment-conditional-swap).

## Dev-build vs retail-build paths

Both `FUN_8001FA88` and `FUN_8001FC00` carry a `_DAT_8007B8C2` (debug-flag) carve-out:

- **Dev branch** (`_DAT_8007B8C2 != 0`) loads sound data via PROT indices. `FUN_8001FA88`'s dev branch loads index `0x37A` (= `sound_data2`) plus `param_1 + 5` for per-scene variations, via the index-based loader (`FUN_8003EB98`).
- **Retail branch** (`_DAT_8007B8C2 == 0`) loads via dev-style paths through the path-based opener (`FUN_8003E6BC`), which resolves `h:\main\bg\domepack\…` into the appropriate PROT entry through the CDNAME-driven name map.

Both paths land at the same files; only the indirection differs. The same dev/retail split appears in [`FUN_800255B8`](../subsystems/asset-loader.md), so it's a pattern that repeats across asset-loading subsystems.

## `bse.dat` master-bank header (from `FUN_8001FA88`)

`FUN_8001FA88`'s body (`ghidra/scripts/funcs/8001fa88.txt`) loads the master sound bank `bse.dat` into the 0x1800-byte buffer `_DAT_8007B8D0`, then derives a second pointer from a single `u16` at offset `+2`:

```text
count     = *(u16 *)(bse_base + 2)
gp[0x678] = bse_base + (count & ~1)        ; (count/2)*2 -- an even byte offset
```

So `bse.dat` leads with `[u16 @0][u16 @2 = even byte-offset to a second section][section A ...][section B at bse_base + (count & ~1)]`. The earlier "`+2` is a record-count divisor" reading was imprecise: the value is used as a **byte offset** that splits `bse.dat` into two sections (the `>>1 … *2` is a round-to-even, not a division of the data), and it indexes into the once-loaded master bank, **not** the per-scene `.dpk`.

## `.dpk` is a streaming-chunk container

`FUN_8001FA88` loads the per-scene `.dpk` into the caller's buffer (`param_3`) and does **not** read it; the **consumer** is `FUN_8001FE70`, which `FUN_800513F0` calls on that same buffer right after the load (`ghidra/scripts/funcs/800513f0.txt:405-413` passes `gp[0xA8C]` to both). `FUN_8001FE70` (`ghidra/scripts/funcs/8001fe70.txt`) is the generic **streaming-chunk walker** the asset dispatcher already models (`legaia_asset` lib docs; `[u32 (type << 24) | (size & 0xFFFFFF)][size bytes, 4-padded]` chunks, advance by `(size & ~3) + 4`, type `1` → `FUN_800198E0`, type `2` = terminator). So the `.dpk` / `sound_data2` per-scene sound pack **is that container**, not a novel layout.

Empirically confirmed against the disc: PROT `0877` (`sound_data2`) walks cleanly as `[type 0: 0x2820][type 1: 0x1BA90][type 2: terminator]` and the type-`2` marker lands inside the file - the same shape `FUN_8001FE70` expects. (`FUN_8001FA88`'s dev branch loads PROT `0x37A` = `sound_data2`, tying the `.dpk` filename to this PROT family.) The difference from the DATA_FIELD walker is **only the terminator**: `FUN_8001FE70` ends on a type-`0x02` chunk (whose `size` is non-zero), whereas `FUN_8002541C` ends on a zero-`size` header - so the DATA_FIELD parser mis-reads a `.dpk`. The walker is `legaia_asset::parse_streaming_with(buf, max, StreamTerminator::TypeTwo)`; the disc-gated `sound_pack_stream_real` test pins the clean type-2 walk of PROT `0877`.

## The `.dpk` / `sound_data2` payload is a VAB + SEQ bundle

The sound-side chunk payloads are now pinned from the disc - the pack is a
**VAB followed by a SEQ**, not a novel `.MAP`/`.PCH`/`.spk` layout:

| Chunk type | Payload | Magic |
|---|---|---|
| `0` | VAB **header** section | `pBAV` (`0x5641_4270` LE) |
| `1` | VAB **sample** section (SPU-ADPCM / VAG waveform pool) | — (raw samples) |
| `2` | **SEQ** (the stream terminator) | `pQES` |

The decisive invariant: **type-0 + type-1 reconstitute one contiguous VAB**
whose header `total_size` (`+0x0C`) equals `chunk[0].size + chunk[1].size`.
Byte-verified across `0877`..=`0885` (e.g. `0877`: `0x2820 + 0x1BA90 ==
0x1E2B0`); the reconstituted VAB parses with `legaia_vab` and the terminator
SEQ with `legaia_seq`.

This settles the earlier open question about the **type-1 chunk payload**. On
the *graphics* battle-init walk, `FUN_8001FE70`'s type-1 handler is the
TIM/CLUT upload (`FUN_800198E0`) - so a sound pack's type-1 could not be a
TIM, and it isn't: it is the VAB's sample pool. The content shape is identical
to the [`scene_vab_stream`](scene-bundles.md) BGM wrapper (VAB + SEQ), only the
container differs (type-2-terminated streaming chunks vs the chunk0-prefixed
form). Decoder: [`legaia_asset::sound_pack`](../../crates/asset/src/sound_pack.rs)
(`extract` returns the reconstituted VAB + the SEQ); disc-gated
`sound_pack_vab_seq_real` test. (An even earlier interim trace mislabelled the
`.dpk` as a *bare* VAG-record container - that remains **falsified**; the VAG
samples are the type-1 section *of a VAB*, indexed by the type-0 header, not a
header-less stream.)

The remaining residual is narrow: the `.MAP` (sample-address map) and `.PCH`
(patch / program data) PsyQ SoundArtist intermediates are the dev-side inputs
that *produce* this VAB; they are not separate on-disc retail chunks here. A
`crates/sound` companion to `crates/vab` is only warranted if those dev
intermediates turn up in a build, which the retail disc does not carry.

## See also

- [VAB sound bank](vab.md) - the instrument-bank format these drivers emit.
- [SEQ sequence](seq.md) - the sequenced-music format played against the banks.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ audio stack.
