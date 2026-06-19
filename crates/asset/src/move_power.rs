//! Battle-overlay per-move **power / parameter** table (runtime VA `0x801F4F5C`).
//!
//! The battle-action damage kernel `FUN_801dd0ac` (dumped at
//! `ghidra/scripts/funcs/overlay_battle_action_801dd0ac.txt`) has two branches,
//! selected by its attacker-slot argument `param_2`:
//!
//! - **summon branch** (`param_2 == 7`): the magnitude is derived from
//!   caster/summon battle state, not a static table (see
//!   [`crate::summon_overlay`] and `docs/formats/spell-table.md`).
//! - **non-summon branch** (`param_2 != 7`, the **arts / physical** path): the
//!   attacker roll's modulus is read from a fixed-stride table based at
//!   `0x801F4F5C`, indexed by the move-type byte `param_1`:
//!
//! ```text
//! 801dd19c  lui   a1,0x801f
//! 801dd1a0  addiu a1,a1,0x4f5c       ; a1 = table base 0x801F4F5C
//! 801dd1a4  andi  a0,s5,0xff         ; a0 = param_1 (move-type byte)
//! 801dd1a8..b8                       ; v1 = a0*26  (sll/addu chain: a0<<1+a0
//!                                    ;   <<2 +a0 <<1 = 26*a0)
//! 801dd1bc  addu  v1,v1,a1           ; v1 = &table[a0]
//! 801dd1c0  lhu   a1,0x0(v1)         ; a1 = u16 at record +0
//! 801dd1c8  sll   a1,a1,0x10
//! 801dd1cc  sra   v1,a1,0x12         ; v1 = (i16)power >> 2   (the roll modulus)
//! ```
//!
//! So each record is **26 bytes** and its first field (`+0`, signed 16-bit) is
//! the move's **power**, which the kernel uses as `rand % (((i16)power >> 2) + 1)`.
//! (`801f3990`/`FUN_801dd0ac` also read `+0` as the half `>> 1` and full `>> 0`
//! values for the same move, so `+0` is the base power used at full / half /
//! quarter scale.)
//!
//! ## `param_1` is a *mapped* index, not the raw move id
//!
//! The kernel's `param_1` is not the battle move id directly - it is looked up
//! through a 128-byte **id → index map** immediately before the table at
//! [`MOVE_ID_INDEX_MAP_VA`] (`0x801F4E63`, raw-entry offset
//! [`MOVE_ID_INDEX_MAP_FILE_OFFSET`]). The setup site passes
//! `param_1 = map[actor[+0x1df]]` (`FUN_801dd0ac(*(byte*)(actor+0x1df) +
//! 0x801F4E63, …)` in `overlay_battle_action_801e09f8`). The map covers move ids
//! `0x00..=0x7F` and resolves the ids `0x04..=0x74` to power indices `0x01..=0x2b`
//! (a `0x00` entry = the unused record 0, `0xFF` = a no-record sentinel). So the
//! full resolution is `power_table[map[move_id]]` - see [`index_for_move_id`] /
//! [`record_for_move_id`]. The map is static (byte-identical across the same two
//! battle save states) and sits exactly `0x80` bytes before the 8-byte-record
//! table at `0x801F4EE3`.
//!
//! ## Provenance - static overlay data, pinned on disc
//!
//! The table is **static** (loaded with the battle-action overlay image, not
//! built per-battle): the `0x801F4F5C..0x801F69D8` window is byte-identical
//! between two unrelated battle save states (a full-party Gobu Gobu fight and
//! the Tetsu-tutorial command menu). Its bytes live in **PROT entry 0898** (the
//! battle-action overlay, `overlay_0898` / `overlay_battle_action`) at raw-entry
//! file offset [`MOVE_POWER_TABLE_FILE_OFFSET`] - pinned by byte-matching the
//! raw PROT 0898 entry against the in-RAM table at VA `0x801F4F5C` (both the
//! table window and the `FUN_801dd0ac` code body map with one consistent base).
//!
//! ## Extent
//!
//! The clean 26-byte-record structure holds for [`MOVE_POWER_TABLE_LEN`] entries
//! (indices `0..=43`; index 0 is an all-zero/unused slot); past it the region
//! transitions to other battle-overlay data (a float/transform table, then the
//! `data\battle\summon.DAT` / `readef.DAT` filename strings).
//!
//! ## How the action SM reads a record
//!
//! The per-move record is consumed by three battle-action functions:
//!
//! - `FUN_801dd0ac` / `801f3990` (damage kernels) read **only `+0x00`** (power)
//!   at full / half / quarter scale.
//! - `FUN_801dea50` (action setup) computes the record address **once**
//!   (`&DAT_801f4f5c + map[actor+0x1df]*0x1a`) and stashes it in the per-battle
//!   context at `ctx+0x1014` (`overlay_battle_action_801dea50.txt:528` →
//!   `sw v0,0x1014(a0)`), then seeds the move counter from `+0x04`.
//! - `FUN_801e09f8` (per-frame action tick) dereferences that held pointer
//!   (`lw …,0x1014(…)`) ~25× and reads the residual fields off it. The byte
//!   offsets it loads are exactly `+0x02,+0x06,+0x08,+0x09,+0x0a,+0x0b,+0x0d,
//!   +0x0e,+0x12,+0x16` - **never `+0x0c`** (confirmed: no `lbu …,0xc(…)` off
//!   the held pointer anywhere).
//!
//! ## Decoded record fields (each code-traced to a battle-action reader)
//!
//! | off | type | meaning | confidence | reader (`overlay_battle_action_*`) |
//! |---|---|---|---|---|
//! | `+0x00` | `i16` | **power** - roll modulus (used `>>0/1/2` at full/half/quarter) | Confirmed | `_801dd0ac.txt:299/343/388` |
//! | `+0x02` | `i16` | **strike-position Y offset** - subtracted from the per-arm Y lane (`ctx + arm*8 + 0x1146`) when the hit point is seeded from the target | Inferred | `_801e09f8.txt:1181/1363` |
//! | `+0x04` | `u16` | **whole-move timing counter** → `ctx+0x6c6`, decremented per frame | Confirmed | `_801dea50.txt:937` |
//! | `+0x06` | `u16` | **per-arm phase duration** → `ctx + arm*2 + 0x6c6` at the strike/re-arm transitions | Inferred | `_801e09f8.txt:1175/1357` |
//! | `+0x08` | `u8` | **homing / approach speed** - scales the per-frame XY step toward the target (`* DAT_1f800393 * 8`); `0x40 - x` reseeds the approach counter | Inferred | `_801e09f8.txt:1277/1282/1316` |
//! | `+0x09` | `u8` | **flag: effect tracks the strike** - when set, the live XY is copied into the spawned effect each frame | Confirmed reader | `_801e09f8.txt:1319` |
//! | `+0x0a` | `u8` | **impact-effect selector** (enum 1..5) - stored at `actor+0x21f`, indexes the 5-entry packed-config table at `0x801f53d4` (`(x-1)*4`) into `actor+0x04`, and switches (3/4/5) extra status-proc rolls | Confirmed reader | `_801e09f8.txt:1412/1416/1420/1422` |
//! | `+0x0b` | `u8` | **trail / afterimage texture-page id** - passed to the streak draw helpers; becomes the GP0 texpage word `0x7700 + id` | Confirmed | `_801e09f8.txt:1244` → `_801e1ab0.txt:250` |
//! | `+0x0c` | `u8` | **designer category tag** (`'C'/'E'/'G'/0`) - present only on the unnamed internal-tier records 1..15; **no runtime reader** (unused at runtime) | Unknown (no reader) | - |
//! | `+0x0d` | `u8` | **sound / voice cue id** → `FUN_8004fcc8` | Confirmed | `_801e09f8.txt:1452` |
//! | `+0x0e` | `u8` | **list-mode flag / effect-list head** - `0xFF` broadcasts the trail to all four arms; otherwise the head of a small id list the setup loop spawns | Confirmed reader | `_801e09f8.txt:1239`, `_801dea50.txt:983/1007` |
//! | `+0x12` | `[u8;4]` | **on-contact effect-id list** (`0`/`0xFF`-terminated) dispatched via `0x801f6324`/`0x801f6418` on the hit branch | Confirmed | `_801e09f8.txt:1162/1285/1312/1335` |
//! | `+0x16` | `[u8;4]` | **launch-strike effect-id list** - same dispatch as `+0x12`, fired at the initial-strike transition | Confirmed | `_801e09f8.txt:1182/1224/1364/1406/1506` |
//!
//! The effect-id lists `+0x12`/`+0x16` index two auxiliary tables that live in
//! the same overlay right after the power table - `0x801f6324` (effect
//! prototypes) and `0x801f6418` (per-effect SFX); ids `< 100` index those, `==
//! 100` spawns a fixed flash, the high bit (`& 0x80`) routes to `FUN_801dfdf0`,
//! `0xFF` is the skip sentinel and `0x00` terminates the list.
//!
//! Still genuinely open (no clear reader): nothing - the remaining table bytes
//! at the record tail past `+0x19` belong to the next record (stride 26). See
//! `docs/formats/move-power.md` for the full write-up.
//!
//! ## What the records are (cross-referenced against the spell table)
//!
//! The move id (`actor[+0x1df]`) the id → index map is keyed on is the **same id
//! space as the SCUS spell-name table** ([`crate::spell_names`], `DAT_800754C8`),
//! which the enemy name lookup also indexes by `actor[+0x1df]`. Joining the two
//! labels every record:
//!
//! - **records `0x10..=0x2b`** (move ids `0x25..=0x74`) are the **named monster
//!   special-attacks** - every one resolves to a non-empty spell-table name (Fire
//!   Breath `0x25`, Tail Fire `0x27` (the enemy-Gimard move), … through the
//!   late-game attacks at `0x61..=0x74`). This is their physical/special-attack
//!   *power*, separate from the *name* the spell table carries.
//! - **records `0x01..=0x0f`** (move ids `0x04..=0x1f`, all `< 0x24`) are the
//!   spell table's unnamed **internal enemy-attack tiers** (escalating-power
//!   triplets; these are the ids the spell-table docs call "internal enemy-attack
//!   tiers with empty name strings").

/// CDNAME / PROT index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// Runtime virtual address the table is loaded to (the `FUN_801dd0ac` base).
pub const MOVE_POWER_TABLE_VA: u32 = 0x801F_4F5C;

/// Raw-entry file offset of the table within PROT 0898. Empirically pinned by
/// byte-matching the entry against the in-RAM table at [`MOVE_POWER_TABLE_VA`].
pub const MOVE_POWER_TABLE_FILE_OFFSET: usize = 0x26744;

/// Load base of the battle-action overlay (PROT 0898), so a runtime VA in this
/// overlay maps to a raw-entry file offset as `va − BATTLE_OVERLAY_BASE`. Derived
/// from the table's own VA / file-offset pin (`0x801F4F5C − 0x26744`).
pub const BATTLE_OVERLAY_BASE: u32 = MOVE_POWER_TABLE_VA - MOVE_POWER_TABLE_FILE_OFFSET as u32;

/// Per-record stride (the `26*param_1` index math in `FUN_801dd0ac`).
pub const MOVE_POWER_RECORD_STRIDE: usize = 26;

/// Observed clean record count before the region transitions to other overlay
/// data. The intended count is a judgement (the structure degrades rather than
/// ending on an explicit sentinel); callers wanting only confirmed entries
/// should treat trailing empties as unused move ids.
pub const MOVE_POWER_TABLE_LEN: usize = 44;

/// Runtime VA of the id → power-index map (`0x80` bytes immediately before the
/// power table). `FUN_801dd0ac`'s `param_1` = `map[actor[+0x1df]]`.
pub const MOVE_ID_INDEX_MAP_VA: u32 = 0x801F_4E63;

/// Raw-entry file offset of the id → index map within PROT 0898
/// (= [`MOVE_POWER_TABLE_FILE_OFFSET`] − `0xF9`).
pub const MOVE_ID_INDEX_MAP_FILE_OFFSET: usize = MOVE_POWER_TABLE_FILE_OFFSET - 0xF9;

/// Length of the id → index map: move ids `0x00..=0x7F`.
pub const MOVE_ID_INDEX_MAP_LEN: usize = 0x80;

/// Map byte meaning "this move id has no power record" (the kernel never indexes
/// the table with it).
pub const MOVE_ID_INDEX_NONE: u8 = 0xFF;

/// Runtime VA of the **effect-prototype table** the records' `+0x12` / `+0x16`
/// effect-id lists index. Each `u32` entry is the spawn parameter `FUN_801e09f8`
/// passes to the effect spawner `FUN_80050ed4` (→ the part-stager `FUN_80021B04`)
/// - and it is a **pointer into this same overlay's data** at a move-VM part
/// record `[i16 model_sel][u16 flags][bytecode]` (the summon part-record format).
/// So the table is the move-FX **part-record index**: every entry resolves to a
/// scene-graph head the move VM then animates. Resolve entries with
/// [`EffectAuxTables::proto_record_offset`] / [`parse_effect_proto_records`].
pub const EFFECT_PROTO_TABLE_VA: u32 = 0x801F_6324;

/// Runtime VA of the **per-effect SFX table** indexed by the same effect-list
/// entry: a non-zero byte is the sound cue `FUN_801e09f8` plays when the effect
/// spawns.
pub const EFFECT_SFX_TABLE_VA: u32 = 0x801F_6418;

/// Raw-entry file offset of [`EFFECT_PROTO_TABLE_VA`] within PROT 0898 (derived
/// from the move-power table's pinned base, the same overlay link mapping).
pub const EFFECT_PROTO_TABLE_FILE_OFFSET: usize =
    MOVE_POWER_TABLE_FILE_OFFSET + (EFFECT_PROTO_TABLE_VA - MOVE_POWER_TABLE_VA) as usize;

/// Raw-entry file offset of [`EFFECT_SFX_TABLE_VA`] within PROT 0898.
pub const EFFECT_SFX_TABLE_FILE_OFFSET: usize =
    MOVE_POWER_TABLE_FILE_OFFSET + (EFFECT_SFX_TABLE_VA - MOVE_POWER_TABLE_VA) as usize;

/// Entry count shared by both effect tables. The `u32`-stride prototype table is
/// immediately followed by the byte-stride SFX table, so its extent is exactly
/// `(0x6418 - 0x6324) / 4 = 61`; the same index space bounds both (the runtime's
/// `< 100` spawn guard is a loose safety check - an `index >= 61` would alias the
/// SFX table into the prototype read).
pub const EFFECT_AUX_TABLE_LEN: usize = (EFFECT_SFX_TABLE_VA - EFFECT_PROTO_TABLE_VA) as usize / 4;

/// Effect-list entry value that spawns the fixed screen-flash instead of a table
/// effect (`FUN_801e09f8`'s `e == 100` arm: the `DAT_801c9070` flash struct +
/// `FUN_80024e80`).
pub const EFFECT_LIST_FIXED_FLASH: u8 = 100;

/// Runtime VA of the **impact-effect config table** the record's `+0x0a`
/// selector indexes. `FUN_801e09f8` reads `0x801f53d4[(impact_effect - 1)]`
/// (1-based; `(id-1)*4`) into the strike actor's `+0x04` field at the impact
/// transition. The entries are packed `u32` config words (`0x3FF`-masked lanes),
/// **not** pointers.
pub const IMPACT_EFFECT_TABLE_VA: u32 = 0x801F_53D4;

/// Raw-entry file offset of [`IMPACT_EFFECT_TABLE_VA`] within PROT 0898.
pub const IMPACT_EFFECT_TABLE_FILE_OFFSET: usize =
    MOVE_POWER_TABLE_FILE_OFFSET + (IMPACT_EFFECT_TABLE_VA - MOVE_POWER_TABLE_VA) as usize;

/// Entry count of the impact-effect table: the `+0x0a` selector is the enum
/// `1..=5`, so the table is 5 `u32` pointers (it ends exactly where the
/// element-affinity matrix at `0x801f53e8` begins).
pub const IMPACT_EFFECT_TABLE_LEN: usize = 5;

/// One 26-byte move record. Only the `+0` power field is interpreted; the raw
/// bytes are retained for forward reference as the remaining fields are decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRecord {
    /// Move id = the record's index into the table (`param_1`).
    pub index: usize,
    /// The `+0` signed-16-bit field (`lhu` then sign-extended by the kernel).
    pub power_raw: i16,
    /// The full 26-byte record.
    pub raw: [u8; MOVE_POWER_RECORD_STRIDE],
}

impl MoveRecord {
    /// The roll-modulus base `FUN_801dd0ac` derives from `+0`: `(i16)power >> 2`
    /// (arithmetic shift - preserves sign).
    pub fn power(&self) -> i32 {
        (self.power_raw as i32) >> 2
    }

    /// `+0x04` `u16` - the move's timing-window counter the action SM seeds at
    /// `ctx+0x6c6` and decrements (`FUN_801dea50` → `801e09f8`).
    pub fn counter_init(&self) -> u16 {
        u16::from_le_bytes([self.raw[4], self.raw[5]])
    }

    /// `+0x02` `i16` - strike-position **Y offset**: subtracted from the per-arm
    /// Y lane (`ctx + arm*8 + 0x1146`) when the move's hit point is seeded from
    /// the target's position (`801e09f8`). Inferred semantic; the read is
    /// confirmed.
    pub fn strike_y_offset(&self) -> i16 {
        i16::from_le_bytes([self.raw[2], self.raw[3]])
    }

    /// `+0x06` `u16` - **per-arm phase duration**: written into the per-arm
    /// countdown slot `ctx + arm*2 + 0x6c6` at the strike / re-arm transitions,
    /// distinct from the whole-move [`counter_init`](Self::counter_init) at
    /// `+0x04`. Inferred semantic; the read is confirmed.
    pub fn phase_duration(&self) -> u16 {
        u16::from_le_bytes([self.raw[6], self.raw[7]])
    }

    /// `+0x08` `u8` - **homing / approach speed**: scales the per-frame XY step
    /// toward the target (`* DAT_1f800393 * 8`) and reseeds the approach counter
    /// as `0x40 - speed` (`801e09f8`). Inferred semantic; the read is confirmed.
    pub fn homing_speed(&self) -> u8 {
        self.raw[8]
    }

    /// `+0x09` `u8` - **flag**: when non-zero the move's live XY position is
    /// copied into the spawned effect actor each frame (the effect tracks the
    /// strike). Confirmed reader (`801e09f8`).
    pub fn effect_tracks_strike(&self) -> bool {
        self.raw[9] != 0
    }

    /// `+0x0a` `u8` - **impact-effect selector** (enum, typically 1..5): stored
    /// at `actor+0x21f`, indexes the 5-entry packed-config table at `0x801f53d4`
    /// (`(value-1)*4`) into `actor+0x04` ([`parse_impact_effect_table`]), and
    /// values 3/4/5 branch to extra status-proc rolls. `0` = no impact effect.
    /// Confirmed reader (`801e09f8`).
    pub fn impact_effect(&self) -> u8 {
        self.raw[0x0a]
    }

    /// `+0x0b` `u8` - **trail / afterimage texture-page id**: the streak draw
    /// helper turns it into the GP0 texpage word `0x7700 + id` (`801e1ab0`).
    /// Confirmed.
    pub fn trail_texture_page(&self) -> u8 {
        self.raw[0x0b]
    }

    /// `+0x0c` `u8` - a **designer category tag** (`'C'`/`'E'`/`'G'`/`0`) baked
    /// into the unnamed internal-tier records only. **No runtime reader exists**
    /// for this byte in any battle-action function, so it is unused at runtime;
    /// exposed for completeness / data inspection. Returns `Some(c)` for a
    /// printable ASCII tag, else `None`.
    pub fn annotation_tag(&self) -> Option<char> {
        let b = self.raw[0x0c];
        (b.is_ascii_graphic()).then_some(b as char)
    }

    /// `+0x0d` `u8` - the move's sound / voice cue id, handed to the cue
    /// dispatcher `FUN_8004fcc8` by the action SM (`801e09f8`).
    pub fn sound_cue_id(&self) -> u8 {
        self.raw[0x0d]
    }

    /// `+0x0e` `u8` - **list-mode flag**: `0xFF` broadcasts the move's trail to
    /// all four party arms (a sweeping / multi-target move); otherwise it is the
    /// head of a small effect-id list the setup loop spawns. Confirmed reader
    /// (`801e09f8` / `801dea50`).
    pub fn list_mode(&self) -> u8 {
        self.raw[0x0e]
    }

    /// `+0x12` `[u8;4]` - the raw **on-contact effect-id list**, dispatched on
    /// the hit branch. Use [`contact_effects`](Self::contact_effects) for the
    /// terminator-trimmed view.
    pub fn contact_effects_raw(&self) -> [u8; 4] {
        [
            self.raw[0x12],
            self.raw[0x13],
            self.raw[0x14],
            self.raw[0x15],
        ]
    }

    /// `+0x16` `[u8;4]` - the raw **launch-strike effect-id list**, dispatched at
    /// the initial-strike transition. Use [`launch_effects`](Self::launch_effects)
    /// for the terminator-trimmed view.
    pub fn launch_effects_raw(&self) -> [u8; 4] {
        [
            self.raw[0x16],
            self.raw[0x17],
            self.raw[0x18],
            self.raw[0x19],
        ]
    }

    /// On-contact effect ids up to the first list terminator. `0x00` ends the
    /// list; `0xFF` is the skip sentinel (and ends collection here). See
    /// [`contact_effects_raw`](Self::contact_effects_raw) for the untrimmed bytes.
    pub fn contact_effects(&self) -> Vec<u8> {
        trim_effect_list(&self.contact_effects_raw())
    }

    /// Launch-strike effect ids up to the first list terminator (same trimming
    /// rule as [`contact_effects`](Self::contact_effects)).
    pub fn launch_effects(&self) -> Vec<u8> {
        trim_effect_list(&self.launch_effects_raw())
    }

    /// `true` when the whole record is zero (an unused move-id slot).
    pub fn is_empty(&self) -> bool {
        self.raw.iter().all(|&b| b == 0)
    }
}

/// Collect a 4-byte effect-id list up to its first terminator: `0x00` ends the
/// list and `0xFF` is the skip sentinel - both stop collection (matching the
/// `id == -1` / `id == 0` loop guards in `FUN_801e09f8`).
fn trim_effect_list(raw: &[u8; 4]) -> Vec<u8> {
    raw.iter()
        .take_while(|&&b| b != 0x00 && b != 0xFF)
        .copied()
        .collect()
}

/// Parse `count` records from `bytes` starting at `offset`. Returns `None` when
/// the slice doesn't fit or the structural guard fails (record 0 must be the
/// all-zero unused slot and at least the first few real records must be
/// populated - a cheap check that the pinned offset still lands on the table).
pub fn parse_at(bytes: &[u8], offset: usize, count: usize) -> Option<Vec<MoveRecord>> {
    let end = offset.checked_add(count.checked_mul(MOVE_POWER_RECORD_STRIDE)?)?;
    if end > bytes.len() {
        return None;
    }
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let base = offset + i * MOVE_POWER_RECORD_STRIDE;
        let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw.copy_from_slice(&bytes[base..base + MOVE_POWER_RECORD_STRIDE]);
        let power_raw = i16::from_le_bytes([raw[0], raw[1]]);
        records.push(MoveRecord {
            index: i,
            power_raw,
            raw,
        });
    }
    // Structural guard: id 0 is the unused all-zero slot; the table proper must
    // carry several populated records right after it.
    if !records[0].is_empty() {
        return None;
    }
    let populated = records
        .iter()
        .skip(1)
        .take(4)
        .filter(|r| !r.is_empty())
        .count();
    if populated == 0 {
        return None;
    }
    Some(records)
}

/// Parse the table out of the raw PROT 0898 (battle-action overlay) entry bytes
/// at the pinned offset + length.
pub fn parse(battle_overlay_0898: &[u8]) -> Option<Vec<MoveRecord>> {
    parse_at(
        battle_overlay_0898,
        MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    )
}

/// Read the 128-byte id → power-index map out of the raw PROT 0898 entry. Each
/// byte `map[move_id]` is the power-table index the kernel uses for that battle
/// move id (`actor[+0x1df]`); [`MOVE_ID_INDEX_NONE`] (`0xFF`) and `0` mean "no
/// power record". Returns `None` if the slice is too short or the structural
/// guard fails (`map[4] == 1`, the first mapped id).
pub fn parse_id_index_map(battle_overlay_0898: &[u8]) -> Option<[u8; MOVE_ID_INDEX_MAP_LEN]> {
    let end = MOVE_ID_INDEX_MAP_FILE_OFFSET + MOVE_ID_INDEX_MAP_LEN;
    if end > battle_overlay_0898.len() {
        return None;
    }
    let mut map = [0u8; MOVE_ID_INDEX_MAP_LEN];
    map.copy_from_slice(&battle_overlay_0898[MOVE_ID_INDEX_MAP_FILE_OFFSET..end]);
    // Guard: move id 4 is the first mapped move (-> power index 1).
    if map[4] != 1 {
        return None;
    }
    Some(map)
}

/// Resolve a battle move id (`actor[+0x1df]`) to its power-table index via the
/// id → index map. Returns `None` for ids out of the map range or mapped to a
/// "no record" sentinel (`0` or `0xFF`).
pub fn index_for_move_id(map: &[u8; MOVE_ID_INDEX_MAP_LEN], move_id: u8) -> Option<u8> {
    let idx = *map.get(move_id as usize)?;
    if idx == 0 || idx == MOVE_ID_INDEX_NONE {
        None
    } else {
        Some(idx)
    }
}

/// Resolve a battle move id straight to its [`MoveRecord`] (map lookup + table
/// index). `table` is the [`parse`] output, `map` the [`parse_id_index_map`]
/// output.
pub fn record_for_move_id<'a>(
    table: &'a [MoveRecord],
    map: &[u8; MOVE_ID_INDEX_MAP_LEN],
    move_id: u8,
) -> Option<&'a MoveRecord> {
    let idx = index_for_move_id(map, move_id)? as usize;
    table.get(idx)
}

/// One decoded entry of a record's `+0x12` (on-contact) / `+0x16` (launch)
/// effect-id list, classified exactly as the `FUN_801e09f8` dispatch loop reads
/// the byte (`overlay_battle_action_801e09f8.txt:1182..1225` / `1285..1312`).
///
/// Both lists dispatch their bytes identically - the only difference is *when*
/// they fire (on contact vs at the launch transition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectListEntry {
    /// `0x00` - terminates the list scan (no effect).
    Terminator,
    /// `0x01..=0x63` - spawn the effect prototype [`EffectAuxTables::effect_proto`]
    /// at this index and, when non-zero, play its SFX
    /// [`EffectAuxTables::effect_sfx`].
    Spawn(u8),
    /// `0x64` (`== 100`) - the fixed screen-flash effect (no table lookup).
    FixedFlash,
    /// High bit set (and not `0xFF`) - routed to `FUN_801dfdf0` with the low 7
    /// bits as the id.
    AltEffect(u8),
    /// `0xFF`, or an unused `0x65..=0x7F` byte - no effect, but the scan does not
    /// terminate here (only `0x00` terminates).
    Skip,
}

impl EffectListEntry {
    /// Classify one effect-list byte exactly as `FUN_801e09f8` does: `0x00`
    /// terminates; the `0x80` bit (except `0xFF`) routes to the alt path with
    /// `id & 0x7F`; `0x01..=0x63` spawns a table effect; `0x64` is the fixed
    /// flash; `0xFF` (and the unused `0x65..=0x7F`) produce no effect.
    pub fn classify(entry: u8) -> EffectListEntry {
        match entry {
            0x00 => EffectListEntry::Terminator,
            0xFF => EffectListEntry::Skip,
            e if e & 0x80 != 0 => EffectListEntry::AltEffect(e & 0x7F),
            EFFECT_LIST_FIXED_FLASH => EffectListEntry::FixedFlash,
            e if (e as usize) < EFFECT_LIST_FIXED_FLASH as usize => EffectListEntry::Spawn(e),
            _ => EffectListEntry::Skip,
        }
    }
}

/// The two auxiliary effect tables a move-power record's `+0x12` / `+0x16`
/// effect-id lists index. Each [`EffectListEntry::Spawn`] index `e` yields the
/// spawn parameter [`Self::effect_proto`]`(e)` (`0x801F6324`, `u32`) and the SFX
/// cue [`Self::effect_sfx`]`(e)` (`0x801F6418`, `u8`; `0` = silent). Both are
/// static PROT 0898 data, loaded with the battle-action overlay like the
/// move-power table itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectAuxTables {
    proto: [u32; EFFECT_AUX_TABLE_LEN],
    sfx: [u8; EFFECT_AUX_TABLE_LEN],
}

impl EffectAuxTables {
    /// Parse both tables out of the raw PROT 0898 (battle-action overlay) entry.
    /// Returns `None` if the slice is too short or the overlay fails the
    /// move-power table's structural guard (so a different build / wrong entry
    /// can't silently yield garbage tables).
    pub fn parse(battle_overlay_0898: &[u8]) -> Option<Self> {
        // Tie validity to the move-power map guard in the same overlay.
        parse_id_index_map(battle_overlay_0898)?;
        let proto_end = EFFECT_PROTO_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN * 4;
        let sfx_end = EFFECT_SFX_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN;
        if proto_end > battle_overlay_0898.len() || sfx_end > battle_overlay_0898.len() {
            return None;
        }
        let mut proto = [0u32; EFFECT_AUX_TABLE_LEN];
        for (i, slot) in proto.iter_mut().enumerate() {
            let b = EFFECT_PROTO_TABLE_FILE_OFFSET + i * 4;
            *slot = u32::from_le_bytes([
                battle_overlay_0898[b],
                battle_overlay_0898[b + 1],
                battle_overlay_0898[b + 2],
                battle_overlay_0898[b + 3],
            ]);
        }
        let mut sfx = [0u8; EFFECT_AUX_TABLE_LEN];
        sfx.copy_from_slice(&battle_overlay_0898[EFFECT_SFX_TABLE_FILE_OFFSET..sfx_end]);
        Some(Self { proto, sfx })
    }

    /// The effect-prototype params (`0x801F6324`), one per spawn index.
    pub fn proto(&self) -> &[u32] {
        &self.proto
    }

    /// The per-effect SFX ids (`0x801F6418`), one per spawn index (`0` = silent).
    pub fn sfx(&self) -> &[u8] {
        &self.sfx
    }

    /// The spawn parameter for a [`EffectListEntry::Spawn`] index, or `None` when
    /// the index is outside the table.
    pub fn effect_proto(&self, index: u8) -> Option<u32> {
        self.proto.get(index as usize).copied()
    }

    /// The SFX cue id for a [`EffectListEntry::Spawn`] index (`0` = silent), or
    /// `None` when the index is outside the table.
    pub fn effect_sfx(&self, index: u8) -> Option<u8> {
        self.sfx.get(index as usize).copied()
    }

    /// Resolve a proto entry's `u32` (a runtime VA into the battle overlay) to its
    /// move-FX part-record **file offset** within PROT 0898, or `None` when the
    /// index is outside the table or the pointer does not land in this overlay.
    /// The record there is the summon part-record format
    /// (`[i16 model_sel][u16 flags][move-VM bytecode]`).
    pub fn proto_record_offset(&self, index: u8) -> Option<usize> {
        let va = self.effect_proto(index)?;
        va.checked_sub(BATTLE_OVERLAY_BASE).map(|o| o as usize)
    }
}

/// Decode the move-FX part records the [`EFFECT_PROTO_TABLE_VA`] entries point at,
/// straight out of the raw PROT 0898 (battle-action overlay) entry.
///
/// Each of the table's [`EFFECT_AUX_TABLE_LEN`] entries is a pointer into this
/// overlay's own data at a `[i16 model_sel][u16 flags][move-VM bytecode]` record
/// - the same scene-graph part format the summon stagers use
/// ([`crate::summon_overlay`]). Entries can alias (several effect ids reuse one
/// record), so the returned parts are the **unique** records, sorted by offset,
/// each with its `bytecode` range bounded by the next record. Map a proto index
/// back to its record with [`EffectAuxTables::proto_record_offset`].
///
/// Returns `None` if the overlay fails the move-power structural guard (so a
/// wrong / different-build entry can't yield garbage). The `bytecode` ranges
/// belong to the move VM ([`crate::move_power`] feeds `legaia-engine-vm`).
pub fn parse_effect_proto_records(
    battle_overlay_0898: &[u8],
) -> Option<Vec<crate::summon_overlay::SummonPart>> {
    let aux = EffectAuxTables::parse(battle_overlay_0898)?;
    let offsets: Vec<usize> = (0..EFFECT_AUX_TABLE_LEN as u8)
        .filter_map(|i| aux.proto_record_offset(i))
        .collect();
    Some(crate::summon_overlay::parse_records_at(
        battle_overlay_0898,
        &offsets,
    ))
}

/// Parse the 5-entry **impact-effect config table** (`0x801f53d4`) out of the
/// raw PROT 0898 entry. Each `u32` is a packed config word (`0x3FF`-masked lanes,
/// **not** a pointer) that the strike actor's `+0x04` is set to; index it with a
/// record's `+0x0a` [`MoveRecord::impact_effect`] minus one (the selector is
/// 1-based, `0` = none). Returns `None` if the slice is too short or the overlay
/// fails the move-power structural guard.
///
/// Beyond the pointer, `FUN_801e09f8` rolls a per-impact status proc keyed on the
/// selector (`overlay_battle_action_801e09f8.txt:1422..1447`): selector `3` has a
/// `1/8` chance (`rand & 7 == 0`) to set the actor's status bit `0` (`+0x16e |
/// 1`), selector `4` the same odds for bit `1` (`| 2`), and selector `5` rolls
/// `rand % 3` to set one of bits `3..=5` on the *target* (gated on the target's
/// character-record immunity flags). Selectors `1`/`2` carry no extra roll.
pub fn parse_impact_effect_table(
    battle_overlay_0898: &[u8],
) -> Option<[u32; IMPACT_EFFECT_TABLE_LEN]> {
    parse_id_index_map(battle_overlay_0898)?;
    let end = IMPACT_EFFECT_TABLE_FILE_OFFSET + IMPACT_EFFECT_TABLE_LEN * 4;
    if end > battle_overlay_0898.len() {
        return None;
    }
    let mut table = [0u32; IMPACT_EFFECT_TABLE_LEN];
    for (i, slot) in table.iter_mut().enumerate() {
        let b = IMPACT_EFFECT_TABLE_FILE_OFFSET + i * 4;
        *slot = u32::from_le_bytes([
            battle_overlay_0898[b],
            battle_overlay_0898[b + 1],
            battle_overlay_0898[b + 2],
            battle_overlay_0898[b + 3],
        ]);
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_shift_matches_kernel() {
        // 0x02ee = 750 -> >>2 = 187 (the kernel's roll modulus base).
        let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw[4] = 0xc8; // +0x04 counter_init = 0x00c8
        raw[0x0d] = 0x4b; // +0x0d sound cue = 0x4b
        let r = MoveRecord {
            index: 1,
            power_raw: 0x02ee,
            raw,
        };
        assert_eq!(r.power(), 187);
        assert_eq!(r.counter_init(), 0x00c8);
        assert_eq!(r.sound_cue_id(), 0x4b);
        // Sign preserved (arithmetic shift).
        let n = MoveRecord {
            index: 0,
            power_raw: -32,
            raw: [0; MOVE_POWER_RECORD_STRIDE],
        };
        assert_eq!(n.power(), -8);
    }

    #[test]
    fn residual_field_accessors() {
        // Record shaped after disc record 3 (move id 0x29): pow 6000, +2=250,
        // +6=480, +8=0x20, +9=1, +a=1, +c='C', +d=0x4d, +12=27 8e 8d 00,
        // +16=28 64 9d 00.
        let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw[0..2].copy_from_slice(&6000i16.to_le_bytes());
        raw[2..4].copy_from_slice(&250i16.to_le_bytes());
        raw[6..8].copy_from_slice(&480u16.to_le_bytes());
        raw[8] = 0x20;
        raw[9] = 1;
        raw[0x0a] = 1;
        raw[0x0b] = 0;
        raw[0x0c] = b'C';
        raw[0x0d] = 0x4d;
        raw[0x12..0x16].copy_from_slice(&[0x27, 0x8e, 0x8d, 0x00]);
        raw[0x16..0x1a].copy_from_slice(&[0x28, 0x64, 0x9d, 0x00]);
        let r = MoveRecord {
            index: 3,
            power_raw: 6000,
            raw,
        };
        assert_eq!(r.power(), 1500);
        assert_eq!(r.strike_y_offset(), 250);
        assert_eq!(r.phase_duration(), 480);
        assert_eq!(r.homing_speed(), 0x20);
        assert!(r.effect_tracks_strike());
        assert_eq!(r.impact_effect(), 1);
        assert_eq!(r.trail_texture_page(), 0);
        assert_eq!(r.annotation_tag(), Some('C'));
        assert_eq!(r.sound_cue_id(), 0x4d);
        assert_eq!(r.contact_effects(), vec![0x27, 0x8e, 0x8d]);
        assert_eq!(r.launch_effects(), vec![0x28, 0x64, 0x9d]);

        // 0xFF skip-sentinel terminates the list (disc record 40 / 0x74).
        let mut raw2 = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw2[0x12..0x16].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        raw2[0x0e] = 0xff;
        let r2 = MoveRecord {
            index: 40,
            power_raw: 0,
            raw: raw2,
        };
        assert!(r2.contact_effects().is_empty());
        assert_eq!(r2.list_mode(), 0xff);
        // A record with no printable +0x0c tag yields None.
        assert_eq!(r2.annotation_tag(), None);
    }

    #[test]
    fn id_index_map_resolves_move_ids() {
        // Synthetic 0898-shaped buffer: map at its offset, full table after it.
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        // map[4] = 1 (the guard + first mapped move), map[5] = 2, map[0x10] = 0xff.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 5] = 2;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x10] = MOVE_ID_INDEX_NONE;
        // table record 1 power 0x02ee, record 2 power 0x09c4.
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2] = 0xc4;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2 + 1] = 0x09;

        let map = parse_id_index_map(&buf).expect("map parses");
        let table = parse(&buf).expect("table parses");
        assert_eq!(index_for_move_id(&map, 4), Some(1));
        assert_eq!(index_for_move_id(&map, 5), Some(2));
        assert_eq!(index_for_move_id(&map, 0), None); // map[0] == 0 -> no record
        assert_eq!(index_for_move_id(&map, 0x10), None); // 0xff sentinel
        assert_eq!(
            record_for_move_id(&table, &map, 4).map(|r| r.power()),
            Some(187)
        );
        assert_eq!(
            record_for_move_id(&table, &map, 5).map(|r| r.power()),
            Some(625)
        );
    }

    #[test]
    fn parse_at_reads_stride_and_guards() {
        // Synthetic: record 0 empty, record 1 has power_raw 0x02ee.
        let mut buf = vec![0u8; 16 + MOVE_POWER_RECORD_STRIDE * 3];
        let off = 16;
        // record 1 (index 1) +0 = 0x02ee
        buf[off + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[off + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        let recs = parse_at(&buf, off, 3).expect("parses");
        assert_eq!(recs.len(), 3);
        assert!(recs[0].is_empty());
        assert_eq!(recs[1].power_raw, 0x02ee);
        assert_eq!(recs[1].power(), 187);

        // Guard: a non-empty record-0 (offset lands off the table) -> None.
        assert!(parse_at(&buf, off + 1, 3).is_none());
        // Guard: slice too short -> None.
        assert!(parse_at(&buf, buf.len() - 4, 2).is_none());
    }

    #[test]
    fn effect_list_entry_classifies_each_dispatch_arm() {
        use EffectListEntry::*;
        assert_eq!(EffectListEntry::classify(0x00), Terminator);
        assert_eq!(EffectListEntry::classify(0x01), Spawn(0x01));
        assert_eq!(EffectListEntry::classify(0x28), Spawn(0x28));
        assert_eq!(EffectListEntry::classify(0x63), Spawn(0x63)); // 99, last spawn
        assert_eq!(EffectListEntry::classify(0x64), FixedFlash); // 100
        assert_eq!(EffectListEntry::classify(0x65), Skip); // 101: unused no-op
        assert_eq!(EffectListEntry::classify(0x7F), Skip); // 127: unused no-op
        assert_eq!(EffectListEntry::classify(0x80), AltEffect(0x00)); // high bit
        assert_eq!(EffectListEntry::classify(0x9d), AltEffect(0x1d));
        assert_eq!(EffectListEntry::classify(0xFE), AltEffect(0x7E));
        assert_eq!(EffectListEntry::classify(0xFF), Skip); // skip sentinel
    }

    #[test]
    fn effect_aux_table_offsets_and_extent() {
        // Pinned against the move-power table base (same overlay link mapping).
        assert_eq!(EFFECT_PROTO_TABLE_FILE_OFFSET, 0x27B0C);
        assert_eq!(EFFECT_SFX_TABLE_FILE_OFFSET, 0x27C00);
        // The prototype table is bounded by the SFX table that follows it.
        assert_eq!(EFFECT_AUX_TABLE_LEN, 61);
        assert_eq!(
            EFFECT_PROTO_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN * 4,
            EFFECT_SFX_TABLE_FILE_OFFSET
        );
    }

    #[test]
    fn impact_effect_table_offset_and_parse() {
        assert_eq!(IMPACT_EFFECT_TABLE_FILE_OFFSET, 0x26BBC);
        // A 0898-shaped buffer with the map guard + a known pointer at index 0.
        let mut buf = vec![0u8; IMPACT_EFFECT_TABLE_FILE_OFFSET + IMPACT_EFFECT_TABLE_LEN * 4];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // map guard
        buf[IMPACT_EFFECT_TABLE_FILE_OFFSET..IMPACT_EFFECT_TABLE_FILE_OFFSET + 4]
            .copy_from_slice(&0x801F_5A00u32.to_le_bytes());
        let table = parse_impact_effect_table(&buf).expect("impact table parses");
        assert_eq!(table.len(), 5);
        assert_eq!(table[0], 0x801F_5A00);
        // Guard: a bad map -> no table.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 0;
        assert!(parse_impact_effect_table(&buf).is_none());
    }

    #[test]
    fn effect_aux_tables_parse_synthetic() {
        // A 0898-shaped buffer: valid move-power map guard + known aux values.
        let mut buf = vec![0u8; EFFECT_SFX_TABLE_FILE_OFFSET + EFFECT_AUX_TABLE_LEN];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // map guard
        // proto[0x28] = 0xCAFEBABE, sfx[0x28] = 0x4d.
        let pb = EFFECT_PROTO_TABLE_FILE_OFFSET + 0x28 * 4;
        buf[pb..pb + 4].copy_from_slice(&0xCAFE_BABEu32.to_le_bytes());
        buf[EFFECT_SFX_TABLE_FILE_OFFSET + 0x28] = 0x4d;

        let aux = EffectAuxTables::parse(&buf).expect("aux tables parse");
        assert_eq!(aux.effect_proto(0x28), Some(0xCAFE_BABE));
        assert_eq!(aux.effect_sfx(0x28), Some(0x4d));
        assert_eq!(aux.effect_sfx(0x00), Some(0)); // silent
        assert_eq!(aux.effect_proto(EFFECT_AUX_TABLE_LEN as u8), None); // out of range
        assert_eq!(aux.proto().len(), EFFECT_AUX_TABLE_LEN);
        assert_eq!(aux.sfx().len(), EFFECT_AUX_TABLE_LEN);

        // Guard: an overlay that fails the move-power map guard yields no tables.
        let mut bad = buf.clone();
        bad[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 0;
        assert!(EffectAuxTables::parse(&bad).is_none());
    }
}
