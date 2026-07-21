# Art Data - Tactical Arts records

Each playable character (Vahn, Noa, Gala) has a per-character table of *art records* describing damage, animation, hit timing, status effects, and Super/Miracle Art trigger metadata. These records drive the battle action system.

Implementation: [`crates/art`](../../crates/art/README.md).

## Contents

- [Where the data lives](#where-the-data-lives)
- [Action Constants](#action-constants)
- [Art record layout](#art-record-layout)
  - [Fixed prefix](#fixed-prefix)
  - [Variable fields](#variable-fields-positions-documented-exact-byte-offsets-per-art-specific)
  - [Power encoding](#power-encoding)
- [Learned Art Constant](#learned-art-constant)
- [Art Anim Data](#art-anim-data)
- [Miracle Arts](#miracle-arts)
- [Super Arts](#super-arts)
- [Arts-name table (`DAT_80075EC4`)](#arts-name-table-dat_80075ec4)
  - [Command-glyph string (`+8`)](#command-glyph-string-8)
  - [Validation oracle](#validation-oracle)
- [See also](#see-also)

## Where the data lives

| Source | Location |
|---|---|
| Vahn art records (RAM) | `0x80160EFC` (first record) onwards |
| Noa art records (RAM)  | `0x80176998` (first record) onwards |
| Gala art records (RAM) | `0x8018BA54` (first record) onwards |
| Learned Art Constants (RAM) | Vahn `0x8008488D`, Noa `0x80084CA1`, Gala `0x8008506C` |
| On-disc source | PROT entry `0x05C4` (Inferred, see warning) |
| Miracle Art trigger entries (RAM `801F` segment) | Vahn's Craze `0x64F4`, Noa's Ark `0x6504`, Biron Rage `0x6514` |
| Miracle Art trigger entries (PROT `0x05C4` area) | Vahn's Craze `0x0CDC`, Noa's Ark `0x0CEC`, Biron Rage `0x0CFC` |

Confidence: **Inferred** - the per-record damage / animation / effect schema below comes from external RE work cross-referenced with Meth962's earlier observations; a watchpoint trace pinning the runtime read sites is still pending.

> **Where the button combos live - there are TWO copies (savestate-proven).**
> The directional command of each art is stored in two different files, and they
> serve two different consumers:
>
> 1. **The matcher** (what actually fires the art) reads the per-character art
>    records at the RAM bases above (`0x80160EFC` Vahn / `0x80176998` Noa /
>    `0x8018BA54` Gala), where the combo is the `1=L,2=R,3=D,4=U` byte run
>    (0-terminated) at record `+0`, on a fixed `0xD0` stride - **exactly the
>    record layout the [Art record layout](#art-record-layout) section below
>    describes**. (So that schema was right about the format; only the "PROT
>    `0x05C4`" label was wrong - `0x05C4` = 1476 isn't a valid PROT index.) These
>    records are *not* resident until the Arts menu is opened; they load from each
>    character's player-data file `record0` (canonical extraction entries: Vahn
>    `0863` / Noa `0864` / Gala `0865` - the `edstati3`/PLAYERn files; the
>    historical "Vahn `0861`" attribution matched the same bytes through 0861's
>    extended over-read window), whose decoded `record0` byte-matches the live
>    RAM.
> 2. **The display** is the SCUS `DAT_80075EC4` arts-name table `+8` glyph string
>    (see [Arts-name table](#arts-name-table-dat_80075ec4)) - only the arrows
>    shown in the menu.
>
> Two emulator playtests proved the split: editing the SCUS glyph copy (whether by
> moving the `+8` pointer or overwriting the glyph bytes) changes the menu arrows
> but the art still triggers on the old combo, because the matcher reads the
> player-file record copy. So a faithful edit must change **both** copies. This is
> what the arts-combo randomizer does - see [`docs/tooling/randomizer.md`](../tooling/randomizer.md).

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

These constants double as the **battle anim-id space**: the action SM's
strike loop stages each queue byte verbatim into `actor[+0x1DA]`, and the
anim commit `FUN_8004AD80` resolves it - directions `0x0C..0x0F` index the
runtime action table directly (those four slots are swing records spliced
from the **equipped-item sections** at battle init), while ids `>= 0x10`
(starters, arts) materialize a record from the per-character `0xD0`-stride
**art-animation bank** (the record[0] `+0x58` pointer) into dynamic table
slot `0x10`/`0x11` - the on-disc "Empty Slots". Art ids `0x1B+` also drive
the HUD art-name display and `FUN_8004C650(char, id - 0x1B)`. The "Empty
Slots 1-8" ids `0x11..0x18` reappear at runtime in `actor[+0x1DB]` (last
staged id), where the battle camera driver `FUN_801D5854` dispatches
per-art camera variants on them. See
[battle-data-pack.md § Battle animations](battle-data-pack.md#battle-animations-record0).

## Art record layout

The layout is **schema-then-walk**: each record begins with a fixed prefix (commands, action constant, anim index), and the remainder is a sequence of variable-width fields whose presence depends on the art. The researcher captured field positions but did not pin every byte - this page documents the schema; [`crates/art`](../../crates/art) ships a strict parser for the prefix and surfaces the unparsed tail for downstream tooling.

> The `+0x00` command-sequence field below is the form the **runtime matcher**
> reads (the `1=L,2=R,3=D,4=U` run at record `+0`, 0-terminated; records are a
> fixed `0xD0` stride - see the warning at the top of this page). It lives in the
> per-character player-data `record0` (canonical extraction entries: Vahn
> `0863` / Noa `0864` / Gala `0865`; `0861` was the historical over-read
> window), not at PROT `0x05C4`. The SCUS [arts-name table](#arts-name-table-dat_80075ec4)
> `+8` glyph string is the *display* copy of the same combo.

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
| Effect on Enemy | Byte status ailment: `1` = Toxic, `2` = Numb, `3` = Venom, `4` = Sleep, `5` = Confuse, `6` = Curse, `7` = Stone, `8` = Faint (see `legaia_engine_vm::status_effects`). |
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

The interleaved connector direction after each art (the `0F` / `0E` above) is **combo-specific**, not derivable from each art's own command string:

- The same art appears with different connectors across Supers (Vahn's `0x27` is followed by `0F` in Tri-Somersault but `0E` in Power Slash).
- Those connectors are **resident table data**, not derived: the battle overlay keeps the full replace-string table at `0x801F65E8` (15 entries, 16-byte stride), read byte-exact for all 15 Supers from a battle-RAM capture, so `super_art.rs`'s `replace` strings are runtime-validated. (`ctx[+0x274]`, once suspected as the queue-builder, is the turn-order active-actor index; the live action queue is `actor[+0x1DF]`.)
- The live player-driven Arts submenu therefore matches a recognized art *ordering* against `SuperArt::art_sequence()` - the Find pattern projected to its art constants only (`[0x27, 0x1F, 0x27]` for Tri-Somersault) - via `legaia_art::recognize_art_sequence` + `SuperMatcher::trigger_by_art_sequence`, which is faithful to *which* combination triggers *which* Super without reproducing the byte-exact queue. See [`subsystems/battle-action.md`](../subsystems/battle-action.md#miracle--super-in-the-live-player-driven-arts-submenu).

## Arts-name table (`DAT_80075EC4`)

The display names + AP costs of every Tactical Art live in a static table in
`SCUS_942.54` at `DAT_80075EC4`. It's the table the MES interpreter's `0xC5`
substitution code reads (see [mes.md](mes.md#bytecode-encoding)) - the `0xC5`
operand `XX` keys it as `(character = XX>>6, art index = XX&0x3F)`.

The expander (`FUN_80036514`) scans 20-byte (`0x14`) records sorted by
character, matching `(record[+0], record[+1])` against the key, and returns the
`+0xC` name pointer (`(&PTR_DAT_80075ED0)[match_index * 5]`).

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | character: `0` Vahn, `1` Noa, `2` Gala |
| `+1` | u8 | art display index within the character |
| `+2` | u8 | **AP cost** |
| `+3` | u8 | padding |
| `+4` | u16 | round value (≈ power/score; exact meaning unconfirmed) |
| `+6` | u16 | zero |
| `+8` | u32 | pointer to the command-input display string (MES arrow-glyph sequence; its first byte is the input count) |
| `+0xC` | u32 | pointer to the name string |
| `+0x10` | u32 | aux pointer (second string) |

A `(99, 99)` record named `"End"` terminates the table. Each character's index
`0` entry is the **Miracle Art** (AP byte `99`; the name string opens with a
`0xCE 0x09` character-name-substitution control, e.g. *"&'s Ark"* / Gala's
*"Biron Rage"*).

The AP costs are byte-exact against the curated [`gamedata`](../reference/gamedata.md)
arts table (every matched art agrees), which makes this the on-disc provenance
for that table's `ap` column + the canonical art display order.

### Command-glyph string (`+8`)

The `+8` pointer is the **command-input string** - the arrow sequence shown in
the arts menu (the *display* copy of the combo; the matcher reads a separate
`1-4` copy in the player-file records, see the warning at the top of this page).
Encoding: `[count u8]` then `count`
two-byte glyph codes. A one-off `0xFF XX` marker separates the sequence
(`0xFF06` for regular arts, `0xFF09` for Miracle arts), is **not** a direction,
and its position within the string varies (it can sit mid-combo). The arrow
glyphs map to physical d-pad directions:

| Glyph | Direction | dir code |
|---|---|---|
| `0x81A9` | ← Left | 1 |
| `0x81A8` | → Right | 2 |
| `0x81AB` | ↓ Down | 3 |
| `0x81AA` | ↑ Up | 4 |

The string stores the **physical** direction; the logical action (Arms /
Ra-Seru) depends on the character's handedness (Noa's Arms / Ra-Seru are
swapped). The codes match the `Left=1 / Right=2 / Down=3 / Up=4` encoding the
PROT records use. Cross-checking against gamedata surfaces at least one
walkthrough error (Vahn's *Hyper Elbow* is `L R L` on disc, not `Arms / Ra-Seru
/ High`). Decoded by `legaia_art::arts_table::parse_from_scus`; dump it with
`art arts-table`.

### Validation oracle

Because the glyph string is byte-exact ground truth, it serves as the
validation oracle for the two derived command sources:

- **The best-effort PROT `0x05C4` parser** ([`legaia_art::parse_record`]).
  `legaia_art::ArtsOracle::by_command(character, &commands)` resolves a decoded
  command sequence back to a named art; the disc-gated contract test
  `crates/art/tests/arts_table_real.rs` runs every art's canonical record bytes
  through `parse_record` and asserts the decode round-trips through the oracle.
  This pins the parser's `1=L,2=R,3=D,4=U` command-byte decode against the
  executable without needing the (still-unpinned) full record stride.
- **The curated `legaia-gamedata` `arts.toml` `ap` + `directions` columns.**
  The disc-gated test `crates/gamedata/tests/arts_scus_oracle.rs` matches each
  curated art to its SCUS row by name and asserts AP + directions agree, with a
  small explicit allowlist for documented walkthrough errors (currently only
  *Hyper Elbow*). A new undocumented divergence fails the test.

## See also

- [`docs/subsystems/battle-action.md`](../subsystems/battle-action.md) - battle action state machine that consumes the queue and resolves damage.
- [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md) - damage / MP / accuracy / RNG arithmetic kernels that read the power bytes.
- [`docs/subsystems/arts-command-gauge.md`](../subsystems/arts-command-gauge.md) - the AP gauge the player spends inputting these arts, and the weapon-specialty arm-width penalty.
- [`docs/formats/mdt.md`](mdt.md) - the per-frame *animation* bytecode for the Tactical Arts move VM, distinct from this art-record layer.
