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
//! +0x0E  u16  stat0=SP      ; -> actor +0x154/+0x156  (spirit/action gauge)
//! +0x10  u16  mp            ; -> actor +0x150/+0x152/+0x174
//! +0x12  u16  stat1=ATK     ; -> actor +0x158/+0x15A  (attacker offense)
//! +0x14  u16  stat2=DEF_hi  ; -> actor +0x15C/+0x15E  (defender defense A)
//! +0x16  u16  stat3=DEF_lo  ; -> actor +0x160/+0x162  (defender defense B)
//! +0x18  u16  stat4=AGL     ; -> actor +0x168/+0x16A  (accuracy/evasion)
//! +0x1A  u16  stat5=SPD     ; -> actor +0x164/+0x166  (turn-order speed)
//! +0x1D  u8   element       ; -> actor +0x1d  (element id 0..7, affinity scale)
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
//! - `+0x00` (u8) — spell/action id. The id doubles as a category selector:
//!   ids `2,3,4,5,0x0B` mark an **elemental resist/affinity** (`FUN_80054CB0`
//!   stores the matching spell index into actor `+0x1EF..+0x1F3`); ids in
//!   `0x0C..=0x1F` are **offensive castable spells** the battle AI may roll
//!   (`overlay_0898_801e9fd4`); `0x23` (`'#'`) is a special category.
//! - `+0x74` (u8) — **SP (spirit) cost**. The enemy-AI spell picker only
//!   considers a spell when `cost != 0xFF` and the actor's current SP
//!   (`+0x154`) is `>= cost`, then subtracts it on cast. `0xFF` = unavailable.
//! - `+0x04` / `+0x08` (u32) — on disc these are **1-based indices** (`0` =
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
//! ## Stat-name mapping (traced from `FUN_80054CB0` + the formula consumers)
//!
//! `FUN_80054CB0` copies each record halfword into a **pair** of adjacent
//! actor halfwords (a working value at the lower offset + a base at `+2`).
//! Naming follows the consumers of those actor slots:
//!
//! - `stat1` (`+0x12`) is the **attacker's offensive value** in the
//!   physical-damage routine (`overlay_battle_action_801ec3e4`, actor `+0x158`)
//!   -> **ATK**.
//! - `stat2` / `stat3` (`+0x14` / `+0x16`) are the **defender's defense**;
//!   the routine picks one or the other by the attack's move index (`+0x15C`
//!   vs `+0x160`), and the "Defense Up" buff raises both together -> the
//!   two-facet **defense pair** (`MonsterDef::udf` / `ldf`).
//! - `stat4` (`+0x18`) seeds the **accuracy/evasion** roll (`FUN_800402F4`
//!   selector 9, actor `+0x168`) -> **AGL**.
//! - `stat5` (`+0x1A`) seeds the per-turn **initiative roll**
//!   (`+0x16C = stat5 + rand(0..stat5/2) + 1` in `overlay_0897_801e23ec`),
//!   has a dedicated "Speed Up" buff, and resets to base each round -> **SPD**
//!   (turn-order speed).
//! - `stat0` (`+0x0E`) is the actor's **SP / spirit-action gauge** (actor
//!   `+0x154` current, `+0x156` base): the AI spends it picking spells
//!   (`overlay_0898_801e9fd4` deducts each spell's `+0x74` cost), it
//!   regenerates to base each round, and the spirit-charge value derives from
//!   it via the `(base*7/5)+8` cap-288 shape (`overlay_battle_action_801d88cc`).
//!   Corroborates the HP/MP/SP-triplet reading in `docs/subsystems/battle.md`.
//!
//! Use the named accessors ([`attack`](MonsterRecord::attack) /
//! [`defense_high`](MonsterRecord::defense_high) /
//! [`defense_low`](MonsterRecord::defense_low) /
//! [`agility`](MonsterRecord::agility) / [`speed`](MonsterRecord::speed) /
//! [`spirit`](MonsterRecord::spirit)); [`stats`](MonsterRecord::stats) keeps
//! all six in raw record order.
//!
//! ## Rewards (EXP / gold / drop)
//!
//! These are inline in the record head at `+0x44..+0x49` (**not** at `+0x04`,
//! which is effect/animation data). The victory-spoils function `FUN_8004E568`
//! reads them from the per-enemy record-pointer table at `0x801C9348`:
//!
//! - `+0x44` (u16) — base gold. Summed `>> 1` across dead enemies, optionally
//!   `* 1.25` (if a living party member has ability bit `0x10000`), then the
//!   total is halved: a lone enemy yields `floor((gold >> 1) / 2)` gold
//!   (Gimard `60` -> `15`, runtime-confirmed).
//! - `+0x46` (u16) — base EXP. Summed `* 3/4` across dead enemies, then split
//!   evenly among living party members.
//! - `+0x48` (u8) — drop item id (`0` = no drop).
//! - `+0x49` (u8) — drop chance in percent (`rand() % 100 < chance`).
//!
//! See [`MonsterRecord::gold`] / [`exp`](MonsterRecord::exp) /
//! [`drop_item`](MonsterRecord::drop_item) /
//! [`drop_chance_pct`](MonsterRecord::drop_chance_pct). Drop *item names* are
//! cross-checked against `legaia-gamedata` (`enemies.toml`).

use anyhow::{Result, bail};

/// Fixed per-monster slot stride inside the archive (`0x14000` bytes = 40
/// sectors). Confirmed by the loader's relative-seek `(id-1)*40` sectors.
pub const SLOT_STRIDE: usize = 0x14000;

/// Minimum decoded-block size that can hold the stat record head.
const MIN_RECORD_BYTES: usize = 0x4C;

/// One spell entry referenced by a monster record's `+0x4C` offset array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterSpell {
    /// Spell/action id (entry `+0x00`). Ids `2,3,4,5,0x0B` mark an elemental
    /// resist/affinity, `0x0C..=0x1F` are offensive castable spells, `0x23`
    /// (`'#'`) is a special category.
    pub id: u8,
    /// SP (spirit) cost (entry `+0x74`). `0xFF` = unavailable (the AI never
    /// picks it; treated as a non-castable / passive slot).
    pub sp_cost: u8,
    /// Block-relative byte offset of this spell entry (the `+0x4C` array
    /// element, before the loader's add-block-base fixup).
    pub offset: u32,
    /// Resolved block-relative byte offset of this spell's **effect /
    /// animation descriptor**, or `None` when the entry has none. On disc the
    /// entry's `+0x04` field is a 1-based **index** (0 = none) into the
    /// per-block effect-offset table; this is the table slot already resolved
    /// to a block-relative offset (the value the loader then add-block-bases
    /// into the runtime `+0x04` pointer). See [`Self::aux_offset`] for `+0x08`.
    pub effect_offset: Option<u32>,
    /// Resolved block-relative byte offset of this spell's secondary descriptor
    /// (entry `+0x08`, same index-into-effect-offset-table scheme as
    /// [`Self::effect_offset`]), or `None`. Set on far fewer entries.
    pub aux_offset: Option<u32>,
}

impl MonsterSpell {
    /// True when the entry is an offensive castable spell (id `0x0C..=0x1F`)
    /// with a usable cost (`!= 0xFF`) — the slots the battle AI rolls over.
    pub fn is_castable(&self) -> bool {
        (0x0C..=0x1F).contains(&self.id) && self.sp_cost != 0xFF
    }
}

/// One monster's parsed stat record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterRecord {
    /// 1-based monster id (the archive slot index + 1).
    pub id: u16,
    /// Display name (control-prefix bytes `< 0x20` stripped; the retail
    /// names carry a leading `0x01` icon/color escape).
    pub name: String,
    /// Max HP.
    pub hp: u16,
    /// Max MP.
    pub mp: u16,
    /// The six stat halfwords at record `+0x0E/+0x12/+0x14/+0x16/+0x18/+0x1A`,
    /// in raw record order: `stats[0]` = SP, `stats[1]` = ATK,
    /// `stats[2]`/`stats[3]` = the defense pair (DEF↑/DEF↓), `stats[4]` = AGL
    /// (accuracy/evasion), `stats[5]` = SPD (turn-order speed). Prefer the
    /// named accessors below.
    pub stats: [u16; 6],
    /// Base gold reward (`+0x44`). Victory spoils scale this; see the
    /// module docs for the lone-enemy `(gold >> 1) / 2` formula.
    pub gold: u16,
    /// Base EXP reward (`+0x46`). Summed `* 3/4` then split among the party.
    pub exp: u16,
    /// Drop item id (`+0x48`; `0` = no drop).
    pub drop_item: u8,
    /// Drop chance in percent (`+0x49`; `rand() % 100 < drop_chance_pct`).
    pub drop_chance_pct: u8,
    /// Element id (`+0x1D`, `0..=7`): the battle loader copies this byte into
    /// the live actor's element slot (`actor[+0x1d]`, read by the affinity scale
    /// `FUN_801dd864`). Id space `earth=0/water=1/fire=2/wind=3/thunder=4/
    /// light=5/dark=6/neutral=7` — matches [`crate::element_affinity::Element`].
    /// Pinned by correlating this byte against the curated enemy elements across
    /// the whole roster (the four party-table ids fire/wind/thunder/neutral
    /// reproduce exactly; water/earth/light/dark corroborate per-element), and
    /// by the byte taking *only* values `0..=7` across every populated record.
    pub element: u8,
    /// Spell-slot count (`+0x4A`).
    pub magic_count: u8,
    /// The `magic_count` spell entries the `+0x4C` offset array points at.
    /// Empty when `magic_count == 0` or an offset falls outside the block.
    pub spells: Vec<MonsterSpell>,
    /// Global magic-attack ids the enemy AI casts by name. The record carries a
    /// 3-slot array at `+0x21..=+0x23`; a slot is a live attack when its value
    /// is `> 1` (`0` / `1` are empty / marker slots). Each id is a **global
    /// spell id** - the same value the AI writes into the live actor at
    /// `+0x1DF` and names through `&DAT_800754D0 + id*0xC`, so it resolves with
    /// [`crate::spell_names::SpellNameTable`] (`0x27` -> `Tail Fire`). This is
    /// distinct from the local `spells` ids above (the `+0x4C` action entries):
    /// those gate the SP cost, these carry the on-screen name. Pinned from the
    /// AI spell picker `FUN_801E9FD4` (`overlay_0898`).
    pub magic_attacks: Vec<u8>,
}

impl MonsterRecord {
    /// Attack (`stats[1]`, record `+0x12`). Read as the attacker's offensive
    /// value in the physical-damage routine (actor `+0x158`).
    pub fn attack(&self) -> u16 {
        self.stats[1]
    }

    /// High/upper defense (`stats[2]`, record `+0x14`, actor `+0x15C`). One of
    /// the two defense facets the damage routine selects by attack move index.
    pub fn defense_high(&self) -> u16 {
        self.stats[2]
    }

    /// Low/lower defense (`stats[3]`, record `+0x16`, actor `+0x160`). The
    /// other defense facet; the "Defense Up" buff raises it with `defense_high`.
    pub fn defense_low(&self) -> u16 {
        self.stats[3]
    }

    /// Agility (`stats[4]`, record `+0x18`, actor `+0x168`). Seeds the
    /// accuracy/evasion roll (`FUN_800402F4` selector 9).
    pub fn agility(&self) -> u16 {
        self.stats[4]
    }

    /// Turn-order speed (`stats[5]`, record `+0x1A`, actor `+0x164`). Seeds the
    /// per-turn initiative roll `+0x16C = speed + rand(0..speed/2) + 1`.
    pub fn speed(&self) -> u16 {
        self.stats[5]
    }

    /// SP / spirit-action gauge (`stats[0]`, record `+0x0E`, actor `+0x154`
    /// current / `+0x156` base). The AI spends it selecting spells; it
    /// regenerates to base each round and seeds the spirit-charge value.
    pub fn spirit(&self) -> u16 {
        self.stats[0]
    }
}

/// Number of `0x14000`-byte slots the archive can hold.
pub fn slot_count(entry: &[u8]) -> usize {
    entry.len() / SLOT_STRIDE
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn read_u16(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
}

/// Decode the monster id `id` (1-based) from the archive entry bytes.
///
/// Returns `Ok(None)` for an out-of-range id or an empty / filler slot
/// (one whose decoded block fails the record sanity checks). Returns
/// `Err` only when the slot claims a valid `dec_size` but the LZS stream
/// fails to decode to that length.
pub fn record(entry: &[u8], id: u16) -> Result<Option<MonsterRecord>> {
    match decode_block(entry, id)? {
        Some(block) => Ok(parse_block(id, &block)),
        None => Ok(None),
    }
}

/// LZS-decode monster id `id`'s archive slot into its raw block bytes.
///
/// Returns `Ok(None)` for an out-of-range id or an empty / filler slot (one
/// whose `dec_size` header fails the plausibility bounds). Returns `Err` only
/// when the slot claims a valid `dec_size` but the LZS stream fails to decode
/// to that length. Shared by [`record`] and [`mesh`].
fn decode_block(entry: &[u8], id: u16) -> Result<Option<Vec<u8>>> {
    if id == 0 {
        return Ok(None);
    }
    let slot = (id as usize - 1) * SLOT_STRIDE;
    let Some(dec_size) = read_u32(entry, slot) else {
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

/// Parse a decoded monster block into a [`MonsterRecord`]. Returns `None`
/// when the block fails the record sanity checks (empty / filler slot).
fn parse_block(id: u16, block: &[u8]) -> Option<MonsterRecord> {
    if block.len() < MIN_RECORD_BYTES {
        return None;
    }
    let name_offset = read_u32(block, 0)? as usize;
    // A real record's name offset points inside the block at a printable,
    // NUL-terminated string. Reject slots that don't.
    if name_offset == 0 || name_offset >= block.len() {
        return None;
    }
    let name = read_cstr(block, name_offset)?;
    if name.is_empty() {
        return None;
    }
    let hp = read_u16(block, 0x0C)?;
    let mp = read_u16(block, 0x10)?;
    let stats = [
        read_u16(block, 0x0E)?,
        read_u16(block, 0x12)?,
        read_u16(block, 0x14)?,
        read_u16(block, 0x16)?,
        read_u16(block, 0x18)?,
        read_u16(block, 0x1A)?,
    ];
    let element = *block.get(0x1D)?;
    let gold = read_u16(block, 0x44)?;
    let exp = read_u16(block, 0x46)?;
    let drop_item = *block.get(0x48)?;
    let drop_chance_pct = *block.get(0x49)?;
    let magic_count = *block.get(0x4A)?;
    let spells = parse_spells(block, magic_count);
    // Global magic-attack ids at +0x21..=+0x23; a slot is live when its value
    // is > 1 (0 / 1 are empty / marker slots), matching the AI picker's gate.
    let magic_attacks = (0x21..=0x23)
        .filter_map(|o| block.get(o).copied())
        .filter(|&b| b > 1)
        .collect();
    Some(MonsterRecord {
        id,
        name,
        hp,
        mp,
        stats,
        element,
        gold,
        exp,
        drop_item,
        drop_chance_pct,
        magic_count,
        spells,
        magic_attacks,
    })
}

/// Read the `+0x4C` spell-offset array (`magic_count` block-relative u32s) and
/// resolve each to a [`MonsterSpell`]. Offsets that fall outside the block (or
/// whose `+0x74` cost byte would read past the end) are skipped, so a partly
/// corrupt / filler record yields a shorter list rather than failing.
fn parse_spells(block: &[u8], magic_count: u8) -> Vec<MonsterSpell> {
    let mut out = Vec::with_capacity(magic_count as usize);
    for i in 0..magic_count as usize {
        let Some(offset) = read_u32(block, 0x4C + i * 4) else {
            break;
        };
        let entry = offset as usize;
        let (Some(&id), Some(&sp_cost)) = (block.get(entry), block.get(entry + 0x74)) else {
            continue;
        };
        out.push(MonsterSpell {
            id,
            sp_cost,
            offset,
            effect_offset: resolve_effect_offset(block, magic_count, read_u32(block, entry + 4)),
            aux_offset: resolve_effect_offset(block, magic_count, read_u32(block, entry + 8)),
        });
    }
    out
}

/// Resolve a spell entry's `+0x04` / `+0x08` **effect index** to a block-relative
/// byte offset, mirroring the battle loader's fixup (`FUN_800542C8`,
/// `ghidra/scripts/funcs/800542c8.txt`): the on-disc field is a 1-based index
/// (`0` = none) into the per-block **effect-offset table** that sits immediately
/// after the `+0x4C` spell-offset array (table word base `magic_count + 0x13`).
/// The loader computes `block[(index + magic_count + 0x12) * 4] + block_base`;
/// this returns the pre-`block_base` table value. `None` when the index is `0`,
/// the table slot is out of range, or the resolved offset is zero / past the
/// block.
fn resolve_effect_offset(block: &[u8], magic_count: u8, index: Option<u32>) -> Option<u32> {
    let index = index? as usize;
    if index == 0 {
        return None;
    }
    let word = index + magic_count as usize + 0x12;
    let off = read_u32(block, word * 4)?;
    if off == 0 || off as usize >= block.len() {
        return None;
    }
    Some(off)
}

/// Read a NUL-terminated monster name at `off` and clean it to a display
/// string. The on-disc names are printable ASCII carrying in-game text
/// escapes: a leading `^X` caret color-code (e.g. `^A `) and an optional
/// `$N` variant suffix (e.g. `Gimard $2`). The caret escapes are stripped;
/// the variant suffix is kept (it distinguishes `Gimard` from `Gimard $2`).
/// Returns `None` if the bytes aren't a plausible printable name.
fn read_cstr(block: &[u8], off: usize) -> Option<String> {
    let end = block[off..].iter().position(|&b| b == 0)? + off;
    let raw = &block[off..end];
    if raw.is_empty() || raw.len() > 32 {
        return None;
    }
    // The names are plain printable ASCII (caret escapes, `$`, letters,
    // digits, spaces). Reject anything else as a filler / non-name slot.
    if !raw.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    // Strip `^X` caret color-escape pairs.
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'^' && i + 1 < raw.len() {
            i += 2;
            continue;
        }
        out.push(raw[i] as char);
        i += 1;
    }
    let name = out.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Decode every populated monster slot in the archive. Skips empty / filler
/// slots silently; propagates an `Err` only on a genuine LZS decode failure.
pub fn records(entry: &[u8]) -> Result<Vec<MonsterRecord>> {
    let mut out = Vec::new();
    for id in 1..=slot_count(entry) as u16 {
        if let Some(rec) = record(entry, id)? {
            out.push(rec);
        }
    }
    Ok(out)
}

/// TMD magic of the Legaia variant the monster meshes use (custom PSX TMD).
const TMD_MAGIC: u32 = 0x8000_0002;

/// A monster's embedded 3D model, located inside its decoded archive block.
///
/// The monster mesh is a [Legaia TMD](../../tmd) stored verbatim in the block
/// at the offset held in the stat record's `+0x04` field (the same pointer the
/// battle loader fixes up into the actor's `+0x230` attack-effect/animation
/// data slot — the "0x1C-stride geometry records" walked by `FUN_80049858`
/// are this TMD's per-object table). The matching texture / CLUT pool is at
/// `+0x08`; [`texture`](MonsterMesh::texture) decodes it into palettes + a 4bpp
/// page (layout pinned from the loader `FUN_80055468`; see [`MonsterTexture`]).
#[derive(Debug, Clone)]
pub struct MonsterMesh {
    /// 1-based monster id (archive slot index + 1).
    pub id: u16,
    /// The full LZS-decoded archive block. The TMD and texture pool are slices
    /// into this buffer.
    pub block: Vec<u8>,
    /// Block-relative byte offset of the embedded TMD (stat record `+0x04`).
    pub tmd_offset: usize,
    /// Block-relative byte offset of the texture / CLUT pool (stat record
    /// `+0x08`). `0` when the record carries no pool pointer.
    pub texture_pool_offset: usize,
}

impl MonsterMesh {
    /// The embedded TMD bytes (from [`tmd_offset`](Self::tmd_offset) to the end
    /// of the block). The TMD parser stops at the model's own extent, so the
    /// trailing pool/spell bytes are harmless. Parse with `legaia_tmd::parse`.
    pub fn tmd_bytes(&self) -> &[u8] {
        &self.block[self.tmd_offset..]
    }

    /// The texture / CLUT pool bytes (from
    /// [`texture_pool_offset`](Self::texture_pool_offset) to the end of the
    /// block), or `None` when the record carries no pool pointer or the
    /// offset is out of range. See [`texture`](Self::texture) for the decoded
    /// palettes + 4bpp page.
    pub fn texture_pool_bytes(&self) -> Option<&[u8]> {
        if self.texture_pool_offset == 0 || self.texture_pool_offset >= self.block.len() {
            return None;
        }
        Some(&self.block[self.texture_pool_offset..])
    }

    /// Decode the texture pool into its palettes + 4bpp page.
    ///
    /// Returns `None` when there is no pool or it's too small to hold the CLUT
    /// region plus at least one texture row. See [`MonsterTexture`] for the
    /// layout and the `FUN_80055468` provenance.
    pub fn texture(&self) -> Option<MonsterTexture> {
        let pool = self.texture_pool_bytes()?;
        if pool.len() <= CLUT_REGION_BYTES {
            return None;
        }
        // 15 sequential 16-colour CLUTs at the head; the loader uploads the
        // whole 240-colour region to one VRAM row and a prim picks palette
        // `cba & 0x3F`. Index-0 colour 0x0000 is the PSX transparent texel.
        let palettes: Vec<[[u8; 4]; 16]> = (0..CLUT_COUNT)
            .map(|c| {
                let mut pal = [[0u8; 4]; 16];
                for (i, slot) in pal.iter_mut().enumerate() {
                    let raw = read_u16(pool, c * 32 + i * 2).unwrap_or(0);
                    *slot = bgr555_to_rgba(raw);
                }
                pal
            })
            .collect();

        // The 4bpp page is always 256 rows tall (StoreImage RECT.h); width is
        // whatever the remaining bytes divide into across those rows (64 B/row
        // = 128 texels for narrow monsters, 128 B/row = 256 texels for wide).
        let pixels = &pool[CLUT_REGION_BYTES..];
        let bytes_per_row = pixels.len() / TEXTURE_HEIGHT;
        if bytes_per_row == 0 {
            return None;
        }
        let width = bytes_per_row * 2;
        let mut indices = vec![0u8; width * TEXTURE_HEIGHT];
        for y in 0..TEXTURE_HEIGHT {
            for xb in 0..bytes_per_row {
                let b = pixels[y * bytes_per_row + xb];
                indices[y * width + xb * 2] = b & 0x0F;
                indices[y * width + xb * 2 + 1] = b >> 4;
            }
        }
        Some(MonsterTexture {
            palettes,
            indices,
            width,
            height: TEXTURE_HEIGHT,
        })
    }

    /// Build a renderable, battle-slot-relocated [`legaia_tmd::mesh::VramMesh`]
    /// for this monster and inject its texture pool into `vram` at the
    /// coordinates the battle loader `FUN_80055468` uses for `slot`.
    ///
    /// The on-disc CBA/TSB in the embedded TMD are nominal defaults; the
    /// loader relocates them per battle slot. This mirrors that relocation so
    /// the standard PSX VRAM texture path renders the monster directly:
    ///
    /// - the CLUT region ([`CLUT_REGION_BYTES`], 15 palettes) is written to
    ///   VRAM row `484 + slot` at x=0, and every prim's CBA is rewritten to
    ///   that row by [`relocate_cba`] (the palette index `cba & 0x3F` is kept);
    /// - the 4bpp page is written at [`monster_page_origin`] (`((5+slot)*64,
    ///   256)`), and every prim's TSB is rewritten to that texture page by
    ///   [`relocate_tsb`] (4bpp, `tpage_y = 256`, abr bits preserved).
    ///
    /// Per-vertex UVs are page-local and left untouched - they resolve
    /// correctly once the page sits at the relocated tpage origin. Returns
    /// `None` if the embedded TMD doesn't parse; otherwise a mesh with the
    /// relocated CBA/TSB (possibly empty if the monster has no textured
    /// prims). `slot` is the 0-based battle monster slot (`0..=4`).
    ///
    /// PORT: FUN_80055468
    pub fn battle_render_mesh(
        &self,
        slot: u8,
        vram: &mut legaia_tim::Vram,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let tmd = legaia_tmd::parse(self.tmd_bytes()).ok()?;
        let mut mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, self.tmd_bytes());

        // Inject the texture pool at the loader's per-slot VRAM coords so the
        // relocated CBA/TSB resolve against populated VRAM.
        if let Some(pool) = self.texture_pool_bytes()
            && pool.len() > CLUT_REGION_BYTES
        {
            vram.write_clut_row(0, monster_clut_row(slot), &pool[..CLUT_REGION_BYTES]);

            let page = &pool[CLUT_REGION_BYTES..];
            let bytes_per_row = page.len() / TEXTURE_HEIGHT;
            if bytes_per_row != 0 {
                let (page_x, page_y) = monster_page_origin(slot);
                // One VRAM cell is one halfword = 4 4bpp texels = 2 source
                // bytes, so the per-row cell count is `bytes_per_row / 2`.
                vram.write_block(
                    page_x,
                    page_y,
                    (bytes_per_row / 2) as u16,
                    TEXTURE_HEIGHT as u16,
                    page,
                );
            }
        }

        for ct in &mut mesh.cba_tsb {
            ct[0] = relocate_cba(ct[0], slot);
            ct[1] = relocate_tsb(ct[1], slot);
        }
        Some(mesh)
    }
}

/// VRAM row the battle loader (`FUN_80055468`) uploads a monster's CLUT
/// region to: row `484 + slot`, palettes packed from x=0.
pub const MONSTER_CLUT_ROW_BASE: u16 = 484;
/// Texture-page x-origin in VRAM tpage columns (64 px each). The loader bases
/// the monster page at 320 px = column 5, then offsets by the battle slot.
const MONSTER_PAGE_TPAGE_BASE: u16 = 5;
/// Texture-page y-origin in VRAM rows (always 256; the loader's StoreImage
/// `RECT.y`).
const MONSTER_PAGE_Y: u16 = 256;

/// VRAM row of the monster CLUT region for battle `slot`.
fn monster_clut_row(slot: u8) -> u16 {
    MONSTER_CLUT_ROW_BASE + slot as u16
}

/// Top-left `(x, y)` in VRAM pixels of the monster 4bpp texture page for
/// battle `slot`: `((5 + slot) * 64, 256)`.
pub fn monster_page_origin(slot: u8) -> (u16, u16) {
    ((MONSTER_PAGE_TPAGE_BASE + slot as u16) * 64, MONSTER_PAGE_Y)
}

/// Relocate a prim's CBA to battle `slot`: preserve the palette index
/// (`cba & 0x3F`) but point the CLUT row at `484 + slot` (where
/// [`MonsterMesh::battle_render_mesh`] writes the palettes).
pub fn relocate_cba(cba: u16, slot: u8) -> u16 {
    let palette = cba & 0x3F;
    (monster_clut_row(slot) << 6) | palette
}

/// Relocate a prim's TSB to battle `slot`: a 4bpp page at tpage column
/// `5 + slot`, `tpage_y = 256`, with the original abr (blend) bits preserved.
pub fn relocate_tsb(tsb: u16, slot: u8) -> u16 {
    let abr = (tsb >> 5) & 0x3;
    let tpage_x_field = (MONSTER_PAGE_TPAGE_BASE + slot as u16) & 0xF;
    // tpage column (bits 0..3); tpage_y=256 -> bit 4; depth bits 7..8 = 0 (4bpp).
    tpage_x_field | (1 << 4) | (abr << 5)
}

/// Size of the CLUT region at the head of the texture pool: 15 sequential
/// 16-colour palettes (`15 * 16 * 2` bytes). The loader (`FUN_80055468`)
/// uploads this region to VRAM row `484 + battle_slot`, 256 colours wide.
pub const CLUT_REGION_BYTES: usize = 0x1E0;
/// Number of 16-colour palettes in the CLUT region. A prim selects palette
/// `cba & 0x3F`; the rest are zero-padded for monsters that use fewer.
pub const CLUT_COUNT: usize = 15;
/// Texture-page height in texels. Always 256 (the `FUN_80055468` StoreImage
/// `RECT.h`); the page width varies (128 or 256 texels).
pub const TEXTURE_HEIGHT: usize = 256;

/// Convert a PSX BGR555 colour to RGBA8. The all-zero colour (`0x0000`) is the
/// PSX transparent texel and maps to alpha 0; every other colour is opaque.
fn bgr555_to_rgba(v: u16) -> [u8; 4] {
    let r = ((v & 0x1F) << 3) as u8;
    let g = (((v >> 5) & 0x1F) << 3) as u8;
    let b = (((v >> 10) & 0x1F) << 3) as u8;
    let a = if v == 0 { 0 } else { 255 };
    [r, g, b, a]
}

/// A monster's decoded battle texture: the palette set plus the 4bpp page.
///
/// Reverse-engineered from the battle loader `FUN_80055468` (see
/// `ghidra/scripts/funcs/80055468.txt`), which the streaming archive loader
/// `FUN_800542C8` calls with the pool pointer (record `+0x08`), the embedded
/// TMD (record `+0x04`), and the battle slot index. The pool is laid out as:
///
/// ```text
/// +0x000  15 x [16 BGR555 colours]   ; CLUT region (0x1E0 bytes, zero-padded)
/// +0x1E0  4bpp indices               ; width x 256 texels, row-major
/// ```
///
/// A textured prim references CLUT base `cba` (palette = `cba & 0x3F`) and
/// samples the page at its per-vertex `(u, v)`; index 0 is transparent.
#[derive(Debug, Clone)]
pub struct MonsterTexture {
    /// The 15 palettes, each 16 RGBA8 colours. A prim with CLUT base `cba`
    /// uses `palettes[(cba & 0x3F) as usize]` (clamp to `CLUT_COUNT`).
    pub palettes: Vec<[[u8; 4]; 16]>,
    /// One 4bpp palette index per texel, row-major (`width * height` bytes).
    pub indices: Vec<u8>,
    /// Page width in texels (128 for narrow monsters, 256 for wide ones).
    pub width: usize,
    /// Page height in texels (always [`TEXTURE_HEIGHT`] = 256).
    pub height: usize,
}

impl MonsterTexture {
    /// Bake the page into a flat RGBA8 image using the given palette index
    /// (`cba & 0x3F` of the prim you want to preview). Transparent texels keep
    /// alpha 0. `width * height * 4` bytes, row-major top-to-bottom.
    pub fn to_rgba(&self, palette: usize) -> Vec<u8> {
        let pal = &self.palettes[palette.min(self.palettes.len() - 1)];
        let mut out = Vec::with_capacity(self.indices.len() * 4);
        for &idx in &self.indices {
            out.extend_from_slice(&pal[idx as usize]);
        }
        out
    }

    /// Flatten the 15 palettes into a single `15 * 16` RGBA8 row, suitable for
    /// uploading as a palette lookup texture (palette `p`, colour `c` is at
    /// pixel `p * 16 + c`).
    pub fn palette_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CLUT_COUNT * 16 * 4);
        for pal in &self.palettes {
            for colour in pal {
                out.extend_from_slice(colour);
            }
        }
        out
    }
}

/// Locate monster id `id`'s embedded 3D mesh.
///
/// Returns `Ok(None)` for an out-of-range id, an empty / filler slot, or a slot
/// whose `+0x04` pointer does not land on a TMD magic (`0x80000002`). Returns
/// `Err` only on a genuine LZS decode failure. The mesh is a Legaia TMD; see
/// [`MonsterMesh`].
pub fn mesh(entry: &[u8], id: u16) -> Result<Option<MonsterMesh>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    // The stat record's +0x04 holds the block-relative TMD offset (and +0x08
    // the texture pool). Validate the TMD magic before trusting the pointer so
    // filler / non-mesh slots return None rather than a bogus offset.
    let Some(tmd_offset) = read_u32(&block, 0x04).map(|v| v as usize) else {
        return Ok(None);
    };
    if tmd_offset + 4 > block.len() || read_u32(&block, tmd_offset) != Some(TMD_MAGIC) {
        return Ok(None);
    }
    let texture_pool_offset = read_u32(&block, 0x08).unwrap_or(0) as usize;
    Ok(Some(MonsterMesh {
        id,
        block,
        tmd_offset,
        texture_pool_offset,
    }))
}

// ---------------------------------------------------------------------------
// Animation (per-object transform keyframes)
// ---------------------------------------------------------------------------

/// One animated object's transform for a single keyframe.
///
/// The battle renderer treats each TMD object as a rigid part and poses it with
/// a translation + a Euler rotation. The decoder (`FUN_8004998c`) unpacks one
/// part's 9 bytes into six 12-bit fields: `[tx, ty, tz]` are **sign-extended**
/// (signed translation in TMD model units) and `[rx, ry, rz]` are **unsigned
/// 12-bit angles** (`4096` = a full turn). The renderer interpolates these
/// across frames (linear for translation, shortest-path angle-wrap for
/// rotation) and applies them per object via the GTE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartPose {
    /// Translation X (signed, TMD model units).
    pub tx: i16,
    /// Translation Y.
    pub ty: i16,
    /// Translation Z.
    pub tz: i16,
    /// Rotation about X (`0..4096` = `0..360°`).
    pub rx: u16,
    /// Rotation about Y.
    pub ry: u16,
    /// Rotation about Z.
    pub rz: u16,
}

/// One monster action's animation: `frame_count` keyframes, each holding a
/// [`PartPose`] for every animated object (`part_count`).
///
/// Sourced from a per-action entry's packed stream at entry `+0x8c` (the
/// entries the `+0x4C` offset array points at — see [`MonsterRecord::spells`]).
/// The stream head is `[u8 part_count][u8 frame_count]` followed by
/// `frame_count * part_count` nine-byte part records. `part_count` matches the
/// monster TMD's object count (one part per object). Action **index 0** is the
/// neutral **idle** animation the engine loops when the monster isn't acting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterAnimation {
    /// Action id (entry `+0x00`): `0` idle, `1` basic attack, then the spell /
    /// special actions (matching [`MonsterSpell::id`]).
    pub action_id: u8,
    /// Number of animated objects per frame (one per TMD object).
    pub part_count: usize,
    /// Number of keyframes.
    pub frame_count: usize,
    /// `frame_count` frames, each `part_count` [`PartPose`]s (`frames[f][p]`).
    pub frames: Vec<Vec<PartPose>>,
}

impl MonsterAnimation {
    /// The poses for frame `f` (one per part), or `None` if out of range.
    pub fn frame(&self, f: usize) -> Option<&[PartPose]> {
        self.frames.get(f).map(|v| v.as_slice())
    }
}

/// Offset of the packed animation stream inside a per-action entry.
const ANIM_STREAM_OFFSET: usize = 0x8c;
/// Bytes per part record in the packed stream (six 12-bit fields).
const ANIM_PART_STRIDE: usize = 9;

/// Sign-extend a 12-bit field to `i16`.
fn sx12(v: u16) -> i16 {
    if v & 0x800 != 0 {
        (v | 0xf000) as i16
    } else {
        v as i16
    }
}

/// Unpack one nine-byte part record into its six 12-bit fields, mirroring the
/// bit layout in `FUN_8004998c`: low bytes at `[0,1,3,4,6,7]`, the high nibbles
/// packed into `[2,5,8]`.
fn unpack_part(b: &[u8]) -> PartPose {
    let v0 = b[0] as u16 | ((b[2] as u16 & 0x0f) << 8);
    let v1 = b[1] as u16 | ((b[2] as u16 & 0xf0) << 4);
    let v2 = b[3] as u16 | ((b[5] as u16 & 0x0f) << 8);
    let v3 = b[4] as u16 | ((b[5] as u16 & 0xf0) << 4);
    let v4 = b[6] as u16 | ((b[8] as u16 & 0x0f) << 8);
    let v5 = b[7] as u16 | ((b[8] as u16 & 0xf0) << 4);
    PartPose {
        tx: sx12(v0),
        ty: sx12(v1),
        tz: sx12(v2),
        rx: v3 & 0xfff,
        ry: v4 & 0xfff,
        rz: v5 & 0xfff,
    }
}

/// Parse one per-action entry's packed animation stream at block offset
/// `entry_off`. Returns `None` when the stream head or frame data falls outside
/// the block, or the part/frame counts are zero.
fn parse_animation(block: &[u8], action_id: u8, entry_off: usize) -> Option<MonsterAnimation> {
    let s = entry_off + ANIM_STREAM_OFFSET;
    let part_count = *block.get(s)? as usize;
    let frame_count = *block.get(s + 1)? as usize;
    if part_count == 0 || frame_count == 0 {
        return None;
    }
    let data = s + 2;
    let need = frame_count * part_count * ANIM_PART_STRIDE;
    if data + need > block.len() {
        return None;
    }
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut parts = Vec::with_capacity(part_count);
        for p in 0..part_count {
            let o = data + (f * part_count + p) * ANIM_PART_STRIDE;
            parts.push(unpack_part(&block[o..o + ANIM_PART_STRIDE]));
        }
        frames.push(parts);
    }
    Some(MonsterAnimation {
        action_id,
        part_count,
        frame_count,
        frames,
    })
}

/// Decode every action animation for monster id `id`, in `+0x4C`-array order.
///
/// Each entry in the monster's action/spell table (`magic_count` of them)
/// carries a packed transform-keyframe stream at entry `+0x8c`; this returns
/// one [`MonsterAnimation`] per entry that decodes cleanly (a malformed or
/// empty stream is skipped, so the returned indices may be sparser than the
/// raw table). Returns `Ok(None)` for an empty / filler / non-mesh slot.
pub fn animations(entry: &[u8], id: u16) -> Result<Option<Vec<MonsterAnimation>>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    if block.len() < MIN_RECORD_BYTES {
        return Ok(None);
    }
    let magic_count = block[0x4a] as usize;
    let mut out = Vec::with_capacity(magic_count);
    for i in 0..magic_count {
        let Some(entry_off) = read_u32(&block, 0x4c + i * 4).map(|v| v as usize) else {
            break;
        };
        let Some(&action_id) = block.get(entry_off) else {
            continue;
        };
        if let Some(anim) = parse_animation(&block, action_id, entry_off) {
            out.push(anim);
        }
    }
    Ok(Some(out))
}

/// Short, display-ready labels for a monster's decoded animations, parallel to
/// the slice returned by [`animations`]. Index 0 is the idle loop; `action_id`
/// `1` is the basic attack; the rest are the monster's spell / special actions
/// (`Action 0xNN`). When two entries would share a label (some monsters carry
/// several actions with the same `action_id`), a ` #N` suffix disambiguates so
/// every label is unique — handy for toggle buttons and glTF animation names.
pub fn action_labels(anims: &[MonsterAnimation]) -> Vec<String> {
    use std::collections::HashMap;
    let base: Vec<String> = anims
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 {
                "Idle".to_string()
            } else if a.action_id == 1 {
                "Attack".to_string()
            } else {
                format!("Action 0x{:02X}", a.action_id)
            }
        })
        .collect();
    let mut totals: HashMap<String, usize> = HashMap::new();
    for b in &base {
        *totals.entry(b.clone()).or_default() += 1;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    base.into_iter()
        .map(|b| {
            if totals.get(&b).copied().unwrap_or(0) > 1 {
                let n = seen.entry(b.clone()).or_default();
                *n += 1;
                format!("{b} #{n}")
            } else {
                b
            }
        })
        .collect()
}

/// Decode just the **idle** animation (action index 0) for monster id `id`.
///
/// This is the neutral pose loop the battle engine plays when the monster is
/// not performing a move. Returns `Ok(None)` if the slot is empty or carries no
/// decodable action animations.
pub fn idle_animation(entry: &[u8], id: u16) -> Result<Option<MonsterAnimation>> {
    Ok(animations(entry, id)?.and_then(|mut a| {
        if a.is_empty() {
            None
        } else {
            Some(a.swap_remove(0))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-slot archive whose head record is a known monster, LZS
    /// stored verbatim (the Legaia LZS decoder round-trips an
    /// uncompressed-flagged stream). We instead lean on a tiny hand-rolled
    /// block and only exercise the byte parser via [`parse_block`].
    #[test]
    fn parse_block_reads_named_record() {
        // Big enough to hold the name at 0x80 plus two spell entries past it.
        let mut block = vec![0u8; 0x200];
        // name at 0x80 (clear of the reward fields at 0x44..0x4A); +0x04
        // effect-data offset is not parsed into a field.
        block[0x00..0x04].copy_from_slice(&0x80u32.to_le_bytes());
        block[0x04..0x08].copy_from_slice(&0x40u32.to_le_bytes());
        block[0x0C..0x0E].copy_from_slice(&99u16.to_le_bytes()); // HP
        block[0x0E..0x10].copy_from_slice(&60u16.to_le_bytes()); // stat0
        block[0x10..0x12].copy_from_slice(&20u16.to_le_bytes()); // MP
        block[0x12..0x14].copy_from_slice(&23u16.to_le_bytes()); // stat1
        block[0x14..0x16].copy_from_slice(&12u16.to_le_bytes());
        block[0x16..0x18].copy_from_slice(&15u16.to_le_bytes());
        block[0x18..0x1A].copy_from_slice(&16u16.to_le_bytes());
        block[0x1A..0x1C].copy_from_slice(&22u16.to_le_bytes());
        block[0x44..0x46].copy_from_slice(&60u16.to_le_bytes()); // gold
        block[0x46..0x48].copy_from_slice(&55u16.to_le_bytes()); // exp
        block[0x48] = 119; // drop item id
        block[0x49] = 10; // drop chance %
        block[0x4A] = 2; // magic count
        // +0x4C spell-offset array: two block-relative offsets.
        block[0x4C..0x50].copy_from_slice(&0x100u32.to_le_bytes());
        block[0x50..0x54].copy_from_slice(&0x180u32.to_le_bytes());
        // name "^A Gimard\0" at 0x80 (caret color-escape + space stripped).
        block[0x80..0x89].copy_from_slice(b"^A Gimard");
        // spell entry 0 @ 0x100: id 0x0D (castable), SP cost 12.
        block[0x100] = 0x0D;
        block[0x100 + 0x74] = 12;
        // spell entry 1 @ 0x180: id 0x03 (elemental affinity), cost 0xFF.
        block[0x180] = 0x03;
        block[0x180 + 0x74] = 0xFF;

        let rec = parse_block(10, &block).expect("record parses");
        assert_eq!(rec.id, 10);
        assert_eq!(rec.name, "Gimard");
        assert_eq!(rec.hp, 99);
        assert_eq!(rec.mp, 20);
        assert_eq!(rec.stats, [60, 23, 12, 15, 16, 22]);
        assert_eq!(rec.magic_count, 2);
        assert_eq!(rec.attack(), 23);
        assert_eq!(rec.defense_high(), 12);
        assert_eq!(rec.speed(), 22);
        assert_eq!(rec.spirit(), 60);
        assert_eq!(rec.gold, 60);
        assert_eq!(rec.exp, 55);
        assert_eq!(rec.drop_item, 119);
        assert_eq!(rec.drop_chance_pct, 10);
        assert_eq!(
            rec.spells,
            vec![
                MonsterSpell {
                    id: 0x0D,
                    sp_cost: 12,
                    offset: 0x100,
                    effect_offset: None,
                    aux_offset: None,
                },
                MonsterSpell {
                    id: 0x03,
                    sp_cost: 0xFF,
                    offset: 0x180,
                    effect_offset: None,
                    aux_offset: None,
                },
            ]
        );
        assert!(rec.spells[0].is_castable());
        assert!(!rec.spells[1].is_castable());
    }

    #[test]
    fn parse_block_rejects_filler() {
        // All-zero block: name_offset 0 -> rejected.
        assert!(parse_block(1, &[0u8; 0x60]).is_none());
        // Too short.
        assert!(parse_block(1, &[0u8; 8]).is_none());
    }

    #[test]
    fn spell_effect_index_resolves_through_table() {
        // magic_count = 2 -> effect-offset table word base = 2 + 0x13 = 0x15
        // (byte 0x54), immediately after the two-entry spell-offset array.
        let mc: u8 = 2;
        let mut block = vec![0u8; 0x200];
        // Spell entry @ 0x100: a +0x04 index of 1, +0x08 index of 0 (none).
        block[0x100 + 4] = 1;
        // table[index-1=0] @ byte 0x54: a block-relative offset of 0x1C0.
        block[0x54..0x58].copy_from_slice(&0x1C0u32.to_le_bytes());
        // table[1] @ 0x58: 0 -> would resolve to None if referenced.
        assert_eq!(
            resolve_effect_offset(&block, mc, read_u32(&block, 0x100 + 4)),
            Some(0x1C0)
        );
        assert_eq!(
            resolve_effect_offset(&block, mc, read_u32(&block, 0x100 + 8)),
            None,
            "index 0 -> none"
        );
        // An index whose table slot is zero resolves to None.
        block[0x100 + 4] = 2;
        assert_eq!(
            resolve_effect_offset(&block, mc, read_u32(&block, 0x100 + 4)),
            None,
            "zero table slot -> none"
        );
        // An out-of-range resolved offset is rejected.
        block[0x58..0x5C].copy_from_slice(&0x9999u32.to_le_bytes());
        assert_eq!(
            resolve_effect_offset(&block, mc, read_u32(&block, 0x100 + 4)),
            None,
            "offset past block end -> none"
        );
    }

    #[test]
    fn unpack_part_splits_six_12bit_fields() {
        // Low bytes at [0,1,3,4,6,7]; high nibbles packed into [2,5,8].
        // Field i = low | (nibble << 8). tx/ty/tz sign-extend; rx/ry/rz don't.
        // b2=0x81 -> v0 high nibble 0x1 (0x081=129), v1 high nibble 0x8 (0x812).
        let b = [0x80, 0x12, 0x81, 0x34, 0x56, 0x07, 0x9a, 0xbc, 0x21];
        let p = unpack_part(&b);
        assert_eq!(p.tx, 0x180); // 0x80 | (0x1<<8) = 0x180, positive
        assert_eq!(p.ty, sx12(0x812)); // 0x12 | (0x8<<8) = 0x812, sign bit set -> negative
        assert_eq!(p.tz, 0x734); // 0x34 | (0x7<<8)
        assert_eq!(p.rx, 0x056); // 0x56 | (0x0<<8) (rotation, unsigned)
        assert_eq!(p.ry, 0x19a); // 0x9a | (0x1<<8)
        assert_eq!(p.rz, 0x2bc); // 0xbc | (0x2<<8)
    }

    #[test]
    fn parse_animation_reads_part_and_frame_counts() {
        // Build an entry whose +0x8c stream is [parts=2][frames=3][3*2 parts].
        let mut block = vec![0u8; 0x8c + 2 + 3 * 2 * ANIM_PART_STRIDE + 4];
        let s = 0x8c;
        block[s] = 2; // part_count
        block[s + 1] = 3; // frame_count
        // Frame 1, part 0: tx low byte 0x10 at its slot (frame 1 * 2 parts).
        let f1p0 = s + 2 + 2 * ANIM_PART_STRIDE;
        block[f1p0] = 0x10;
        let anim = parse_animation(&block, 0x00, 0).expect("animation parses");
        assert_eq!(anim.action_id, 0x00);
        assert_eq!(anim.part_count, 2);
        assert_eq!(anim.frame_count, 3);
        assert_eq!(anim.frames.len(), 3);
        assert_eq!(anim.frames[0].len(), 2);
        assert_eq!(anim.frame(1).unwrap()[0].tx, 0x10);
        // Out-of-range / zero-count streams yield None.
        assert!(parse_animation(&[0u8; 0x8c + 2], 0, 0).is_none());
    }

    #[test]
    fn read_cstr_strips_caret_escapes_keeps_variant() {
        let mut b = vec![0u8; 0x20];
        b[..6].copy_from_slice(b"Hornet");
        assert_eq!(read_cstr(&b, 0).as_deref(), Some("Hornet"));
        // Caret color-escape + space prefix stripped; `$N` variant kept.
        let mut g = vec![0u8; 0x20];
        g[..12].copy_from_slice(b"^A Gimard $2");
        assert_eq!(read_cstr(&g, 0).as_deref(), Some("Gimard $2"));
    }

    /// `relocate_cba` keeps the palette index but re-homes the CLUT row to
    /// `484 + slot`, matching where `battle_render_mesh` writes the palettes.
    #[test]
    fn relocate_cba_preserves_palette_and_sets_row() {
        for slot in 0u8..5 {
            for palette in 0u16..15 {
                // Build an on-disc CBA with that palette and an arbitrary
                // (to-be-discarded) original row of 256.
                let on_disc = (256u16 << 6) | palette;
                let relocated = relocate_cba(on_disc, slot);
                // Decode the way `Prim::cba_xy` does.
                assert_eq!(relocated & 0x3F, palette, "palette preserved");
                assert_eq!(
                    (relocated >> 6) & 0x1FF,
                    MONSTER_CLUT_ROW_BASE + slot as u16,
                    "CLUT row = 484 + slot"
                );
            }
        }
    }

    /// `relocate_tsb` points the page at tpage column `5 + slot`, `tpage_y =
    /// 256`, 4bpp depth, and preserves the original abr bits.
    #[test]
    fn relocate_tsb_sets_page_and_keeps_abr() {
        for slot in 0u8..5 {
            for abr in 0u16..4 {
                // On-disc TSB with some other column, 8bpp, tpage_y=0.
                let on_disc = 0x03 | (abr << 5) | (1 << 7);
                let relocated = relocate_tsb(on_disc, slot);
                // Decode the way `Prim::tpage_xy` does.
                let tpage_x = (relocated & 0xF) * 64;
                let tpage_y = ((relocated >> 4) & 1) * 256;
                let depth = (relocated >> 7) & 0x3; // 0 == 4bpp
                let abr_out = (relocated >> 5) & 0x3;
                assert_eq!(tpage_x, monster_page_origin(slot).0, "page x = (5+slot)*64");
                assert_eq!(tpage_y, 256, "tpage_y = 256");
                assert_eq!(depth, 0, "4bpp depth");
                assert_eq!(abr_out, abr, "abr preserved");
            }
        }
    }

    /// The texture page never overlaps any slot's CLUT row: palettes occupy
    /// x in `0..240`, pages start at x>=320, so injection slots are disjoint.
    #[test]
    fn monster_page_clear_of_clut_region() {
        for slot in 0u8..5 {
            let (px, py) = monster_page_origin(slot);
            assert!(px >= 320, "page x past the 240-wide CLUT region");
            assert_eq!(py, 256);
            assert!(MONSTER_CLUT_ROW_BASE + slot as u16 >= 484);
        }
    }
}
