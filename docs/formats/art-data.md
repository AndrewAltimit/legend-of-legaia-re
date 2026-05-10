# Art Data - Tactical Arts records

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

Confidence: **Inferred** - citation chain comes from external RE work cross-referenced with Meth962's earlier observations; a watchpoint trace pinning the runtime read sites is still pending.

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
| `0x1B–0x32` | Per-character arts (the *constant* is shared across characters but resolves to a different art per character - see [`crates/art/src/tables.rs`](../../crates/art/src/tables.rs)) |

The first 4 directional bytes of a Miracle Art's replacement string are stored on disc with the high nibble's MSB set (`0x8C` / `0x8D` / `0x8E` / `0x8F`); the runtime ANDs with `0x7F` when copying into the queue. `legaia_art::miracle::unmask_replacement_byte` matches that.

## Art record layout

The layout is **schema-then-walk**: each record begins with a fixed prefix (commands, action constant, anim index), and the remainder is a sequence of variable-width fields whose presence depends on the art. The researcher captured field positions but did not pin every byte - this page documents the schema; [`crates/art`](../../crates/art) ships a strict parser for the prefix and surfaces the unparsed tail for downstream tooling.

### Fixed prefix

```
+0x00  u8[]   command sequence - values 1=L, 2=R, 3=D, 4=U; terminated by 0x00
       u8     action constant (0x1B..=0x32)
       u8     anim_index (primary)
       u8[5]  anim_extra (reserved; usually 0; some Hyper Arts chain multiple records)
```

### Variable fields (positions documented, exact byte offsets per art-specific)

| Field | Encoding |
|---|---|
| Art Name | UTF-like string. Populated for Super Arts, Miracle Art finishers, and some Hyper Arts; absent for regular arts (the runtime falls back to a per-character name table). |
| Art Power (×4) | Each byte is a damage multiplier - see [Power encoding](#power-encoding) below. |
| Damage Timing (×4) | One byte per Art Power byte; the animation frame at which that hit fires. |
| Special Effect Cues (×2) | Each cue occupies 2 words: half-word effect_id, then 3 half-words XYZ. Active iff any field is non-zero. |
| Hit Effect Cues (×4) | Each cue is a 32-bit word: high half = timing in frames, low half = constant (`0x1A` = sound effect, `0x4C` = hit effect, …). |
| Identifier | Byte. Some values trigger special animations (`0x67` in Heaven's Drop = Thunderbolt). |
| Anim Speed | Byte. Lower = slower playback, higher = faster. |
| Effect on Enemy | Byte. `1` = Burned, `2` = Shocked, others reserved. |
| Repeat Frames | 3 bytes: count, start_frame, end_frame. Replays a frame range; for some arts also repeats the damage from power bytes that fall in the range (Super Tempest's 4 power bytes → 8 actual hits). |
| Background | Byte. `0` = regular, `2` = black (Super Arts and Tornado Flame Hyper Art). |
| Runtime Address | Word. Written by the runtime after the art is used once in battle - always `None` in static data. |

### Power encoding

| Byte range | Defense target | Multiplier sequence | Notes |
|---|---|---|---|
| `0x16–0x1A` | UDF (Upper Defense Factor) | `12, 18, 20, 22, 28` | Standard UDF range |
| `0x1B–0x1F` | LDF (Lower Defense Factor) | `12, 18, 20, 22, 28` | Standard LDF range |
| `0x0C–0x10` | UDF (alt range) | `12, 18, 20, 22, 28` | UDF-target hits miss **short** enemies |
| `0x11–0x15` | LDF (alt range) | `12, 18, 20, 22, 28` | LDF-target hits miss **floating** enemies |
| any other | - | - | No damage |

Concretely: `0x1D` = LDF × 20, `0x19` = UDF × 22, `0x1F` = LDF × 28, `0x1A` = UDF × 28.

## Learned Art Constant

A separate per-character byte (`0x8008488D` Vahn, `0x80084CA1` Noa, `0x8008506C` Gala) tracks the *highest learned art slot*. Slot indices `0..=0x10` resolve to action constants through a per-character indirection table - and that table has **holes**: Noa skips slots `0x02` and `0x03` because her Hurricane Kick covers all three on-disc levels through a single learned slot.

Crate API: [`legaia_art::learned_art_action(character, slot)`](../../crates/art/src/tables.rs) returns the action constant for a slot, or `None` for holes / out-of-range.

| Slot | Vahn | Noa | Gala |
|---|---|---|---|
| `0x00` | Vahn's Craze (`0x1B`) | Noa's Ark (`0x1B`) | Biron Rage (`0x1B`) |
| `0x01` | Burning Flare (`0x1C`) | Hurricane Kick (`0x1C`) | Explosive Fist (`0x1C`) |
| `0x02` | Fire Blow (`0x1D`) | - | Lightning Storm (`0x1D`) |
| `0x03` | Tornado Flame (`0x1E`) | - | Thunder Punch (`0x1E`) |
| `0x04` | Cyclone (`0x1F`) | Vulture Blade (`0x1F`) | Bull Horns (`0x1F`) |
| `0x05` | Hurricane (`0x20`) | Frost Breath (`0x20`) | Electro Thrash (`0x20`) |
| `0x06` | PK Combo (`0x21`) | Tempest Break (`0x21`) | Neo Raising (`0x21`) |
| `0x07` | Spin Combo (`0x22`) | Rushing Gale (`0x22`) | Black Rain (`0x22`) |
| `0x08` | Pyro Pummel (`0x23`) | Tough Love (`0x23`) | Side Kick (`0x23`) |
| `0x09` | Cross Kick (`0x24`) | Swan Driver (`0x24`) | Head-Splitter (`0x24`) |
| `0x0A` | Power Punch (`0x25`) | Bird Step (`0x25`) | Guillotine (`0x25`) |
| `0x0B` | Slash Kick (`0x26`) | Dolphin Attack (`0x26`) | Back Punch (`0x26`) |
| `0x0C` | Somersault (`0x27`) | Mirage Lancer (`0x27`) | Ironhead (`0x27`) |
| `0x0D` | Charging Scorch (`0x28`) | Blizzard Bash (`0x28`) | Battering Ram (`0x28`) |
| `0x0E` | Hyper Elbow (`0x29`) | Sonic Javelin (`0x29`) | Flying Knee Attack (`0x29`) |
| `0x0F` | - | Acrobatic Blitz (`0x2A`) | - |
| `0x10` | - | Lizard Tail (`0x2B`) | - |

Vahn and Gala stop at `0x0E` (15 learned arts each). Noa extends to `0x10` (15 learned arts but spread across 17 slot positions because of the two Hurricane Kick holes).

## Art Anim Data

Selects the animation record played when the art fires. The Art Record's `anim_index` byte at offset +16 indexes a per-character animation table. Slot `0` is always Spirit; slot `3` is always Art Starter. Some slots are holes (e.g. Vahn has no slot `0x0C`, Gala has no slot `0x09`).

Crate API: [`legaia_art::art_anim_name(character, anim_index)`](../../crates/art/src/tables.rs).

| Anim | Vahn | Noa | Gala |
|---|---|---|---|
| `0x00` | Spirit | Spirit | Spirit |
| `0x01` | Power Punch | Tempest Break | Bull Horns |
| `0x02` | Slash Kick | Tough Love | Head-Splitter |
| `0x03` | Art Starter | Art Starter | Art Starter |
| `0x04` | Tornado Flame | Hurricane Kick 1 | Lightning Storm |
| `0x05` | Hurricane | Hurricane Kick 2 | Back Punch |
| `0x06` | Charging Scorch | Rushing Gale | Ironhead |
| `0x07` | PK Combo | Swan Driver | Battering Ram |
| `0x08` | Fire Blow | Frost Breath | Flying Knee Attack |
| `0x09` | Somersault | Lizard Tail | - |
| `0x0A` | Cyclone | Jurassic Blow 2 | Thunder Punch |
| `0x0B` | Hyper Elbow | Bird Step | Guillotine |
| `0x0C` | - | Dolphin Attack | Explosive Fist |
| `0x0D` | Burning Flare | Vulture Blade | Black Rain |
| `0x0E` | Spin Combo | Mirage Lancer | - |
| `0x0F` | Pyro Pummel | Blizzard Bash | - |
| `0x10` | Cross Kick | Sonic Javelin | Side Kick |
| `0x11` | Acrobatic Blitz | Electro Thrash | - |
| `0x12` | - | - | Neo Raising |

Most art records reference exactly one anim slot; a handful (e.g. Hurricane Kick on Noa) use the `anim_extra` reserved bytes at +17 to chain into a continuation slot for multi-stage animations.

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

Example - Vahn's Tri-Somersault (`0x2B`):

```
Find:    19 27 0F 19 1F 0E 19 27
         (Starter Somersault Up Starter Cyclone Down Starter Somersault)
Replace: 19 27 0F 19 1F 0E 1A 2B 2B 2B
         (… SpecialStarter, Tri-Somersault × 3 hits)
```

Triggers:
1. The last art of the Find string must be the last action in the queue.
2. All arts in the Find string must be non-NEW (their AP cost is paid).
3. Super Arts themselves do not consume AP - the chain arts pay it.

Full per-character tables (5 entries each for Vahn / Noa / Gala = 15 total) are in [`crates/art/src/super_art.rs`](../../crates/art/src/super_art.rs).

## See also

- [`docs/subsystems/battle-action.md`](../subsystems/battle-action.md) - battle action state machine that consumes the queue and resolves damage.
- [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md) - damage / MP / accuracy / RNG arithmetic kernels that read the power bytes.
- [`docs/formats/mdt.md`](mdt.md) - the per-frame *animation* bytecode for the Tactical Arts move VM, distinct from this art-record layer.
