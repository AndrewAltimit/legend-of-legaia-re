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
//! +0x44  u16  gold          ; base gold reward (victory spoils)
//! +0x46  u16  exp           ; base EXP reward (victory spoils)
//! +0x48  u8   drop_item     ; drop item id (0 = no drop)
//! +0x49  u8   drop_chance   ; drop chance, percent (rand()%100 < pct)
//! +0x4A  u8   magic_count   ; spell-entry count
//! +0x4C  u32[] spell_offsets ; element-resistance source (first byte = element)
//! ```
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
    /// Spell-slot count (`+0x4A`).
    pub magic_count: u8,
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
    Ok(parse_block(id, &block))
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
    let gold = read_u16(block, 0x44)?;
    let exp = read_u16(block, 0x46)?;
    let drop_item = *block.get(0x48)?;
    let drop_chance_pct = *block.get(0x49)?;
    let magic_count = *block.get(0x4A)?;
    Some(MonsterRecord {
        id,
        name,
        hp,
        mp,
        stats,
        gold,
        exp,
        drop_item,
        drop_chance_pct,
        magic_count,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-slot archive whose head record is a known monster, LZS
    /// stored verbatim (the Legaia LZS decoder round-trips an
    /// uncompressed-flagged stream). We instead lean on a tiny hand-rolled
    /// block and only exercise the byte parser via [`parse_block`].
    #[test]
    fn parse_block_reads_named_record() {
        let mut block = vec![0u8; 0x60];
        // name at 0x50 (clear of the reward fields at 0x44..0x4A); +0x04
        // effect-data offset is not parsed into a field.
        block[0x00..0x04].copy_from_slice(&0x50u32.to_le_bytes());
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
        block[0x4A] = 9; // magic count
        // name "^A Gimard\0" at 0x40 (caret color-escape + space stripped).
        block[0x50..0x59].copy_from_slice(b"^A Gimard");

        let rec = parse_block(10, &block).expect("record parses");
        assert_eq!(rec.id, 10);
        assert_eq!(rec.name, "Gimard");
        assert_eq!(rec.hp, 99);
        assert_eq!(rec.mp, 20);
        assert_eq!(rec.stats, [60, 23, 12, 15, 16, 22]);
        assert_eq!(rec.magic_count, 9);
        assert_eq!(rec.attack(), 23);
        assert_eq!(rec.defense_high(), 12);
        assert_eq!(rec.speed(), 22);
        assert_eq!(rec.spirit(), 60);
        assert_eq!(rec.gold, 60);
        assert_eq!(rec.exp, 55);
        assert_eq!(rec.drop_item, 119);
        assert_eq!(rec.drop_chance_pct, 10);
    }

    #[test]
    fn parse_block_rejects_filler() {
        // All-zero block: name_offset 0 -> rejected.
        assert!(parse_block(1, &[0u8; 0x60]).is_none());
        // Too short.
        assert!(parse_block(1, &[0u8; 8]).is_none());
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
}
