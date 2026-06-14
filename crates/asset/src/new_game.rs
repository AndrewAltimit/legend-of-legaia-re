//! New-game starting-party template (`SCUS_942.54`).
//!
//! When the title screen's NEW GAME row is confirmed, the boot chain
//! (`FUN_80025B64` -> `FUN_801D6704`, see `docs/subsystems/boot.md`) launches
//! the field/town overlay with a fresh game state. Part of that fresh state is
//! the starting party: a small static table in the executable holds each
//! roster member's opening stats and display name, which the seed routine
//! expands into the live per-character records at `0x80084708 + n*0x414`.
//!
//! This module parses that static table straight out of a `SCUS_942.54` image,
//! so the engine can seed a faithful New Game from the user's own disc at
//! runtime - the same "decode in-game data from the disc, never commit Sony
//! bytes" pattern as [`crate::item_names`] / [`crate::spell_names`].
//!
//! ## Record layout (26-byte stride)
//!
//! The table sits at [`PARTY_TEMPLATE_VA`]; each record is eight little-endian
//! `u16` stats followed by a fixed 10-byte NUL-padded name:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0`  | u16 | `hp_max` |
//! | `+2`  | u16 | `mp_max` |
//! | `+4`  | u16 | `agl` (also seeds the spirit gauge + cap; see below) |
//! | `+6`  | u16 | `atk` |
//! | `+8`  | u16 | `udf` (upper / physical defence) |
//! | `+10` | u16 | `ldf` (lower / magical defence) |
//! | `+12` | u16 | `spd` |
//! | `+14` | u16 | `intel` |
//! | `+16` | u8[10] | display name, NUL-padded |
//!
//! The table holds [`PARTY_RECORDS`] entries in roster order
//! (`Vahn`, `Noa`, `Gala`, `Terra`). At a true New Game only Vahn has joined;
//! the rest are the templates the game uses when each character is introduced.
//!
//! The `+4` stat is a single value the seed routine fans out to several live
//! fields. Cross-validated against an early `town01` save state, Vahn's `+4`
//! (`100`) lands in the live record as `agl`, `cap_constant`, and the initial
//! spirit-gauge value all at once; the per-character archetypes
//! (`Noa = 120`, `Gala = 80`) read as agility, so this module names it `agl`.
//!
//! A New Game's first scene is the prologue cutscene `opdeene`
//! ([`OPENING_CUTSCENE_SCENE`]) - the in-engine 3D "It was the Seru."
//! Genesis-tree narration. The front-end launcher (`init_game` in the title
//! overlay) writes that scene id into the active-scene-name buffer
//! (`0x8007050C`); a `new_game_cutscene_intro_a` save state reads back
//! `"opdeene"` there, and the later Rim Elm states read `"town01"`. The
//! cutscene hands off to the interactive scene `town01` (Rim Elm) -
//! [`OPENING_SCENE`].

/// RAM address of the starting-party template (Vahn's record base).
pub const PARTY_TEMPLATE_VA: u32 = 0x8007_8C4C;

/// Per-record stride: eight `u16` stats (16 bytes) + a 10-byte name.
pub const RECORD_STRIDE: usize = 26;

/// Number of `u16` stat fields per record.
pub const STAT_COUNT: usize = 8;

/// Length of the fixed, NUL-padded name field per record.
pub const NAME_LEN: usize = 10;

/// Number of roster records the template carries (Vahn, Noa, Gala, Terra).
pub const PARTY_RECORDS: usize = 4;

/// RAM address of the new-game inventory-seed code block inside
/// `FUN_80034A6C`. The retail routine writes the single starting item here
/// (`DAT_80085958 = 0x77` / `DAT_80085959 = 5` = Healing Leaf ×5) with a
/// `li`/`sb` pair, immediately followed by an inline loop that zeroes the 512
/// bytes *below* the inventory. Both callers (`FUN_8001DCF8`'s new-game branch
/// and `FUN_8001FFA4`) memset `SC[0..0x1a18)` — which includes the whole
/// inventory — right before calling, so that inline zero-loop is redundant.
/// The 10 instructions from here on (`0x80034b04..0x80034b2b`, 40 bytes) are
/// therefore reclaimable as the starting-item seed region.
pub const STARTING_INV_SEED_VA: u32 = 0x8003_4B04;

/// Byte length of the reclaimable starting-item seed region (10 MIPS
/// instructions = 4 original seed + 6 redundant zero-loop).
pub const STARTING_INV_SEED_LEN: usize = 40;

/// Byte offset of the consumable inventory base relative to the save-context
/// (`SC`) base (`0x80084140`); the live inventory is `SC + 0x1818`
/// (`0x80085958`). The seed code's `sb`/`sh` stores use `$s0` (= `SC` base)
/// with these offsets, so the decoder reads slots from here.
pub const INVENTORY_SC_OFFSET: u32 = 0x1818;

/// Item id of Door of Wind — the consumable that opens the warp menu (a teleport
/// to any *previously visited* town). In the contiguous consumable block, so the
/// starting-item seed can write it directly to the inventory page.
pub const DOOR_OF_WIND_ITEM: u8 = 0x89;

/// Item id of Incense — the consumable that lowers the random-encounter rate for
/// a while. In the same contiguous consumable block as Door of Wind, so the
/// starting-item seed can write it directly to the inventory page.
pub const INCENSE_ITEM: u8 = 0x8A;

/// Byte offset of the low half of the Door-of-Wind "visited towns" bitmask
/// relative to the save-context (`SC`) base; the live word is `SC + 0x161C`
/// (`0x8008575C`). It lives in the story-flag block (`SC + 0x14C0..0x16C0` =
/// `0x80085600..0x80085800`, see `docs/reference/memory-map.md`), which the
/// New-Game seed memset covers, so the seed code can preset it the same way it
/// presets the inventory. Door of Wind reads this mask to decide which warp
/// destinations to offer; the known "Access All Towns" GameShark code forces it.
pub const WARP_FLAGS_SC_OFFSET: u32 = 0x161C;

/// The "all towns" visited-towns bitmask, split into the two halfwords the
/// GameShark code writes (`0x8008575C = 0xF77F`, `0x8008575E = 0xF8FF`): every
/// real Door-of-Wind warp destination marked reachable. Stored little-endian as
/// the four bytes `7F F7 FF F8` at `WARP_FLAGS_SC_OFFSET`.
pub const WARP_ALL_FLAGS_LO: u16 = 0xF77F;
/// High halfword of the [`WARP_ALL_FLAGS_LO`] bitmask (`0x8008575E`).
pub const WARP_ALL_FLAGS_HI: u16 = 0xF8FF;

/// RAM address of a **second** reclaimable region in `FUN_80034A6C`, used to
/// preset the Door-of-Wind warp bitmask **without** stealing from the
/// inventory-seed budget. At `0x80034adc..0x80034aeb` the routine clears four
/// `SC` words it has already been told are zero —
///
/// ```text
/// 80034adc  sw $zero, 0x460($s0)
/// 80034ae0  sw $zero, 0x464($s0)
/// 80034ae4  sw $zero, 0x470($s0)
/// 80034ae8  sw $zero, 0x478($s0)
/// ```
///
/// — all inside `SC[0..0x1a18)`, which both callers `memset` before the call,
/// so these four stores are redundant in exactly the way the zero-loop is. They
/// are reclaimable for the warp preset. **Crucially the preset must not touch
/// `$v0`**: it holds `0x2dc0` set just above (`0x80034ad8`) and consumed just
/// below (`0x80034af0` → `DAT_80073ef8`), so the warp stores use `$v1` (dead
/// after `0x80034acc`). The party-stat seeder `FUN_800560b4` called between this
/// region and gameplay never touches the warp window, so the preset survives —
/// **provided** the zero-loop at [`STARTING_INV_SEED_VA`] (which would otherwise
/// re-clear `SC+0x161C`) is overwritten, which it always is whenever the seed is
/// rewritten at all.
pub const WARP_SEED_VA: u32 = 0x8003_4ADC;

/// Byte length of the reclaimable warp-preset region ([`WARP_SEED_VA`]): four
/// MIPS instructions = two `addiu`/`sh` pairs.
pub const WARP_SEED_LEN: usize = 16;

/// CDNAME label of the prologue cutscene a New Game enters first (the
/// in-engine "It was the Seru." Genesis-tree narration). Written as the
/// opening scene id by the front-end launcher (`init_game`), verified live in
/// the `new_game_cutscene_intro_a` save state. Hands off to [`OPENING_SCENE`].
pub const OPENING_CUTSCENE_SCENE: &str = "opdeene";

/// CDNAME label of the interactive opening scene a New Game reaches after the
/// prologue cutscene ([`OPENING_CUTSCENE_SCENE`]) - Rim Elm.
pub const OPENING_SCENE: &str = "town01";

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`. (Same shape as the resolver
/// in [`crate::item_names`]; kept local so this module stands alone.)
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    /// File offset for a virtual address, or `None` if outside the data
    /// segment.
    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// File offset of the starting-inventory seed region ([`STARTING_INV_SEED_VA`])
/// within a `SCUS_942.54` image, or `None` if the image isn't a PSX-EXE or the
/// address is out of range. The disc patcher writes the seed patch here.
pub fn starting_inv_seed_file_offset(scus: &[u8]) -> Option<usize> {
    ExeMap::parse(scus)?.off(STARTING_INV_SEED_VA)
}

/// File offset of the warp-preset region ([`WARP_SEED_VA`]) within a
/// `SCUS_942.54` image, or `None` if the image isn't a PSX-EXE or the address is
/// out of range. The disc patcher writes the warp preset here (separate from the
/// inventory seed, so it never reduces the starting-item capacity).
pub fn warp_seed_file_offset(scus: &[u8]) -> Option<usize> {
    ExeMap::parse(scus)?.off(WARP_SEED_VA)
}

/// One roster member's opening stats + name, decoded from the template.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct StartingChar {
    /// Display name (e.g. `"Vahn"`).
    pub name: String,
    /// Maximum (and starting) HP.
    pub hp_max: u16,
    /// Maximum (and starting) MP.
    pub mp_max: u16,
    /// Agility; also seeds the spirit-gauge value and stat cap at New Game.
    pub agl: u16,
    /// Physical attack.
    pub atk: u16,
    /// Upper / physical defence.
    pub udf: u16,
    /// Lower / magical defence.
    pub ldf: u16,
    /// Speed (turn-order initiative seed).
    pub spd: u16,
    /// Intelligence.
    pub intel: u16,
}

/// The decoded starting-party template: one [`StartingChar`] per roster slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartingParty {
    members: Vec<StartingChar>,
}

impl StartingParty {
    /// Parse the starting-party template out of a `SCUS_942.54` image. Returns
    /// `None` if the image isn't a PSX-EXE or the table address is out of
    /// range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut members = Vec::with_capacity(PARTY_RECORDS);
        for rec in 0..PARTY_RECORDS {
            let base = map.off(PARTY_TEMPLATE_VA + (rec * RECORD_STRIDE) as u32)?;
            let stat = |i: usize| -> Option<u16> {
                let o = base + i * 2;
                Some(u16::from_le_bytes(scus.get(o..o + 2)?.try_into().ok()?))
            };
            let name_off = base + STAT_COUNT * 2;
            let name_bytes = scus.get(name_off..name_off + NAME_LEN)?;
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
            members.push(StartingChar {
                name,
                hp_max: stat(0)?,
                mp_max: stat(1)?,
                agl: stat(2)?,
                atk: stat(3)?,
                udf: stat(4)?,
                ldf: stat(5)?,
                spd: stat(6)?,
                intel: stat(7)?,
            });
        }
        Some(Self { members })
    }

    /// Build directly from a member list (tests / non-SCUS callers).
    pub fn from_members(members: Vec<StartingChar>) -> Self {
        Self { members }
    }

    /// All roster records in slot order.
    pub fn members(&self) -> &[StartingChar] {
        &self.members
    }

    /// The record at roster slot `idx` (`0` = Vahn), or `None` if out of range.
    pub fn member(&self, idx: usize) -> Option<&StartingChar> {
        self.members.get(idx)
    }

    /// Number of records decoded.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// `true` when no records were decoded.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Replay a seed region's `$s0`-relative byte/halfword stores into a sparse
/// `SC`-offset → byte map.
///
/// A seed region loads a constant into a scratch register then stores it
/// relative to `$s0` (= `SC` base): `addiu rt,$zero,imm` then `sb`/`sh
/// rt,off($s0)`. The inventory seed uses `$v0`; the warp preset uses `$v1` (it
/// must leave `$v0` alone, see [`WARP_SEED_VA`]). This recognises stores from
/// either register so both regions decode through one walker. See the
/// instruction encodings in `docs/formats/new-game-table.md`.
fn replay_seed_stores(region: &[u8]) -> std::collections::BTreeMap<u32, u8> {
    use std::collections::BTreeMap;

    let mut bytes: BTreeMap<u32, u8> = BTreeMap::new();
    // The scratch registers a seed store may use: $v0 (2) and $v1 (3).
    let mut regs: [u32; 32] = [0; 32];
    for chunk in region.chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        let op = word >> 26;
        let rs = (word >> 21) & 0x1F;
        let rt = ((word >> 16) & 0x1F) as usize;
        let imm = word & 0xFFFF;
        let is_scratch = rt == 2 || rt == 3;
        match op {
            // addiu rt, $zero, imm  (load the constant; low 16 bits are all the
            // stores below use).
            0x09 if rs == 0 && is_scratch => regs[rt] = imm,
            // sb rt, imm($s0)
            0x28 if rs == 16 && is_scratch => {
                bytes.insert(imm, (regs[rt] & 0xFF) as u8);
            }
            // sh rt, imm($s0)
            0x29 if rs == 16 && is_scratch => {
                bytes.insert(imm, (regs[rt] & 0xFF) as u8);
                bytes.insert(imm + 1, ((regs[rt] >> 8) & 0xFF) as u8);
            }
            _ => {}
        }
    }
    bytes
}

/// `true` if a 40-byte seed region presets the full [`WARP_ALL_FLAGS_LO`] /
/// [`WARP_ALL_FLAGS_HI`] "all towns" Door-of-Wind bitmask at
/// [`WARP_FLAGS_SC_OFFSET`] — i.e. the all-warps starting toggle is enabled.
pub fn region_unlocks_all_warps(region: &[u8]) -> bool {
    let bytes = replay_seed_stores(region);
    let halfword = |off: u32| -> Option<u16> {
        Some(u16::from_le_bytes([
            *bytes.get(&off)?,
            *bytes.get(&(off + 1))?,
        ]))
    };
    halfword(WARP_FLAGS_SC_OFFSET) == Some(WARP_ALL_FLAGS_LO)
        && halfword(WARP_FLAGS_SC_OFFSET + 2) == Some(WARP_ALL_FLAGS_HI)
}

/// `true` if a `SCUS_942.54` image's warp-preset region ([`WARP_SEED_VA`])
/// presets the all-towns Door-of-Wind bitmask. `None` if the image isn't a
/// PSX-EXE / the region is out of range.
pub fn scus_unlocks_all_warps(scus: &[u8]) -> Option<bool> {
    let map = ExeMap::parse(scus)?;
    let off = map.off(WARP_SEED_VA)?;
    let region = scus.get(off..off + WARP_SEED_LEN)?;
    Some(region_unlocks_all_warps(region))
}

/// The decoded new-game starting inventory: the `(item_id, count)` slots the
/// seed routine ([`STARTING_INV_SEED_VA`]) writes into the live consumable
/// inventory at New Game, in slot order. Vanilla retail is a single slot
/// `(0x77, 5)` (Healing Leaf ×5); the starting-item randomizer rewrites this
/// region to seed up to five slots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartingInventory {
    items: Vec<(u8, u8)>,
}

impl StartingInventory {
    /// Decode the starting inventory by interpreting the seed code region.
    ///
    /// The region writes inventory bytes with one of two idioms, both of which
    /// load a constant into `$v0` then store it relative to `$s0` (= `SC`
    /// base): the vanilla `sb` byte-store pair (`addiu $v0,id; sb …; addiu
    /// $v0,count; sb …`) or the randomizer's packed `sh` halfword-store
    /// (`addiu $v0,(count<<8)|id; sh …`). This walks the 40 bytes, replays
    /// every `sb $v0`/`sh $v0` store into a sparse `SC`-offset → byte map, then
    /// reads `(id, count)` slots from [`INVENTORY_SC_OFFSET`] until the
    /// id-`0` terminator — so it handles either encoding (and any future one)
    /// without special-casing instruction order. Returns `None` if the image
    /// isn't a PSX-EXE or the region is out of range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let off = map.off(STARTING_INV_SEED_VA)?;
        let region = scus.get(off..off + STARTING_INV_SEED_LEN)?;
        Some(Self::decode_region(region))
    }

    /// Decode a 40-byte seed region (exposed for callers that already hold the
    /// raw bytes, e.g. a patcher reading back its own edit).
    pub fn decode_region(region: &[u8]) -> Self {
        let bytes = replay_seed_stores(region);
        let mut items = Vec::new();
        let mut slot = 0u32;
        loop {
            let id = bytes
                .get(&(INVENTORY_SC_OFFSET + slot * 2))
                .copied()
                .unwrap_or(0);
            if id == 0 {
                break;
            }
            let count = bytes
                .get(&(INVENTORY_SC_OFFSET + slot * 2 + 1))
                .copied()
                .unwrap_or(0);
            items.push((id, count));
            slot += 1;
        }
        Self { items }
    }

    /// Build directly from `(id, count)` slots (tests / non-SCUS callers).
    pub fn from_items(items: Vec<(u8, u8)>) -> Self {
        Self { items }
    }

    /// The decoded `(item_id, count)` slots in slot order.
    pub fn items(&self) -> &[(u8, u8)] {
        &self.items
    }

    /// Number of seeded slots.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when the new game seeds no starting items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PSX-EXE image holding the given records at
    /// [`PARTY_TEMPLATE_VA`], so the parser is exercisable without Sony bytes.
    fn synth_scus(records: &[StartingChar]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (PARTY_TEMPLATE_VA - T_ADDR) as usize + 0x800;
        let total = table_off + records.len() * RECORD_STRIDE + 0x10;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        let t_size = (total - 0x800) as u32;
        buf[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (i, r) in records.iter().enumerate() {
            let base = table_off + i * RECORD_STRIDE;
            for (j, v) in [
                r.hp_max, r.mp_max, r.agl, r.atk, r.udf, r.ldf, r.spd, r.intel,
            ]
            .iter()
            .enumerate()
            {
                buf[base + j * 2..base + j * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            let name_off = base + STAT_COUNT * 2;
            let nb = r.name.as_bytes();
            let n = nb.len().min(NAME_LEN - 1);
            buf[name_off..name_off + n].copy_from_slice(&nb[..n]);
        }
        buf
    }

    fn vahn() -> StartingChar {
        StartingChar {
            name: "Vahn".into(),
            hp_max: 180,
            mp_max: 20,
            agl: 100,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        }
    }

    #[test]
    fn parses_records_and_names() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        let scus = synth_scus(&[vahn(), noa.clone(), Default::default(), Default::default()]);
        let party = StartingParty::from_scus(&scus).expect("parse");
        assert_eq!(party.len(), PARTY_RECORDS);
        assert_eq!(party.member(0), Some(&vahn()));
        assert_eq!(party.member(1), Some(&noa));
        // Out-of-range slot.
        assert_eq!(party.member(PARTY_RECORDS), None);
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(StartingParty::from_scus(b"not an exe").is_none());
        assert!(StartingParty::from_scus(&[0u8; 0x900]).is_none());
    }

    #[test]
    fn from_members_round_trips() {
        let p = StartingParty::from_members(vec![vahn()]);
        assert_eq!(p.member(0).unwrap().name, "Vahn");
        assert!(!p.is_empty());
    }

    /// Assemble a 40-byte seed region from MIPS instruction words (LE).
    fn region(words: &[u32]) -> Vec<u8> {
        let mut buf = vec![0u8; STARTING_INV_SEED_LEN];
        for (i, w) in words.iter().enumerate() {
            buf[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        buf
    }

    #[test]
    fn decodes_vanilla_sb_seed() {
        // The exact retail instruction stream at 0x80034b04: the `addiu`/`sb`
        // pair seeding Healing Leaf (0x77) ×5, then the redundant zero-loop
        // (which stores via `$zero`/`$v1`, so the decoder ignores it).
        let r = region(&[
            0x240401ff, // addiu $a0, $zero, 0x1ff
            0x02041821, // addu  $v1, $s0, $a0
            0x24020077, // addiu $v0, $zero, 0x77
            0xa2021818, // sb    $v0, 0x1818($s0)   id
            0x24020005, // addiu $v0, $zero, 5
            0xa2021819, // sb    $v0, 0x1819($s0)   count
            0xa0601618, // sb    $zero, 0x1618($v1) (zero-loop body, ignored)
            0x2484ffff, // addiu $a0, $a0, -1
            0x0481fffd, // bgez  $a0, ...
            0x2463ffff, // addiu $v1, $v1, -1
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(0x77, 5)]);
    }

    #[test]
    fn decodes_packed_sh_seed() {
        // The randomizer's packed halfword form: `addiu $v0,(count<<8)|id; sh`.
        let r = region(&[
            0x24020280, // addiu $v0, $zero, 0x0280  -> id 0x80, count 2
            0xa6021818, // sh    $v0, 0x1818($s0)
            0x2402017e, // addiu $v0, $zero, 0x017e  -> id 0x7e, count 1
            0xa602181a, // sh    $v0, 0x181a($s0)
            0x24020388, // addiu $v0, $zero, 0x0388  -> id 0x88, count 3
            0xa602181c, // sh    $v0, 0x181c($s0)
            0, 0, 0, 0, // nop padding
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(0x80, 2), (0x7e, 1), (0x88, 3)]);
    }

    #[test]
    fn decode_stops_at_id_zero_terminator() {
        // A `sh` that writes id 0 terminates the list even if later slots hold
        // data (matches the game's id-0 sentinel scan).
        let r = region(&[
            0x24020105, // id 5, count 1
            0xa6021818, // sh slot 0
            0x24020000, // id 0 (terminator)
            0xa602181a, // sh slot 1
            0x24020207, // id 7, count 2 (orphaned past the terminator)
            0xa602181c, // sh slot 2
            0, 0, 0, 0,
        ]);
        let inv = StartingInventory::decode_region(&r);
        assert_eq!(inv.items(), &[(5, 1)], "scan stops at the id-0 slot");
    }

    #[test]
    fn empty_region_decodes_to_no_items() {
        assert!(StartingInventory::decode_region(&[0u8; STARTING_INV_SEED_LEN]).is_empty());
    }

    #[test]
    fn warp_region_with_v1_stores_unlocks_all_warps() {
        // The real warp preset uses $v1 (rt 3) to avoid clobbering $v0:
        // addiu $v1, lo; sh $v1, 0x161C($s0); addiu $v1, hi; sh $v1, 0x161E($s0).
        let r = [
            0x2403_0000 | WARP_ALL_FLAGS_LO as u32,
            0xA603_0000 | WARP_FLAGS_SC_OFFSET,
            0x2403_0000 | WARP_ALL_FLAGS_HI as u32,
            0xA603_0000 | (WARP_FLAGS_SC_OFFSET + 2),
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<u8>>();
        assert_eq!(r.len(), WARP_SEED_LEN);
        assert!(region_unlocks_all_warps(&r));
    }

    #[test]
    fn warp_region_decode_is_independent_of_the_inventory_region() {
        // An inventory region full of $v0 item stores does NOT read as a warp
        // preset (the two are separate regions now).
        let inv = region(&[
            0x24020a89, // addiu $v0, Door of Wind x10
            0xa6021818, // sh slot 0
            0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert!(!region_unlocks_all_warps(&inv));
        assert_eq!(
            StartingInventory::decode_region(&inv).items(),
            &[(DOOR_OF_WIND_ITEM, 10)]
        );
    }

    #[test]
    fn warp_region_without_stores_is_not_unlocked() {
        // The original four `sw $zero` redundant stores at WARP_SEED_VA do not
        // read as a warp preset.
        let zeros = [
            0xAE00_0460u32, // sw $zero, 0x460($s0)
            0xAE00_0464,    // sw $zero, 0x464($s0)
            0xAE00_0470,    // sw $zero, 0x470($s0)
            0xAE00_0478,    // sw $zero, 0x478($s0)
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<u8>>();
        assert!(!region_unlocks_all_warps(&zeros));
        // A partial mask (only the low half) must not read as fully unlocked.
        let half = [
            0x2403_0000 | WARP_ALL_FLAGS_LO as u32,
            0xA603_0000 | WARP_FLAGS_SC_OFFSET,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<u8>>();
        assert!(!region_unlocks_all_warps(&half));
    }
}
