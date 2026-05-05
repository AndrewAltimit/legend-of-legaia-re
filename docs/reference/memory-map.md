# RAM map + key globals

PSX RAM is 2 MB total at KSEG0 base `0x80000000`. Legaia's runtime layout:

```
0x80000000 — 0x8000FFFF    BIOS scratchpad area (kernel + thread state)
0x80010000 — 0x800FFFFF    SCUS_942.54 code + data (~960 KB)
0x80100000 — 0x801BFFFF    runtime data buffers (asset slabs, character struct, save state)
0x801C0000 — 0x801FFFFF    overlay window (256 KB, see "Overlays" below)
0x80200000+                 extended overlay region
```

Plus the PSX-specific scratchpad at `0x1F800000-0x1F8003FF` (1 KB) which Legaia uses for global story flags and a few per-frame transients.

## Static (`SCUS_942.54`-resident) globals

| Address | Type | Purpose |
|---|---|---|
| `0x800840F8` | u32 | BIOS pad data (read by `FUN_8001822C`). |
| `0x80084340` | inventory base | Per-page inventory state, 0x414-byte stride. |
| `0x80084540` | u16 | Current map / scene PROT base index. |
| `0x80084594` | u8 | Party member count. |
| `0x80084598` | u8[] | Party member IDs (sorted insertion, cap 4). |
| `0x80084628` | i16 | Set by op 0x4C nibble-8 sub-8. |
| `0x80086D70` | u8[32] | **Fourth flag bank** — 256-bit bitfield, accessed via SET / CLEAR / TEST `(idx >> 3, 0x80 >> (idx & 7))`. |
| `0x80087AF8` | u32 | Result of `FUN_80020224` descriptor walker, set by town-overlay MAIN INIT. |
| `0x800845DC` | (mirror of `_DAT_80084570`) | Snapshot written by op 0x4C nibble-E sub-E. |
| `0x800845A4` | u32 | Party-money / XP bank. |

## Sound + audio path

| Address | Purpose |
|---|---|
| `0x8007B380` | 12-byte per-extension flag/mode metadata table. |
| `0x8007B38C` | Path prefix `"sound\"` for streaming-asset loads. |
| `0x8007B394` | `".spk"` extension. |
| `0x8007B39C` | `".LZS"`. |
| `0x8007B3A4` | Two 4-byte mode descriptors used by `FUN_8001EBEC`. |
| `0x8007B3AC` | `"bse.dat"` master file name. |
| `0x8007B3B4` | `".dpk"`. |
| `0x8007B3BC` | `".MAP"`. |
| `0x8007B3C4` | `".PCH"`. |
| `0x8007B3D4` | `".pac"`. |
| `0x8007B3DC` | `"STR"`. |
| `0x8007B7F8` | sin lookup table. |
| `0x8007B81C` | cos lookup table. |
| `0x8007B824` | u32 | Mode index read by `FUN_8001EBEC`. |
| `0x8007B840` | MOVE2 buffer base. |
| `0x8007B888` | MOVE buffer base. |
| `0x8007B8D0` | u32 | `bse.dat` master bank pointer (0x1800-byte buffer). |
| `0x8007BAC8` | u16 | BGM ID written by field-VM op 0x35 sub-1. |
| `0x8007BC64` | u16 | Global BGM pool base for IDs ≥ 2000. |
| `0x8007BD30` | 5008 bytes | Effect-runtime pool: 16-byte head + 128 child slots + 32 master slots. |
| `0x8007BD5C` | u32 | Effect 2-pack wrapper buffer pointer (post-init). |

## Runtime PROT TOC + asset chain

| Address | Purpose |
|---|---|
| `0x801C70F0` | In-RAM PROT TOC — populated at boot by `FUN_8003E4E8`. Different stride from on-disc. |
| `0x801C6EA4` | Current world / scene struct pointer. |
| `0x801C6460` | 64-entry × u16 scratchpad slot table. Written by op 0x4C nibble-C sub-A; adjusted by sub-B / sub-C. |
| `0x801C66A0` | 64-slot ramp scheduler pool (stride 0x20). |
| `0x8007C018` | TMD pointer table (`idx * 4` stride). Written by `FUN_80026B4C`. |
| `0x8007C348` | u32 | Free-list LIFO stack pointer for the actor allocator. |
| `0x8007C354` | Actor linked-list head. |
| `0x8007C364` | Player context pointer. |
| `0x8007C34C` | Linked-list head for `func_0x8003C83C`'s `0xFB` lookup. |
| `0x8007326C` | TMD per-mode descriptor table (8-byte stride × 6 entries). |
| `0x8007A940` | SsAPI per-note pitch / per-voice volume exponential lookup table (read by `FUN_80066E50` / `FUN_80067550`). |
| `0x801CD2B8` | SsAPI 16-bit slot-allocation bitmap. Bit `i` = sequencer slot `i` allocated. |
| `0x801CD2C0` | SsAPI 16-entry per-slot pointer table. Each entry → `0xB0`-byte sequence-state struct. |
| `0x801C4BEC` | libcd directory-entry cache (up to 128 entries, populated by `FUN_8005DEA0`). |
| `0x80074358` | Global 4×u32 ability bitmask. Written by `FUN_80042558` (OR-aggregate); read by `FUN_800431D0` (bit-test). |
| `0x80086D70` | 256-bit "fourth flag bank" bitfield. Wired to field-VM ops `0x50` / `0x60` / `0x70` via `FUN_8003CE08` / `_CE34` / `_CE64`. |

## Debug flags

| Address | Purpose |
|---|---|
| `0x8007B8C2` | Dev/retail loader-path flag. Read by 26 SCUS functions; no static writers. |
| `0x8007B98F` | In-game debug menu enable. Accessed as the high byte of the word at `0x8007B98C`. |
| `0x8007B7C0` | Debug-dispatch trigger. |
| `0x8007B450` | Debug-dispatch parameter slot. Also used by the field-VM `STATE_RESUME` opcode (`0x49`) as its tristate state register. |
| `0x8007B6F4` | "Small maps" debug mode flag. |
| `0x8007B850` | Per-frame button mask (built by `FUN_8001822C`). |
| `0x8007B7C0` | Previous-frame button mask. |
| `0x8007B874` | "Newly pressed this frame" (edge detection). |

JP retail uses build-shifted addresses (`0x07D51F` for the in-game debug menu enable; +0x1B90 from the NA address).

## PSX scratchpad (`0x1F800000-0x1F8003FF`)

The PSX has 1 KB of fast scratchpad RAM mapped here. Legaia uses the high end:

| Address | Type | Purpose |
|---|---|---|
| `0x1F800314` | i16[] | Inverted-Y mirror table (op 0x4C nibble-9 sub-E writes `-words[i]` here). |
| `0x1F800393` | u8 | Per-frame tick byte (read by op 0x4A `WAIT_FRAMES` and the 0xFFFF sentinel in op 0x4C nibble-C sub-B/C). |
| `0x1F800394` | u32 | **Global story-flag word.** Read by `GFLAG_TST` (0x30); written by `GFLAG_SET` / `GFLAG_CLR` (0x2E / 0x2F); also gates op 0x4C nibble-4 sub-9's tristate dispatch via bits `0x01000000` / `0x02000000`. Set by the dialog opener with bit 0x40 (`"dialog active"` lock). |
| `0x1F8003E8` | u32 | Render-config block (op 0x46). |
| `0x1F8003EC` | u8[] | Tile-flag bitmap base used by op 0x4C nibble-7 (rectangle SET/CLEAR over `+0x4000` offset). |
| `0x1F8003F8` / `0x1F8003FA` | i16 | Camera-scroll values used by op 0x23 player path. |

## Overlay window (`0x801C0000+`)

The 256 KB overlay window is shared between several runtime overlays — only one is loaded at any time. See [`tooling/overlay-capture.md`](../tooling/overlay-capture.md) for the per-overlay capture protocol and [`subsystems/boot.md`](../subsystems/boot.md) for which overlay loads when.

| RAM range | Overlay | Subsystems |
|---|---|---|
| `0x801C0000+` | Title screen | Actor / sprite VM (`FUN_801D6628`) |
| `0x801CE818+` | Town / field / dialog (loaded from PROT entry `0897_xxx_dat`) | Field VM (`FUN_801DE840`), MES renderer, inventory hub, MAIN INIT |
| `0x801CE818+` | Battle (loaded from PROT entry `0898_xxx_dat`) | Per-actor state machine, battle main dispatcher, effect VM cluster |
| `0x801C5818+` | Options / config menu (PROT 0896 = 0897 + 36 KB prefix) | In-game options UI |
| `0x801F0000+` | Battle effect helpers extend into here | `0x801F5D90`, `0x801F5CF8` (effect_id specials), `0x801F8004 / 88FC / 8D4C / 8E6C / 8F28` (particle / emitter cluster) |

## Mini-game state regions

Each mini-game gets its own ~64 KB slab of upper RAM, loaded fresh when entered. See [`reference/builds.md`](builds.md) for the per-mini-game RAM addresses.
