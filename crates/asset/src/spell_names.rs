//! Spell-name + stat table parser (`DAT_800754C8` / `DAT_800754D0` in
//! `SCUS_942.54`).
//!
//! The battle-action state machine resolves a cast's on-screen name, MP cost
//! and target shape from a static 12-byte-stride table inside `SCUS_942.54`,
//! viewed through two interleaved base pointers (see
//! `docs/formats/spell-table.md`):
//!
//! - `DAT_800754C8` - stats base. `+0` class byte, `+2` target shape, `+3` MP
//!   cost.
//! - `DAT_800754D0` - the same record `+8`: the display-name pointer.
//!
//! This is the table the **enemy** name lookup uses too. An enemy's cast is
//! resolved to a *global* spell id (the monster record's magic-attack id at
//! [`crate::monster_archive`] record `+0x21..=+0x23`), written into the live
//! actor at `+0x1DF`, and named through `&DAT_800754D0 + id*0xC` - exactly the
//! party path. So this parser turns a monster's magic-attack id into the same
//! name the game prints (`0x27` -> `Tail Fire`).
//!
//! ids `0x00..=0x24` are internal enemy-attack tiers with empty name strings;
//! `0x25..` carry the named monster attacks (`Fire Breath`, `Tail Fire`, ...)
//! and the player Seru-magic block at `0x81..=0x8b`.

/// RAM address of the stats base (`DAT_800754C8`).
pub const STATS_VA: u32 = 0x8007_54C8;
/// Per-id stride in bytes.
pub const RECORD_STRIDE: usize = 0x0C;
/// Number of spell ids the table covers.
pub const SPELL_COUNT: usize = 256;

/// PSX-EXE `t_addr` -> file-offset resolver (see [`crate::item_names`]).
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

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

fn read_name(scus: &[u8], map: &ExeMap, va: u32) -> Option<String> {
    let start = map.off(va)?;
    let mut out = String::new();
    let mut i = start;
    while i < scus.len() {
        let b = scus[i];
        if b == 0 {
            break;
        }
        if b == 0xCE {
            // 0xCE + element-colour byte (+ an optional trailing space).
            i += 2;
            if scus.get(i) == Some(&0x20) {
                i += 1;
            }
            continue;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
        i += 1;
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Decoded shape of a spell's `+2` target byte.
///
/// The byte is two independent bits over a side/scope pair (pinned against the
/// player Seru-magic block `0x81..=0x8b`, whose shapes are byte-exact from the
/// gamedata cross-reference):
///
/// - bit `0x02` = **ally side** (clear = enemy side)
/// - bit `0x20` = **all targets** (clear = single target)
///
/// So `0x44` = single enemy, `0x64` = all enemies, `0x06` = single ally,
/// `0x26` = all allies. (Empty / internal-tier slots read `0x00`/`0x04`,
/// decoding to single-enemy.) See [`docs/formats/spell-table.md`].
///
/// The model holds across the whole named player block and the six offensive
/// Ra-Seru summons, with one documented exception: the revive Ra-Seru
/// **Horn / "Resurrector"** (`0x9c`) carries an *enemy-side* `+2` byte (`0x24`,
/// decoding to all-enemies) even though its effect revives all allies - the
/// summon's projection plays toward the enemy field and the revive is
/// special-cased by spell id. (Cross-checked in
/// `legaia-gamedata::magic_vs_disc`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellTargetShape {
    /// Single enemy (bit `0x02` clear, `0x20` clear).
    OneEnemy,
    /// All enemies (bit `0x02` clear, `0x20` set).
    AllEnemies,
    /// Single ally (bit `0x02` set, `0x20` clear).
    OneAlly,
    /// All allies (bit `0x02` set, `0x20` set).
    AllAllies,
}

/// Target-byte bit: the spell targets the **ally** side (clear = enemy side).
pub const TARGET_ALLY_BIT: u8 = 0x02;
/// Target-byte bit: the spell hits **all** targets on its side (clear = one).
pub const TARGET_ALL_BIT: u8 = 0x20;

/// One decoded spell-table entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellEntry {
    /// Display name, or `None` for an empty / internal-tier slot.
    pub name: Option<String>,
    /// MP cost (`stats +3`).
    pub mp: u8,
    /// Target-shape byte (`stats +2`).
    pub target: u8,
}

impl SpellEntry {
    /// Decode the `+2` byte into a [`SpellTargetShape`] (side + scope bits).
    pub fn target_shape(&self) -> SpellTargetShape {
        let ally = self.target & TARGET_ALLY_BIT != 0;
        let all = self.target & TARGET_ALL_BIT != 0;
        match (ally, all) {
            (false, false) => SpellTargetShape::OneEnemy,
            (false, true) => SpellTargetShape::AllEnemies,
            (true, false) => SpellTargetShape::OneAlly,
            (true, true) => SpellTargetShape::AllAllies,
        }
    }
}

/// The decoded spell table: one entry per spell id (`0x00..=0xFF`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellNameTable {
    entries: Vec<SpellEntry>,
}

impl SpellNameTable {
    /// Parse the spell table out of a `SCUS_942.54` image. `None` if the image
    /// isn't a PSX-EXE or the table is out of range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut entries = Vec::with_capacity(SPELL_COUNT);
        for id in 0..SPELL_COUNT {
            let stat = map.off(STATS_VA + (id * RECORD_STRIDE) as u32)?;
            let target = *scus.get(stat + 2)?;
            let mp = *scus.get(stat + 3)?;
            let name_ptr = u32::from_le_bytes(scus.get(stat + 8..stat + 12)?.try_into().ok()?);
            let name = read_name(scus, &map, name_ptr);
            entries.push(SpellEntry { name, mp, target });
        }
        Some(Self { entries })
    }

    /// Build directly from an entry list (tests / non-SCUS callers).
    pub fn from_entries(entries: Vec<SpellEntry>) -> Self {
        Self { entries }
    }

    /// Display name for spell `id`, or `None` for an empty / internal slot.
    pub fn name(&self, id: u8) -> Option<&str> {
        self.entries.get(id as usize)?.name.as_deref()
    }

    /// MP cost for spell `id`.
    pub fn mp(&self, id: u8) -> Option<u8> {
        self.entries.get(id as usize).map(|e| e.mp)
    }

    /// The full entry for spell `id`.
    pub fn entry(&self, id: u8) -> Option<&SpellEntry> {
        self.entries.get(id as usize)
    }

    /// Number of id slots the table covers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the table holds no slots.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal PSX-EXE whose spell table holds the given `(name, mp)` rows.
    fn synth_scus(rows: &[(&str, u8)]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        let table_off = (STATS_VA - T_ADDR) as usize + 0x800;
        let table_bytes = SPELL_COUNT * RECORD_STRIDE;
        let pool_va = STATS_VA + table_bytes as u32;
        let pool_off = (pool_va - T_ADDR) as usize + 0x800;

        let mut pool = Vec::new();
        let mut str_va = Vec::new();
        for (s, _) in rows {
            str_va.push(pool_va + pool.len() as u32);
            pool.extend_from_slice(s.as_bytes());
            pool.push(0);
        }

        let total = pool_off + pool.len() + 0x10;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());

        for (id, ((_, mp), va)) in rows.iter().zip(&str_va).enumerate() {
            let rec = table_off + id * RECORD_STRIDE;
            buf[rec + 3] = *mp; // stats +3 = MP
            buf[rec + 8..rec + 12].copy_from_slice(&va.to_le_bytes()); // +8 = name ptr
        }
        buf[pool_off..pool_off + pool.len()].copy_from_slice(&pool);
        buf
    }

    #[test]
    fn parses_names_and_mp() {
        let scus = synth_scus(&[("", 0), ("Tail Fire", 16), ("Fire Breath", 70)]);
        let t = SpellNameTable::from_scus(&scus).unwrap();
        assert_eq!(t.len(), SPELL_COUNT);
        assert_eq!(t.name(0), None);
        assert_eq!(t.name(1), Some("Tail Fire"));
        assert_eq!(t.mp(1), Some(16));
        assert_eq!(t.name(2), Some("Fire Breath"));
        assert_eq!(t.mp(2), Some(70));
        assert_eq!(t.name(3), None);
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(SpellNameTable::from_scus(b"nope").is_none());
    }

    #[test]
    fn target_shape_decodes_side_and_scope_bits() {
        let shape = |b: u8| {
            SpellEntry {
                name: None,
                mp: 0,
                target: b,
            }
            .target_shape()
        };
        // The four player Seru-block byte values.
        assert_eq!(shape(0x44), SpellTargetShape::OneEnemy);
        assert_eq!(shape(0x64), SpellTargetShape::AllEnemies);
        assert_eq!(shape(0x06), SpellTargetShape::OneAlly);
        assert_eq!(shape(0x26), SpellTargetShape::AllAllies);
        // Internal-tier / empty slots decode to single-enemy.
        assert_eq!(shape(0x00), SpellTargetShape::OneEnemy);
        assert_eq!(shape(0x04), SpellTargetShape::OneEnemy);
    }
}
