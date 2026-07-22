//! Monster stat archive parser (PROT entry `0867_battle_data`).
//!
//! This is the global monster table the battle loader (`FUN_800542C8`)
//! streams at battle init: one fixed-size `0x14000`-byte slot per monster
//! id (1-based), `slot = (id-1) * 0x14000`. Each slot is
//! `[u32 decompressed_size][Legaia LZS stream]`; the decoded block's head
//! is the stat record that `FUN_80054CB0` copies into the battle actor.
//!
//! Pinned by a runtime watchpoint during live battles (Rim Elm scripted
//! fights): the loader's `disc_read` CdlLOC + relative-seek `(id-1)*40`
//! sectors resolve to PROT.DAT offset `0x38AF000` = entry 867, and three
//! decoded records (Gimard id 10, Killer Bee id 62, Queen Bee id 63) match
//! the live actor HP/MP/stats byte-for-byte. The CDNAME label `monster_data`
//! (PROT 869) is a misleading stub; the real archive is the 15.9 MB
//! `battle_data` entry 867. See `docs/subsystems/battle.md`.
//!
//! ## Record layout (decoded block head)
//!
//! All multi-byte fields are little-endian. Offsets are into the LZS-decoded
//! block; `name_offset` and `effect_offset` are block-relative byte offsets
//! the loader fixes up to absolute pointers at load.
//!
//! ```text
//! +0x00  u32  name_offset   ; -> NUL-terminated name string in the block
//! +0x04  u32  effect_offset ; -> attack-effect / animation data (actor +0x230;
//!                           ;    walked as 0x1C-stride geometry records by
//!                           ;    FUN_80049858 / FUN_800495C8). NOT XP/drop.
//! +0x08  u32  ptr3          ; -> shared resource pointer (fixed up at load)
//! +0x0C  u16  hp            ; -> actor +0x14C/+0x14E/+0x172
//! +0x0E  u16  stat0=AGL     ; -> actor +0x154/+0x156  (agility / action gauge,
//!                           ;    cur+base; spent per action, reset each round)
//! +0x10  u16  mp            ; -> actor +0x150/+0x152/+0x174
//! +0x12  u16  stat1=ATK     ; -> actor +0x158/+0x15A  (attacker offense)
//! +0x14  u16  stat2=DEF_hi  ; -> actor +0x15C/+0x15E  (defender defense A)
//! +0x16  u16  stat3=DEF_lo  ; -> actor +0x160/+0x162  (defender defense B)
//! +0x18  u16  stat4=INT     ; -> actor +0x168/+0x16A  (accuracy/evasion seed)
//! +0x1A  u16  stat5=SPD     ; -> actor +0x164/+0x166  (turn-order speed)
//! +0x1D  u8   element       ; element id 0..7; read record-DIRECT by affinity scale
//!                           ; FUN_801dd864 (record-ptr table 0x801C9348, NOT copied to actor)
//! +0x1F  u8   size_class    ; body-size / bulk class; read record-DIRECT through the
//!                           ; same 0x801C9348 table by the battle-camera framing
//!                           ; FUN_801f0348 (`ctx+0x6D0 = size << 7`, clamped
//!                           ; 0x0C00..0x1400) and by the enemy stager FUN_800513f0
//!                           ; (`actor+0x58 = size << 5`). Not copied to the actor.
//! +0x44  u16  gold          ; base gold reward (victory spoils)
//! +0x46  u16  exp           ; base EXP reward (victory spoils)
//! +0x48  u8   drop_item     ; drop item id (0 = no drop)
//! +0x49  u8   drop_chance   ; drop chance, percent (rand()%100 < pct)
//! +0x4A  u8   magic_count   ; spell-entry count
//! +0x4C  u32[] spell_offsets ; magic_count block-relative offsets -> spell entries
//! ```
//!
//! ## Spell list (`+0x4C`)
//!
//! `+0x4A` (u8) is the spell count; `+0x4C` is an array of that many u32
//! **block-relative byte offsets**, each pointing at a spell entry inside the
//! same decoded block. The loader `FUN_800542C8` fixes every offset to an
//! absolute pointer at battle init (`record[+0x4C + i*4] += block_base`),
//! exactly like `name_offset`; this parser keeps them block-relative.
//!
//! Each spell entry's head:
//!
//! - `+0x00` (u8) - spell/action id. The id doubles as a category selector:
//!   ids `2,3,4,5,0x0B` mark the **hit-reaction animation family** (light
//!   flinch / heavy flinch / knockdown / get-up / block - `FUN_80054CB0`
//!   caches the matching entry index into actor `+0x1EF..+0x1F3`, the map
//!   the damage primitive `FUN_800402F4` and the anim commit `FUN_8004AD80`
//!   stage reactions from); ids in `0x0C..=0x1F` are **offensive castable
//!   spells** the battle AI may roll (`overlay_0898_801e9fd4`); `0x20`/`0x21`/
//!   `0x22` are the attack-approach family the action SM resolves by
//!   first-byte search (`FUN_80050E2C`: pre-approach / close-in / victory);
//!   `0x23` (`'#'`) is a special category.
//! - `+0x74` (u8) - **AGL (action) cost**. Every battle action draws down the
//!   actor's AGL gauge; the enemy-AI spell picker only considers a spell when
//!   `cost != 0xFF` and the actor's current AGL (`+0x154`) is `>= cost`, then
//!   subtracts it on cast. `0xFF` = unavailable.
//! - `+0x04` / `+0x08` (u32) - on disc these are **1-based indices** (`0` =
//!   none), not pointers. Each indexes the per-block **effect-offset table**
//!   that immediately follows the `+0x4C` spell-offset array (table word base
//!   `magic_count + 0x13`). The battle loader `FUN_800542C8` resolves them with
//!   `entry[+0x04] = block[(index + magic_count + 0x12)*4] + block_base`, and
//!   initialises `+0x88` to `entry+0x8C` (a self-pointer; `0` on disc).
//!   [`MonsterSpell::effect_offset`] / [`MonsterSpell::aux_offset`] expose the
//!   resolved block-relative offsets, which land on a short fixed effect /
//!   animation descriptor (a small record, not a TMD); its interior field
//!   semantics are not yet pinned (the runtime consumer is the cast/effect path,
//!   not the AI picker). Earlier notes calling `+0x04`/`+0x08` direct
//!   block-relative sub-pointers "with the same geometry as the monster's own
//!   `+0x04`" were wrong twice over: they are indices (max observed `0x0A`), and
//!   the target is a small descriptor, not TMD geometry.
//!
//! ## Stat-name mapping
//!
//! The stat names match the game's own labels and the fan bestiaries (AGL /
//! INT / SPD), cross-checked against the runtime consumer each value feeds.
//! `FUN_80054CB0` copies each record halfword into a **pair** of adjacent
//! actor halfwords (a working value at the lower offset + a base at `+2`):
//!
//! - `stat1` (`+0x12`) is the **attacker's offensive value** in the
//!   physical-damage routine (`overlay_battle_action_801ec3e4`, actor `+0x158`)
//!   -> **ATK**.
//! - `stat2` / `stat3` (`+0x14` / `+0x16`) are the **defender's defense**;
//!   the routine picks one or the other by the attack's move index (`+0x15C`
//!   vs `+0x160`), and the "Defense Up" buff raises both together -> the
//!   two-facet **defense pair** (`MonsterDef::udf` / `ldf`).
//! - `stat0` (`+0x0E`) is the **AGL / agility (action) gauge** (actor `+0x154`
//!   current, `+0x156` base). Confirmed in-game: the "Power Up" buff prints
//!   *"<enemy>'s agility increased!"* and raises this cur/base pair
//!   (live-RAM-verified by Zetopheonix). Every action draws it down; the enemy
//!   AI only queues an action whose `+0x74` cost it can still afford
//!   (`overlay_0898_801e9fd4`), and it resets to base each round
//!   (`overlay_battle_action_801d88cc`; the "Spirit"/charge state raises the
//!   reset value via the `(base*7/5)+8` cap-288 shape). This is the gauge fan
//!   bestiaries call **AGL** (30 AGL ~= one extra command slot); earlier notes
//!   mislabeled it "SP / spirit".
//! - `stat4` (`+0x18`) is the **INT** stat (the curated `enemies.toml` `int`
//!   column byte-matches it; see `gamedata/tests/enemy_stats_vs_disc`).
//!   Meth962's walkthrough: INT governs **magical damage and defense against
//!   magic** - and the summon/arts damage kernel (`FUN_801dd0ac`) confirms it,
//!   reading the attacker's INT as a damage term and the defender's INT as a
//!   mitigation term; it also seeds the **accuracy/evasion** roll (`FUN_800402F4`
//!   selector 9, actor `+0x168`). Earlier notes mislabeled it "AGL".
//! - `stat5` (`+0x1A`) seeds the per-turn **initiative roll**
//!   (`+0x16C = stat5 + rand(0..stat5/2) + 1` in `overlay_0897_801e23ec`),
//!   has a dedicated "Speed Up" buff, and resets to base each round -> **SPD**
//!   (turn-order speed).
//!
//! Use the named accessors ([`attack`](MonsterRecord::attack) /
//! [`defense_high`](MonsterRecord::defense_high) /
//! [`defense_low`](MonsterRecord::defense_low) /
//! [`agility`](MonsterRecord::agility) / [`speed`](MonsterRecord::speed) /
//! [`intelligence`](MonsterRecord::intelligence)); [`stats`](MonsterRecord::stats)
//! keeps all six in raw record order.
//!
//! ## Battle-load stat boost
//!
//! The values in the record are **not** the values the player fights. When
//! `FUN_80054cb0` copies a record into the live battle actor it then *boosts*
//! four of the six combat stats - so the raw record systematically understates
//! the enemy. The function carries two scaling profiles, selected by the
//! battle-context flag at `_DAT_8007bd24 + 0x287` (itself
//! `(*(u8*)0x8007BD60 >> 5) & 4` = bit 7 of a per-battle flags byte, set by the
//! actor-registration routine `FUN_800513f0`):
//!
//! | stat | gate-set profile (B) | gate-clear profile (A) |
//! |---|---|---|
//! | `attack` (ATK)          | `+= atk >> 2` (`×5/4`) | unchanged |
//! | `defense_high` (UDF)    | `× 2`                  | `+= (udf>>1)+(udf>>2)` (`×7/4`) |
//! | `defense_low` (LDF)     | `× 2`                  | `+= (ldf>>1)+(ldf>>2)` (`×7/4`) |
//! | `intelligence` (INT)    | `+= int >> 3` (`×9/8`) | `+= int >> 2` (`×5/4`) |
//! | HP / MP / AGL / SPD     | unchanged              | unchanged |
//!
//! Both profiles boost; the flag only picks which. The **gate-set profile (B)**
//! is the one a live international-retail battle capture (Gaza Sim-Seru, id 166)
//! reproduces byte-for-byte, and the one the curated `enemies.toml` bestiary
//! matches - so [`MonsterRecord::battle_stats`] returns it as *the* in-battle
//! stat block. The cross-region origin of this boost (the international US/PAL
//! build hitting harder than the raw record / the Japanese release) was first
//! surfaced by **Zetopheonix**; this module pins the mechanism and exact
//! factors. See `docs/subsystems/battle.md` ("Monster-record source layout" -
//! the *Battle-load stat boost* note) and `docs/subsystems/battle-formulas.md`.
//!
//! ## Rewards (EXP / gold / drop)
//!
//! These are inline in the record head at `+0x44..+0x49` (**not** at `+0x04`,
//! which is effect/animation data). The victory-spoils function `FUN_8004E568`
//! reads them from the per-enemy record-pointer table at `0x801C9348`:
//!
//! - `+0x44` (u16) - base gold. Summed `>> 1` across dead enemies, optionally
//!   `* 1.25` (if a living party member has ability bit `0x10000`), then the
//!   total is halved: a lone enemy yields `floor((gold >> 1) / 2)` gold
//!   (Gimard `60` -> `15`, runtime-confirmed).
//! - `+0x46` (u16) - base EXP. Summed `* 3/4` across dead enemies, then split
//!   evenly among living party members.
//! - `+0x48` (u8) - drop item id (`0` = no drop).
//! - `+0x49` (u8) - drop chance in percent (`rand() % 100 < chance`).
//!
//! See [`MonsterRecord::gold`] / [`exp`](MonsterRecord::exp) /
//! [`drop_item`](MonsterRecord::drop_item) /
//! [`drop_chance_pct`](MonsterRecord::drop_chance_pct). Drop *item names* are
//! cross-checked against `legaia-gamedata` (`enemies.toml`).

use anyhow::{Result, bail};

mod animation;
mod mesh;
mod record;

pub use animation::*;
pub use mesh::*;
pub use record::*;

pub(crate) use animation::{ANIM_RATE_OFFSET, parse_animation_stream};

/// Fixed per-monster slot stride inside the archive (`0x14000` bytes = 40
/// sectors). Confirmed by the loader's relative-seek `(id-1)*40` sectors.
pub const SLOT_STRIDE: usize = 0x14000;

/// Minimum decoded-block size that can hold the stat record head.
const MIN_RECORD_BYTES: usize = 0x4C;

/// LZS-decode monster id `id`'s archive slot into its raw block bytes.
///
/// Returns `Ok(None)` for an out-of-range id or an empty / filler slot (one
/// whose `dec_size` header fails the plausibility bounds). Returns `Err` only
/// when the slot claims a valid `dec_size` but the LZS stream fails to decode
/// to that length. Shared by [`record`] and [`mesh`]; the raw block is also
/// the modder-facing edit surface (see [`encode_slot`] for the way back).
pub fn decode_block(entry: &[u8], id: u16) -> Result<Option<Vec<u8>>> {
    if id == 0 {
        return Ok(None);
    }
    let slot = (id as usize - 1) * SLOT_STRIDE;
    let Some(dec_size) = legaia_bytes::u32_le(entry, slot) else {
        return Ok(None);
    };
    let dec_size = dec_size as usize;
    // Filler / empty slots carry a tiny or absurd dec_size. Bound it to a
    // plausible monster-block size before trusting the LZS decode.
    if !(MIN_RECORD_BYTES..=SLOT_STRIDE * 8).contains(&dec_size) {
        return Ok(None);
    }
    // Hand the decoder a generous source slice (the LZS stream can spill past
    // its own slot, like every other Legaia LZS container).
    let src = &entry[slot + 4..];
    let block = legaia_lzs::decompress(src, dec_size)?;
    if block.len() != dec_size {
        bail!(
            "monster id {id}: LZS decoded {} bytes, expected {dec_size}",
            block.len()
        );
    }
    Ok(Some(block))
}

/// Re-pack a decoded monster block into a full [`SLOT_STRIDE`]-byte archive
/// slot: `[u32 block_len][LZS stream]`, zero-padded. The encoder is not
/// byte-identical to Sony's, but the retail decoder accepts any valid stream;
/// errors when the re-packed stream would overflow the fixed slot.
pub fn encode_slot(block: &[u8]) -> Result<Vec<u8>> {
    let stream = legaia_lzs::compress(block);
    if 4 + stream.len() > SLOT_STRIDE {
        bail!(
            "re-packed stream does not fit a monster slot: 4 + {} > {SLOT_STRIDE}",
            stream.len()
        );
    }
    let mut out = Vec::with_capacity(SLOT_STRIDE);
    out.extend_from_slice(&(block.len() as u32).to_le_bytes());
    out.extend_from_slice(&stream);
    out.resize(SLOT_STRIDE, 0);
    Ok(out)
}

#[cfg(test)]
mod slot_tests {
    #[test]
    fn encode_slot_roundtrips_through_the_retail_decoder_shape() {
        let block: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
        let slot = super::encode_slot(&block).unwrap();
        assert_eq!(slot.len(), super::SLOT_STRIDE);
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        assert_eq!(declared, block.len());
        let decoded = legaia_lzs::decompress(&slot[4..], declared).unwrap();
        assert_eq!(decoded, block);
    }
}
