# Art Data — Tactical Arts records

Each playable character (Vahn, Noa, Gala) has a per-character table of *art records* describing damage, animation, hit timing, status effects, and Super/Miracle Art trigger metadata. These records drive the battle action system.

Implementation: [`crates/art`](../../crates/art/README.md).

## Where the data lives

| Source | Location |
|---|---|
| Vahn art records (RAM) | `0x80160EFC` (first record) onwards |
| Noa art records (RAM)  | `0x80176998` (first record) onwards |
| Gala art records (RAM) | `0x8018BA54` (first record) onwards |
| Learned Art Constants (RAM) | Vahn `0x8008488D`, Noa `0x80084CA1`, Gala `0x8008506C` |
| On-disc source | PROT entry `0x05C4` |
| Miracle Art trigger entries (RAM `801F` segment) | Vahn's Craze `0x64F4`, Noa's Ark `0x6504`, Biron Rage `0x6514` |
| Miracle Art trigger entries (PROT `0x05C4` area) | Vahn's Craze `0x0CDC`, Noa's Ark `0x0CEC`, Biron Rage `0x0CFC` |

Confidence: **Inferred** — citation chain comes from external RE work cross-referenced with Meth962's earlier observations; a watchpoint trace pinning the runtime read sites is still pending.

## Action Constants

Every entry in the battle action queue is one of these `0x00–0x32` values:

| Byte | Meaning |
|---|---|
| `0x00` | Nothing |
| `0x01` | Item |
| `0x02` | Magic |
| `0x03` | Attack |
| `0x04` | Spirit |
| `0x05` | Escape |
| `0x06` | unidentified |
| `0x07` | Faint Animation 1 |
| `0x08` | Faint Animation 2 |
| `0x09` | unidentified |
| `0x0A` | Item / Magic Animation |
| `0x0B` | Block Animation |
| `0x0C` | Left |
| `0x0D` | Right |
| `0x0E` | Down |
| `0x0F` | Up |
| `0x10` | Spirit Animation |
| `0x11–0x18` | Empty Slots 1–8 (placeholder, never appears in static data) |
| `0x19` | Regular Art Starter |
| `0x1A` | Special Art Starter |
| `0x1B–0x32` | Per-character arts (the *constant* is shared across characters but resolves to a different art per character — see [`crates/art/src/tables.rs`](../../crates/art/src/tables.rs)) |

The first 4 directional bytes of a Miracle Art's replacement string are stored on disc with the high nibble's MSB set (`0x8C` / `0x8D` / `0x8E` / `0x8F`); the runtime ANDs with `0x7F` when copying into the queue. `legaia_art::miracle::unmask_replacement_byte` matches that.

## Art record layout

The layout is **schema-then-walk**: each record begins with a fixed prefix (commands, action constant, anim index), and the remainder is a sequence of variable-width fields whose presence depends on the art. The researcher captured field positions but did not pin every byte — this page documents the schema; [`crates/art`](../../crates/art) ships a strict parser for the prefix and surfaces the unparsed tail for downstream tooling.

### Fixed prefix

```
+0x00  u8[]   command sequence — values 1=L, 2=R, 3=D, 4=U; terminated by 0x00
       u8     action constant (0x1B..=0x32)
       u8     anim_index (primary)
       u8[5]  anim_extra (reserved; usually 0; some Hyper Arts chain multiple records)
```

### Variable fields (positions documented, exact byte offsets per art-specific)

| Field | Encoding |
|---|---|
| Art Name | UTF-like string. Populated for Super Arts, Miracle Art finishers, and some Hyper Arts; absent for regular arts (the runtime falls back to a per-character name table). |
| Art Power (×4) | Each byte is a damage multiplier — see [Power encoding](#power-encoding) below. |
| Damage Timing (×4) | One byte per Art Power byte; the animation frame at which that hit fires. |
| Special Effect Cues (×2) | Each cue occupies 2 words: half-word effect_id, then 3 half-words XYZ. Active iff any field is non-zero. |
| Hit Effect Cues (×4) | Each cue is a 32-bit word: high half = timing in frames, low half = constant (`0x1A` = sound effect, `0x4C` = hit effect, …). |
| Identifier | Byte. Some values trigger special animations (`0x67` in Heaven's Drop = Thunderbolt). |
| Anim Speed | Byte. Lower = slower playback, higher = faster. |
| Effect on Enemy | Byte. `1` = Burned, `2` = Shocked, others reserved. |
| Repeat Frames | 3 bytes: count, start_frame, end_frame. Replays a frame range; for some arts also repeats the damage from power bytes that fall in the range (Super Tempest's 4 power bytes → 8 actual hits). |
| Background | Byte. `0` = regular, `2` = black (Super Arts and Tornado Flame Hyper Art). |
| Runtime Address | Word. Written by the runtime after the art is used once in battle — always `None` in static data. |

### Power encoding

| Byte range | Defense target | Multiplier sequence | Notes |
|---|---|---|---|
| `0x16–0x1A` | UDF (Upper Defense Factor) | `12, 18, 20, 22, 28` | Standard UDF range |
| `0x1B–0x1F` | LDF (Lower Defense Factor) | `12, 18, 20, 22, 28` | Standard LDF range |
| `0x0C–0x10` | UDF (alt range) | `12, 18, 20, 22, 28` | UDF-target hits miss **short** enemies |
| `0x11–0x15` | LDF (alt range) | `12, 18, 20, 22, 28` | LDF-target hits miss **floating** enemies |
| any other | — | — | No damage |

Concretely: `0x1D` = LDF × 20, `0x19` = UDF × 22, `0x1F` = LDF × 28, `0x1A` = UDF × 28.

## Miracle Arts

Each character has one Miracle Art. When the player enters the *exact* command sequence for that art, the runtime **clears the entire action queue** and writes the art's replacement string instead.

| Character | Art | RAM | PROT | Command sequence |
|---|---|---|---|---|
| Vahn | Vahn's Craze | `0x64F4` | `0x0CDC` | R D L U L U R D L |
| Noa | Noa's Ark | `0x6504` | `0x0CEC` | L U R D U L U D R |
| Gala | Biron Rage | `0x6514` | `0x0CFC` | R R D U D U D L L |

Each replacement string follows the shape `[L, R, D, U, SpecialStarter, art1, art2, ...]` where the four leading directionals are the on-disc MSB-set bytes (`0x8C`/`0x8D`/`0x8E`/`0x8F`), masked to `0x0C`/`0x0D`/`0x0E`/`0x0F` at copy time. The full table is in [`crates/art/src/miracle.rs`](../../crates/art/src/miracle.rs).

## Super Arts

Super Arts are not invoked by direct command-string match. Instead, after each art finishes, the runtime walks the full action queue and looks for a registered *Find* pattern. If a Find pattern matches the **tail** of the queue and all participating arts paid AP, the matched bytes are replaced by a *Replace* tail that ends with the Super Art's finisher action constant.

Example — Vahn's Tri-Somersault (`0x2B`):

```
Find:    19 27 0F 19 1F 0E 19 27
         (Starter Somersault Up Starter Cyclone Down Starter Somersault)
Replace: 19 27 0F 19 1F 0E 1A 2B 2B 2B
         (… SpecialStarter, Tri-Somersault × 3 hits)
```

Triggers:
1. The last art of the Find string must be the last action in the queue.
2. All arts in the Find string must be non-NEW (their AP cost is paid).
3. Super Arts themselves do not consume AP — the chain arts pay it.

Full per-character tables (5 entries each for Vahn / Noa / Gala = 15 total) are in [`crates/art/src/super_art.rs`](../../crates/art/src/super_art.rs).

## See also

- [`docs/subsystems/battle-action.md`](../subsystems/battle-action.md) — battle action state machine that consumes the queue and resolves damage.
- [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md) — damage / MP / accuracy / RNG arithmetic kernels that read the power bytes.
- [`docs/formats/mdt.md`](mdt.md) — the per-frame *animation* bytecode for the Tactical Arts move VM, distinct from this art-record layer.
