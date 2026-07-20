//! Monster stat record: the decoded-block head parser and accessors.

use anyhow::Result;

use super::{MIN_RECORD_BYTES, SLOT_STRIDE, decode_block};

/// One spell entry referenced by a monster record's `+0x4C` offset array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterSpell {
    /// Spell/action id (entry `+0x00`). Ids `2,3,4,5,0x0B` mark an elemental
    /// resist/affinity, `0x0C..=0x1F` are offensive castable spells, `0x23`
    /// (`'#'`) is a special category.
    pub id: u8,
    /// AGL (action) cost (entry `+0x74`) - spent from the actor's AGL gauge.
    /// `0xFF` = unavailable (the AI never picks it; a non-castable / passive
    /// slot).
    pub agl_cost: u8,
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
    /// with a usable cost (`!= 0xFF`) - the slots the battle AI rolls over.
    pub fn is_castable(&self) -> bool {
        (0x0C..=0x1F).contains(&self.id) && self.agl_cost != 0xFF
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
    /// in raw record order: `stats[0]` = AGL (agility/action gauge),
    /// `stats[1]` = ATK, `stats[2]`/`stats[3]` = the defense pair (DEF↑/DEF↓),
    /// `stats[4]` = INT (accuracy/evasion seed), `stats[5]` = SPD (turn-order
    /// speed). Prefer the named accessors below.
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
    /// Element id (`+0x1D`, `0..=7`): the affinity scale `FUN_801dd864` reads
    /// this byte **directly from the record** (via the per-enemy record-pointer
    /// table `0x801C9348[slot-3]`, dump `overlay_battle_action_801dd864.txt`
    /// `0x801dd8dc`) - it is **not** copied into a live-actor field the way the
    /// `+0x0E..+0x1A` stats are. Id space `earth=0/water=1/fire=2/wind=3/
    /// thunder=4/light=5/dark=6/neutral=7` - matches
    /// [`crate::element_affinity::Element`].
    /// Pinned by correlating this byte against the curated enemy elements across
    /// the whole roster (the four party-table ids fire/wind/thunder/neutral
    /// reproduce exactly; water/earth/light/dark corroborate per-element), and
    /// by the byte taking *only* values `0..=7` across every populated record.
    pub element: u8,
    /// Body-size / bulk class (`+0x1F`). Like [`Self::element`] this byte is
    /// read **record-direct** through the per-enemy record-pointer table
    /// `0x801C9348[slot-3]` and is never copied into a live-actor field.
    ///
    /// Two consumers, both scaling the same magnitude:
    /// - battle-camera framing `FUN_801F0348` (`overlay_battle_action_801f0348.txt`,
    ///   `0x801f03ac` / `0x801f03f4`): `ctx+0x6D0 = size << 7`, clamped to
    ///   `0x0C00..=0x1400`, so only `0x18..=0x28` is an active band.
    /// - the enemy stager `FUN_800513F0` (`0x800518c4`): `actor+0x58 = size << 5`.
    ///
    /// Across the retail roster the byte spans `14..=48` with no zero and no
    /// outlier, and it tracks model bulk rather than any stat: the bee / bat
    /// family sits at `14`, Caruban at `46` and Koru at `48`, while Lapis
    /// (64800 HP) sits at `20`.
    pub size_class: u8,
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
    /// those gate the AGL cost, these carry the on-screen name. Pinned from the
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

    /// Intelligence (`stats[4]`, record `+0x18`, actor `+0x168`) - the **INT**
    /// stat. Meth962's walkthrough: INT "affects your magical damage and defense
    /// against other magical spells". The binary bears this out - the summon/arts
    /// damage kernel (`FUN_801dd0ac`) reads the attacker's INT as a damage term
    /// and the defender's INT as a mitigation (magic-defense) term; it also seeds
    /// the accuracy/evasion roll (`FUN_800402F4` selector 9). Curated
    /// `enemies.toml` `int` column byte-matches it ×9/8.
    pub fn intelligence(&self) -> u16 {
        self.stats[4]
    }

    /// Turn-order speed (`stats[5]`, record `+0x1A`, actor `+0x164`). Seeds the
    /// per-turn initiative roll `+0x16C = speed + rand(0..speed/2) + 1`.
    pub fn speed(&self) -> u16 {
        self.stats[5]
    }

    /// AGL / agility (action) gauge (`stats[0]`, record `+0x0E`, actor `+0x154`
    /// current / `+0x156` base). Every action spends it; it resets to base each
    /// round, and the "Power Up" buff raises it ("agility increased!"). This is
    /// the gauge fan bestiaries label AGL.
    pub fn agility(&self) -> u16 {
        self.stats[0]
    }

    /// The six stats as the battle loader **installs them into the live actor**,
    /// in [`stats`](Self::stats) order. The raw record bytes are *not* what the
    /// player fights: `FUN_80054cb0` boosts four of the six combat stats while
    /// copying the record into the battle actor (see the module's *Battle-load
    /// stat boost* note). This returns the **boosted
    /// (gate-set) profile** - the one the international retail build uses for the
    /// captured fights, and the one the curated `enemies.toml` bestiary matches
    /// byte-for-byte:
    ///
    /// - `attack`   += `attack >> 2`   (`×5/4`, truncating)
    /// - `defense_high` `× 2` (the **upper** defense, UDF)
    /// - `defense_low`  `× 2` (the **lower** defense, LDF)
    /// - `intelligence` += `int >> 3`  (`×9/8`, truncating)
    /// - `agility` (AGL), HP, MP and `speed` (SPD) are copied unchanged.
    ///
    /// Each op is a 16-bit truncating integer step matching the MIPS exactly
    /// (`wrapping` so a degenerate record can't panic; no real record overflows).
    /// The alternate gate-clear profile (`DEF ×7/4`, `INT ×5/4`, ATK unchanged)
    /// is documented in the module note but not produced here - both profiles
    /// boost, so the raw record always understates the fight.
    pub fn battle_stats(&self) -> [u16; 6] {
        let s = self.stats;
        [
            s[0],                         // AGL  - copied unchanged
            s[1].wrapping_add(s[1] >> 2), // ATK  + ATK>>2   (×5/4)
            s[2].wrapping_mul(2),         // UDF  ×2
            s[3].wrapping_mul(2),         // LDF  ×2
            s[4].wrapping_add(s[4] >> 3), // INT  + INT>>3   (×9/8)
            s[5],                         // SPD  - copied unchanged
        ]
    }
}

/// Number of `0x14000`-byte slots the archive can hold.
pub fn slot_count(entry: &[u8]) -> usize {
    entry.len() / SLOT_STRIDE
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

/// Parse a decoded monster block into a [`MonsterRecord`]. Returns `None`
/// when the block fails the record sanity checks (empty / filler slot).
fn parse_block(id: u16, block: &[u8]) -> Option<MonsterRecord> {
    if block.len() < MIN_RECORD_BYTES {
        return None;
    }
    let name_offset = legaia_bytes::u32_le(block, 0)? as usize;
    // A real record's name offset points inside the block at a printable,
    // NUL-terminated string. Reject slots that don't.
    if name_offset == 0 || name_offset >= block.len() {
        return None;
    }
    let name = read_cstr(block, name_offset)?;
    if name.is_empty() {
        return None;
    }
    let hp = legaia_bytes::u16_le(block, 0x0C)?;
    let mp = legaia_bytes::u16_le(block, 0x10)?;
    let stats = [
        legaia_bytes::u16_le(block, 0x0E)?,
        legaia_bytes::u16_le(block, 0x12)?,
        legaia_bytes::u16_le(block, 0x14)?,
        legaia_bytes::u16_le(block, 0x16)?,
        legaia_bytes::u16_le(block, 0x18)?,
        legaia_bytes::u16_le(block, 0x1A)?,
    ];
    let element = *block.get(0x1D)?;
    let size_class = *block.get(0x1F)?;
    let gold = legaia_bytes::u16_le(block, 0x44)?;
    let exp = legaia_bytes::u16_le(block, 0x46)?;
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
        size_class,
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
        let Some(offset) = legaia_bytes::u32_le(block, 0x4C + i * 4) else {
            break;
        };
        let entry = offset as usize;
        let (Some(&id), Some(&agl_cost)) = (block.get(entry), block.get(entry + 0x74)) else {
            continue;
        };
        out.push(MonsterSpell {
            id,
            agl_cost,
            offset,
            effect_offset: resolve_effect_offset(
                block,
                magic_count,
                legaia_bytes::u32_le(block, entry + 4),
            ),
            aux_offset: resolve_effect_offset(
                block,
                magic_count,
                legaia_bytes::u32_le(block, entry + 8),
            ),
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
    let off = legaia_bytes::u32_le(block, word * 4)?;
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
        // spell entry 0 @ 0x100: id 0x0D (castable), AGL cost 12.
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
        assert_eq!(rec.agility(), 60);
        assert_eq!(rec.intelligence(), 16);
        assert_eq!(rec.gold, 60);
        assert_eq!(rec.exp, 55);
        assert_eq!(rec.drop_item, 119);
        assert_eq!(rec.drop_chance_pct, 10);
        assert_eq!(
            rec.spells,
            vec![
                MonsterSpell {
                    id: 0x0D,
                    agl_cost: 12,
                    offset: 0x100,
                    effect_offset: None,
                    aux_offset: None,
                },
                MonsterSpell {
                    id: 0x03,
                    agl_cost: 0xFF,
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
    fn battle_stats_applies_the_load_boost() {
        // Gaza (Sim-Seru), monster id 166, raw disc record:
        //   stats [AGL 128, ATK 288, UDF 222, LDF 200, INT 220, SPD 146].
        // A live international-retail battle capture of this fight shows the
        // actor with ATK 360, UDF 444, LDF 400, INT 247 - the gate-set boost
        // profile (FUN_80054cb0): ATK ×5/4, UDF/LDF ×2, INT ×9/8; AGL/SPD ×1.
        let rec = MonsterRecord {
            id: 166,
            name: "Gaza".into(),
            hp: 15000,
            mp: 1200,
            stats: [128, 288, 222, 200, 220, 146],
            element: 6,
            size_class: 26,
            gold: 30000,
            exp: 42000,
            drop_item: 0,
            drop_chance_pct: 0,
            magic_count: 0,
            spells: vec![],
            magic_attacks: vec![],
        };
        assert_eq!(rec.battle_stats(), [128, 360, 444, 400, 247, 146]);
        // AGL, HP, MP and SPD are pass-through; the four combat stats are boosted.
        assert_eq!(rec.battle_stats()[0], rec.agility());
        assert_eq!(rec.battle_stats()[5], rec.speed());
        assert_eq!(rec.battle_stats()[1], rec.attack() + (rec.attack() >> 2));
        assert_eq!(rec.battle_stats()[2], rec.defense_high() * 2);
        assert_eq!(rec.battle_stats()[3], rec.defense_low() * 2);
        assert_eq!(
            rec.battle_stats()[4],
            rec.intelligence() + (rec.intelligence() >> 3)
        );
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
            resolve_effect_offset(&block, mc, legaia_bytes::u32_le(&block, 0x100 + 4)),
            Some(0x1C0)
        );
        assert_eq!(
            resolve_effect_offset(&block, mc, legaia_bytes::u32_le(&block, 0x100 + 8)),
            None,
            "index 0 -> none"
        );
        // An index whose table slot is zero resolves to None.
        block[0x100 + 4] = 2;
        assert_eq!(
            resolve_effect_offset(&block, mc, legaia_bytes::u32_le(&block, 0x100 + 4)),
            None,
            "zero table slot -> none"
        );
        // An out-of-range resolved offset is rejected.
        block[0x58..0x5C].copy_from_slice(&0x9999u32.to_le_bytes());
        assert_eq!(
            resolve_effect_offset(&block, mc, legaia_bytes::u32_le(&block, 0x100 + 4)),
            None,
            "offset past block end -> none"
        );
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
