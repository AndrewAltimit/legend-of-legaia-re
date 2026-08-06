//! Battle **action effect script**: the 8-byte record stream a battle action
//! walks to place its visual effects, and the move-power install + homing seed
//! it does when the stream terminates.
//!
//! PORT: FUN_801DEA50
//!
//! One call per frame for the acting actor. The actor carries a cursor byte at
//! `+0x1F5`; each call consumes records from that cursor until it hits a
//! terminator or runs the cursor to `8`. Every record is an offset in the
//! actor's own local frame, which the stepper **rotates by the actor's facing**
//! (`actor[+0x46]`) before spawning anything, so an art authored facing "north"
//! plays correctly whichever way the actor is turned.
//!
//! Three things happen per record, and only the first is effect-specific:
//!
//! 1. the record's offset is rotated into world space and an effect is spawned
//!    at it (`FUN_80050ED4` for the table-driven forms, `FUN_801DFDF0` for the
//!    `0x80`-flagged direct form);
//! 2. on the terminator, the acting actor's **move-power record** is installed
//!    at the battle context's `+0x1014` and its `+0x04` word mirrored to
//!    `+0x6C6`;
//! 3. every target slot gets its homing state seeded - phase `+0x24E`, launch
//!    position `+0x1144`, bearing `+0x1166` and target index `+0x252`.
//!
//! The kernels here are the deterministic ones: the record layout, the facing
//! rotation, the move-power index chain, the effect-id classification, and the
//! target **band** the homing seed sweeps. The spawns themselves come back as a
//! list of requests rather than calls, so a host can route them at whatever
//! layer owns effects.
//!
//! `see ghidra/scripts/funcs/overlay_battle_action_801dea50.txt` and
//! [`docs/formats/move-power.md`](../../../docs/formats/move-power.md) for the
//! record the terminator installs.
//!
//! ## Retail wiring (all three former input gaps are closed)
//!
//! **The caller is not the battle-action SM.** A five-form reference scan
//! ([`docs/tooling/address-reference-scan.md`](../../../docs/tooling/address-reference-scan.md))
//! over `SCUS_942.54`, the based overlay images and the raw PROT entries finds
//! no reference of any form to `0x801DEA50` inside the battle-action overlay
//! image: the only two `jal` sites in the corpus are `0x800478B8` and
//! `0x80047C08`, both inside **`FUN_80047430`**, the battle per-frame
//! anim-node tick. Both are gated on `DAT_8007BD71 == 0xFF`, the effect-VM
//! ready flag, and both are preceded by the paired call to `FUN_801EC3E4` with
//! the same three arguments. So this runs once per frame off the render tick
//! for the acting actor, not once per action off the SM - and that tick is
//! itself reached by function pointer from the actor-list iterator
//! `FUN_8002519C`, which is live-pinned mid-battle.
//!
//! The three inputs, each pinned:
//!
//! 1. **The script block is the action's anim record itself.** The stepper's
//!    second argument is `node[+0x4C]` (`FUN_80047430` passes `s3 =
//!    *(node+0x4C)` at both `jal` sites), and `node[+0x4C]` is installed from
//!    `actor[+0x234 + i*4]` (`FUN_80049348`, `0x800494C8..D0`) - the committed
//!    per-action entry the anim commit `FUN_8004AD80` selects. On disc that
//!    entry is a monster-archive action entry / a player record[0] action
//!    entry, and its `+0x14..+0x53` region is exactly this record stream -
//!    carried by `legaia_asset::monster_archive::MonsterAnimation::effect_script`.
//! 2. **The cursor is per-actor state** (`+0x1F5`), zeroed when a new anim
//!    record commits (`FUN_8004AD80`, `0x8004B060`) - the engine mirror is
//!    `world::Actor::battle_effect_cursor`, reset by the anim commit.
//! 3. **The [`RotationLut`] pair is one SCUS-resident sine table.**
//!    `FUN_80026BE0` sets `_DAT_8007B81C = 0x80070A2C` and `_DAT_8007B7F8 =
//!    0x80070A2C + 0x800`. The table at `0x80070A2C` is 5120 `i16` entries of
//!    `trunc(sin(i * 2pi / 4096) * 4096)` (disc-verified byte-exact, zero
//!    mismatches, truncation toward zero; the final 1024 entries repeat the
//!    first 1024 so the `+0x400`-entry cos read never wraps). So `b()` = sin
//!    and `a()` = cos of the 12-bit angle - [`RetailRotationLut`] generates
//!    the same table from the formula.
//!
//! Engine wiring: `World::tick_battle_animations` steps each battle actor's
//! committed clip through [`step_effect_script`] and queues the resulting
//! [`EffectSpawn`]s as `world::BattleEffectSpawn`s (drained via
//! `World::drain_battle_effect_spawns`, consumed by the native window's
//! battle FX layer).
//!
//! The terminator's context writes now have a sink: [`MoveFxStreak`] models
//! the `ctx[+0x1014]` / `+0x6C6` / `+0x24E` / `+0x1144` block, the live tick
//! installs it (`World::step_actor_effect_script` →
//! `World::move_fx_streak`), and the render layer projects the streak
//! billboard from it (`legaia_engine_render::afterimage`, drawn by the native
//! window's screen-FX pass). What is still **not** modelled is the stepper's
//! function-head sibling, retail's `0x801DEA50..0x801DEBEC` prologue - whose
//! semantics are now pinned from the disassembly even though the port carries
//! no seat for them:
//!
//! * `ctx[+0x1028]` is a **single tracked part handle**, installed by the
//!   table arm for codes `0xA` / `0x2D` / `0x2E` (`0x801df1b8..0x801df1e8`),
//!   which also stash the record's *raw, unrotated* XYZ at
//!   `ctx[+0x1184/+0x1186/+0x1188]`.
//! * The prologue is **not** target-seeking integration. Each call it first
//!   drops the handle when the part's `+0x10` flags carry bit `3`
//!   (`0x801dea88..0x801deaac` - the part retired), then, only while the
//!   stepped actor is the context's active actor (`ctx[+0x13]`,
//!   `0x801deabc`), it **re-seats** the part at the actor: position
//!   `+0x14/+0x16/+0x18` = actor `+0x34..+0x3B`, `y -= ctx[+0x1186]`, and
//!   x/z offset by the stashed legs rotated through the sin/cos LUTs at the
//!   **current** facing `+ 0x800` (`0x801deae0..0x801debe4`). So the
//!   spawned effect *follows the acting actor* frame by frame (a dash drags
//!   its projectile along); the per-target `+0x1144` quads feed the separate
//!   `+0x0E`-list spawns, not this handle.
//! * The engine's table-form spawns are seated once at spawn position and
//!   never re-seated: `BattleEffectSpawn` carries no raw-offset channel and
//!   the spawned `SummonScene` is not identified back to its record, so the
//!   follow-the-actor motion has no carrier yet. The streak block reads
//!   `+0x1144` only as a projection input.
//!
//! A second unported prologue lane: `ctx[+0x263]` non-zero consumes the whole
//! call - it clears the flag and bumps the actor's `+0x1F5` **and** `+0x1F6`
//! cursors without spawning (`0x801dec08..0x801dec48`), an external
//! "skip one record" strobe.
//!
//! REF: FUN_80050ED4, FUN_801DFDF0 - the two effect-spawn entry points.
//! REF: FUN_80047430 - the sole retail caller, the battle anim-node tick.
//! REF: FUN_801EC3E4 - the sibling call the caller pairs this with.
//! REF: FUN_8002519C - the actor-list iterator that dispatches the tick.
//! REF: FUN_80049348 - shadows `actor[+0x234+i*4]` into `node[+0x4C]`.
//! REF: FUN_8004AD80 - the anim commit that installs the entry + zeroes `+0x1F5`.
//! REF: FUN_801E295C - the action SM that stages the move. It is *not* a
//! caller of this routine: a five-form reference sweep finds no `jal`, `j`,
//! literal word or `lui`+`addiu` pair for `FUN_801DEA50` anywhere inside the
//! battle-action overlay image.
//! REF: FUN_80019B28 - the bearing helper.

/// Bytes per effect-script record.
pub const RECORD_STRIDE: usize = 8;

/// Byte offset of record 0 inside the script block (`0x801deca8`: the cursor
/// scales by `8` then adds `0x14`).
pub const RECORD_BASE: usize = 0x14;

/// Cursor values `>= this` end the walk (`0x801dec6c`: `sltiu v0,v0,0x8`).
pub const MAX_CURSOR: u8 = 8;

/// Record `+0x01` low-7-bit value that marks the stream's end. The two spawn
/// branches test it differently - the direct branch compares the whole byte
/// against `0xFF` (`0x801dee88`) and the table branch compares against `0x7F`
/// (`0x801df0d4`) - and since the branch is selected by bit `0x80`, the two
/// tests together are exactly "`effect & 0x7F == 0x7F`".
pub const TERMINATOR_EFFECT_MASKED: u8 = 0x7F;

/// Slots the terminator's unconditional homing pre-seed sweeps
/// (`0x801df294..0x801df2e0`: `ctx[+0x24E + i] = 1` and the launch position
/// into `ctx[+0x1144 + i*8]`, for `i` in `0..4`).
pub const HOMING_SLOT_COUNT: usize = 4;

/// Record `+0x01` bit that selects the **direct** spawn form (`FUN_801DFDF0`)
/// over the table form.
pub const EFFECT_DIRECT_BIT: u8 = 0x80;

/// Exclusive upper bound of the table-form codes that consult the per-effect
/// SFX map (`0x801F6418`): the table arm's sound gate is `sltiu v0,v1,0x32`
/// (`0x801df0d8`), so only plain codes `0x00..=0x31` can fire the `0x1DC`
/// sound packet, and a code at or above this (e.g. the spreadsheet's `0x4C`
/// "hit effect" constant) is silent by construction. Contrast the cue-group
/// expander `FUN_801E22C8`, whose SFX arm has **no** such bound - the two
/// arms gate differently. Consumed by
/// `World::drain_battle_effect_spawns`, the engine seat of the arm.
///
/// PORT: FUN_801DEA50 (`0x801df0d4..0x801df134`, the SFX gate + packet build)
/// REF: FUN_80058490 (the sound-driver command submit the packet reaches)
pub const TABLE_SFX_GATE: u8 = 0x32;

/// PSX angle units in a full revolution.
const ANGLE_MASK: i32 = 0xFFF;

/// Half a revolution - the bias the stepper adds to the facing before indexing
/// the LUTs.
const ANGLE_HALF: i32 = 0x800;

/// Fixed-point shift of the rotation products.
const ROT_SHIFT: u32 = 12;

/// Move-power table stride (`0x801df264..0x801df27c` builds `id * 0x1A` out of
/// shifts and adds).
pub const MOVE_POWER_STRIDE: usize = 0x1A;

/// Target-slot band the homing seed sweeps, derived from the acting actor's
/// `+0x1DD` scope byte.
///
/// PORT: FUN_801DEA50 (`0x801df2e4..0x801df338`).
///
/// The byte is a scope selector, not a slot index:
///
/// - `0..=7`: a single slot - `first == last == scope`, so the sweep touches
///   exactly that actor.
/// - `9`: the **monster** row - slots `3..=6`.
/// - `8`: the **party** row - slots `0..=2`.
/// - anything else: the registers keep whatever the `0..=7` arm left, which for
///   a fresh call is the raw byte in both, i.e. the single-slot form.
///
/// Retail's loop is `for (i = 0; first + i <= last; i++)`, so an empty band is
/// impossible - the single-slot form always runs once.
///
/// The scope byte is `+0x1DD` =
/// `legaia_engine_vm::battle_action::BattleActor::active_target`, written and
/// read across the live action SM (the attack band, the spirit band, the
/// target cursor, `battle_round`); its group codes `8` / `9` are exactly the
/// ones `battle_action::magic`'s cast-begin retarget pass and
/// `battle_target_group` deal in. The band is computed on every terminator
/// [`step_effect_script`] reaches from the live tick, but its **consumer** is
/// still missing: the engine models no per-target `+0x1144` homing block for
/// the seed to write into (see the module note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetBand {
    /// First slot index the sweep visits.
    pub first: u8,
    /// Last slot index (inclusive).
    pub last: u8,
}

impl TargetBand {
    /// Classify the acting actor's `+0x1DD` scope byte.
    pub fn from_scope(scope: u8) -> Self {
        match scope {
            9 => Self { first: 3, last: 6 },
            8 => Self { first: 0, last: 2 },
            s => Self { first: s, last: s },
        }
    }

    /// The slots the sweep visits, in order.
    pub fn slots(self) -> impl Iterator<Item = u8> {
        (0u8..).map_while(move |i| {
            let s = self.first.checked_add(i)?;
            (s <= self.last).then_some(s)
        })
    }
}

/// One decoded effect-script record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectRecord {
    /// `+0x00` - the frame gate. The stepper skips the whole record while the
    /// action's frame counter plus one is below this.
    pub frame: u8,
    /// `+0x01` - the effect selector. [`TERMINATOR_EFFECT_MASKED`] in the low
    /// seven bits ends the stream; [`EFFECT_DIRECT_BIT`] picks the direct spawn
    /// form.
    pub effect: u8,
    /// `+0x02` (`i16`) - local-frame X offset.
    pub off_x: i16,
    /// `+0x04` (`i16`) - local-frame Y offset. Subtracted from the actor's Y,
    /// not added, and scaled by the actor's mesh scale.
    pub off_y: i16,
    /// `+0x06` (`i16`) - local-frame Z offset.
    pub off_z: i16,
}

impl EffectRecord {
    /// Decode the record at `cursor` from a script block. The block is the
    /// action's anim-record head
    /// (`legaia_asset::monster_archive::MonsterAnimation::effect_script`),
    /// whose records sit at [`RECORD_BASE`]` + cursor * `[`RECORD_STRIDE`].
    ///
    /// PORT: FUN_801DEA50 (`0x801dec84..0x801ded48`).
    pub fn at(block: &[u8], cursor: u8) -> Option<Self> {
        let o = RECORD_BASE + usize::from(cursor) * RECORD_STRIDE;
        let r = block.get(o..o + RECORD_STRIDE)?;
        Some(Self {
            frame: r[0],
            effect: r[1],
            off_x: i16::from_le_bytes([r[2], r[3]]),
            off_y: i16::from_le_bytes([r[4], r[5]]),
            off_z: i16::from_le_bytes([r[6], r[7]]),
        })
    }

    /// `true` when this record ends the stream (`0x7F` or `0xFF`).
    pub fn is_terminator(self) -> bool {
        self.effect & 0x7F == TERMINATOR_EFFECT_MASKED
    }

    /// `true` when the record spawns through the direct form.
    pub fn is_direct(self) -> bool {
        self.effect & EFFECT_DIRECT_BIT != 0
    }
}

/// The two `i16` LUTs the rotation indexes, in the pairing the stepper uses.
///
/// Retail dereferences `_DAT_8007B7F8` and `_DAT_8007B81C`. The stepper indexes
/// the first at `0xFFF - a` and the second at `a`, where `a` is the biased
/// facing - the complement is what makes the pair a rotation rather than two
/// independent scalings.
pub trait RotationLut {
    /// The `_DAT_8007B7F8` table, `1 << 12` fixed point.
    fn a(&self, angle: i32) -> i32;
    /// The `_DAT_8007B81C` table, `1 << 12` fixed point.
    fn b(&self, angle: i32) -> i32;
}

/// The retail LUT pair, reconstructed from its mathematical definition.
///
/// PORT: FUN_80026BE0 (installs the pair: `_DAT_8007B81C = 0x80070A2C`,
/// `_DAT_8007B7F8 = 0x80070A2C + 0x800`).
///
/// Both pointers land in **one** static `SCUS_942.54` sine table at
/// `0x80070A2C`: 5120 `i16` entries of `trunc(sin(i * 2pi / 4096) * 4096)`
/// (12-bit angle space, `1 << 12` fixed point, truncation toward zero -
/// disc-verified byte-exact over all 5120 entries). The `+0x800`-**byte**
/// offset between the two pointers is `+0x400` entries = a quarter
/// revolution, and the table's final 1024 entries repeat its first 1024 so
/// the offset read never needs a mask: `b(angle)` is sine, `a(angle)` is
/// cosine. This type generates the table from the same formula (the values
/// are a mathematical function of the index, not creative content), so any
/// engine host can hold a byte-exact pair without disc access.
pub struct RetailRotationLut {
    /// One revolution of `trunc(sin) * 4096`, indexed by 12-bit angle.
    sin: [i16; 4096],
}

impl RetailRotationLut {
    /// Build the table. `trunc` (toward zero), not `round` - the retail
    /// bytes match truncation exactly and differ from rounding in 2510 of
    /// the 5120 entries.
    pub fn new() -> Self {
        let mut sin = [0i16; 4096];
        for (i, slot) in sin.iter_mut().enumerate() {
            let a = (i as f64) * std::f64::consts::TAU / 4096.0;
            *slot = (a.sin() * 4096.0) as i16; // `as` truncates toward zero
        }
        Self { sin }
    }
}

impl Default for RetailRotationLut {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide [`RetailRotationLut`], built once on first use - the
/// engine analogue of retail's boot-time pointer install (`FUN_80026BE0`).
pub fn retail_rotation_lut() -> &'static RetailRotationLut {
    static LUT: std::sync::OnceLock<RetailRotationLut> = std::sync::OnceLock::new();
    LUT.get_or_init(RetailRotationLut::new)
}

impl RotationLut for RetailRotationLut {
    fn a(&self, angle: i32) -> i32 {
        // The `+0x800`-byte pointer offset = +0x400 entries (cosine). The
        // retail read relies on the table's repeated tail instead of a mask;
        // over the stepper's `0..=0xFFF` domain the two are identical.
        i32::from(self.sin[((angle + 0x400) & ANGLE_MASK) as usize])
    }
    fn b(&self, angle: i32) -> i32 {
        i32::from(self.sin[(angle & ANGLE_MASK) as usize])
    }
}

/// Rotate a local-frame `(x, z)` offset into world space by an actor facing.
///
/// PORT: FUN_801DEA50 (`0x801dedd4..0x801dee7c` and the sibling block at
/// `0x801defa0..0x801df050`).
///
/// Four LUT reads, not two - the pair and its **complement** `0xFFF - a`:
///
/// ```text
/// dx = (b[a]    * z) >> 12  +  (a[~a] * x) >> 12
/// dz = (b[~a]   * x) >> 12  +  (a[a]  * z) >> 12
/// ```
///
/// Each product narrows on its own with a plain arithmetic `>> 12`, so the two
/// halves round independently - summing first and shifting once is *not*
/// equivalent.
///
/// The two blocks that use this differ only in the angle: the direct-spawn
/// branch takes `facing & 0xFFF` ([`FacingBias::None`]) and the table branch
/// takes `(facing + 0x800) & 0xFFF` ([`FacingBias::Half`]).
///
/// The retail LUT pair behind `_DAT_8007B7F8` / `_DAT_8007B81C` is pinned as
/// one SCUS sine table (see [`RetailRotationLut`]), which is what the live
/// battle tick supplies.
pub fn rotate_offset<L: RotationLut>(
    lut: &L,
    facing: u16,
    bias: FacingBias,
    x: i32,
    z: i32,
) -> (i32, i32) {
    let a = (i32::from(facing) + bias.units()) & ANGLE_MASK;
    let comp = ANGLE_MASK - a;
    let dx = ((lut.b(a) * z) >> ROT_SHIFT) + ((lut.a(comp) * x) >> ROT_SHIFT);
    let dz = ((lut.b(comp) * x) >> ROT_SHIFT) + ((lut.a(a) * z) >> ROT_SHIFT);
    (dx, dz)
}

/// Which of the stepper's two facing conventions a rotation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacingBias {
    /// `facing & 0xFFF` - the direct-spawn branch (`0x801ded7c`).
    None,
    /// `(facing + 0x800) & 0xFFF` - the table-spawn branch (`0x801defb0`).
    Half,
}

impl FacingBias {
    /// The angle units added before masking.
    pub fn units(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Half => ANGLE_HALF,
        }
    }

    /// The bias a record's effect byte selects.
    pub fn for_effect(effect: u8) -> Self {
        if effect & EFFECT_DIRECT_BIT != 0 {
            Self::None
        } else {
            Self::Half
        }
    }
}

/// Scale a record's local offset by the actor's mesh scale.
///
/// PORT: FUN_801DEA50 (`0x801ded0c..0x801ded38` and siblings).
///
/// `scale` is the `u16` at the actor's mesh header `+0x72`. The product is
/// biased by `0xFFF` when negative before the `>> 12`, i.e. it truncates
/// toward zero rather than flooring.
///
/// Input caveat (narrower than a missing caller): engine battle actors carry
/// a typed pose and no mesh-header pointer, so the live tick substitutes the
/// q12 unit (`0x1000`) for the retail `actor[+0x22C][+0x72]` word - offsets
/// are unscaled until the per-actor render-node scale is modeled.
pub fn scale_offset(off: i16, scale: u16) -> i32 {
    let p = i32::from(off) * i32::from(scale);
    let p = if p < 0 { p + 0xFFF } else { p };
    p >> ROT_SHIFT
}

/// Resolve the move-power table index for an acting actor.
///
/// PORT: FUN_801DEA50 (`0x801df248..0x801df284`).
///
/// The actor's queued action byte `+0x1DF` indexes the **map** at
/// `0x801F4E64` - and retail reads `map[action - 1]`, not `map[action]`
/// (`0x801df25c`: `lbu v1,-0x1(v1)` after the add). The resulting id scales by
/// [`MOVE_POWER_STRIDE`] into the record table at `0x801F4F5C`.
///
/// Returns `None` for action `0` (no queued action - the read would run off the
/// front of the map).
///
/// The map is `legaia_asset::move_power::parse_id_index_map`'s table (runtime
/// VA `0x801F4E64`), which the live tick passes when a
/// `crate::move_power::MovePowerCatalog` is installed. Note the off-by-one:
/// `map[action - 1]`, not `map[action]` (`0x801df25c`). The resulting record
/// offset is surfaced on [`EffectScriptStep`] but has no consumer yet - the
/// engine has no `ctx[+0x1014]` staged-record slot (see the module note).
pub fn move_power_record_offset(map: &[u8], action: u8) -> Option<usize> {
    let idx = usize::from(action).checked_sub(1)?;
    let id = usize::from(*map.get(idx)?);
    Some(id * MOVE_POWER_STRIDE)
}

/// One effect the stepper wants spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSpawn {
    /// Cursor the record sat at.
    pub cursor: u8,
    /// The record's effect selector, with [`EFFECT_DIRECT_BIT`] still set when
    /// the direct form applies.
    pub effect: u8,
    /// `true` when this goes through `FUN_801DFDF0` rather than
    /// `FUN_80050ED4`.
    pub direct: bool,
    /// World-space position the effect is spawned at.
    pub at: (i32, i32, i32),
}

/// What one call of the stepper decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectScriptStep {
    /// Cursor value the actor's `+0x1F5` should be left at.
    pub cursor: u8,
    /// Effects to spawn, in record order.
    pub spawns: Vec<EffectSpawn>,
    /// `Some(offset)` when the stream terminated this call - the byte offset of
    /// the move-power record to install at the battle context's `+0x1014`.
    pub move_power_offset: Option<usize>,
    /// `Some(band)` when the stream terminated - the target slots whose homing
    /// state the terminator seeds.
    pub homing_band: Option<TargetBand>,
    /// `Some(point)` when the stream terminated - the world-space **launch
    /// position** the terminator writes into every `ctx[+0x1144 + i*8]` slot.
    ///
    /// It is the terminator record's own rotated offset, not the bare actor
    /// position: retail re-seeds the stack pair from `actor[+0x34..+0x3B]` at
    /// the top of *every* record iteration (`0x801DECE4`) and then runs the
    /// scale + facing rotation on it (`0x801DED30` for Y, `0x801DEE40` /
    /// `0x801DEE7C` or `0x801DF014` / `0x801DF050` for X / Z) **before** the
    /// terminator test, so the quad the seed loop copies out at `0x801DF2B4`
    /// carries the terminator record's placement. That point is what the
    /// afterimage streak later projects
    /// ([`legaia_engine_render::afterimage`]).
    pub launch: Option<(i32, i32, i32)>,
}

/// The battle-context block the terminator installs, and the consumer the
/// move-FX streak reads it back through.
///
/// PORT: FUN_801DEA50 (`0x801df248..0x801df2e0` - the install + seed loop)
///
/// This is the sink for the two context writes the stepper used to compute
/// and drop. Retail's block spans four context words:
///
/// | context | written by | read by |
/// |---|---|---|
/// | `+0x1014` | `sw` of the move-power record pointer (`0x801DF284`) | the action SM's per-move behaviour reads |
/// | `+0x6C6` | `sh` of that record's `+0x04` (`0x801DF290`) | the afterimage streak's billboard **half-width**, as `word - 0x200` (`0x801E1B44`) |
/// | `+0x24E + i` | `sb 1` per slot `i in 0..4` (`0x801DF2A4`) | the homing phase gate |
/// | `+0x1144 + i*8` | the launch position, same loop (`0x801DF2B4`) | the streak's billboard **centre** (`0x801E1AF8`) |
///
/// The engine keeps the record as an id rather than a pointer (its damage
/// path resolves move power by id through [`crate::move_power`]), and models
/// the four homing slots as one shared launch point - retail's pre-seed
/// loop writes the *same* quad into all four, and only the band loop that
/// follows differentiates them, which is per-target homing state the streak
/// never reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveFxStreak {
    /// `ctx[+0x1014]`, as a table index rather than a pointer: the move id
    /// whose record the terminator staged. `None` before any terminator.
    pub move_id: Option<u8>,
    /// `ctx[+0x6C6]` - the staged record's `+0x04` word.
    pub counter_word: u16,
    /// `ctx[+0x1144]` - the launch position every homing slot is seeded with.
    pub launch: Option<(i32, i32, i32)>,
    /// `ctx[+0x24E + i]` - the homing phase byte, `1` once seeded. Retail
    /// writes four identical bytes; the engine keeps the one value.
    pub phase: u8,
    /// The band the seed loop swept, for hosts that want to know which
    /// targets the move homes on.
    pub band: Option<TargetBand>,
}

impl MoveFxStreak {
    /// Apply one terminator's writes. No-op for a step that did not
    /// terminate, so a host can call it unconditionally per frame.
    ///
    /// `counter_word` is the staged move-power record's `+0x04`
    /// ([`legaia_asset::move_power::MoveRecord::counter_init`]); pass `None`
    /// when no catalog is installed and the block keeps its previous word,
    /// exactly as retail's `sh` would be skipped.
    pub fn install(&mut self, step: &EffectScriptStep, counter_word: Option<u16>) -> bool {
        let Some(band) = step.homing_band else {
            return false;
        };
        self.move_id = step
            .move_power_offset
            .map(|off| (off / MOVE_POWER_STRIDE) as u8);
        if let Some(w) = counter_word {
            self.counter_word = w;
        }
        self.launch = step.launch;
        self.phase = 1;
        self.band = Some(band);
        true
    }

    /// The streak billboard's half-width: `ctx[+0x6C6] - 0x200`, the value
    /// `FUN_801E1AB0` hands the GTE projector at `0x801E1B44`. Mirrors
    /// `legaia_engine_render::afterimage::streak_half_width`, which is the
    /// canonical port - repeated here so `engine-core` (which cannot depend
    /// on the wgpu-linked render crate) can answer the same question.
    pub fn half_width(&self) -> i16 {
        (self.counter_word as i16).wrapping_sub(0x200)
    }

    /// `true` once a terminator has staged a launch point - i.e. the frame's
    /// streak has something to project.
    pub fn is_armed(&self) -> bool {
        self.phase != 0 && self.launch.is_some()
    }

    /// The per-frame counter walk: retail's phase-1 arm decrements
    /// `ctx[+0x6C6]` by `DAT_1F800393 << 2` per game frame and floors it at
    /// zero (`FUN_801E09F8`, `0x801E0C1C..0x801E0C40`) - per 1-vsync engine
    /// tick, 4. The word is what schedules the trail's two emitters: the
    /// single-quad afterimage draws while it is `>= 0x281`, the chained
    /// ribbon once it falls below `0x201` (`0x801E0C84..0x801E0CD4`), and
    /// its `- 0x200` is the afterimage's shrinking half-width. Call once
    /// per battle frame; no-op while unarmed.
    // REF: FUN_801E09F8 (the move-FX phase driver's counter walk)
    pub fn tick_counter(&mut self) {
        if self.phase != 0 {
            self.counter_word = self.counter_word.saturating_sub(4);
        }
    }

    /// Drop the block (the move finished / the actor's clip changed).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Actor state the stepper reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectScriptActor {
    /// `+0x1F5` - the record cursor.
    pub cursor: u8,
    /// `+0x46` - facing, in PSX angle units.
    pub facing: u16,
    /// `+0x34`/`+0x38`/`+0x3C` - the actor's world position the offsets are
    /// applied to.
    pub world: (i32, i32, i32),
    /// The actor's mesh scale (`actor[+0x22C][+0x72]`).
    pub scale: u16,
    /// `+0x1DD` - the target-scope byte.
    pub scope: u8,
    /// `+0x1DF` - the queued action byte.
    pub action: u8,
    /// `+0x1DC` bit `0x8` - "this actor's effects are suppressed", which makes
    /// the whole call a no-op.
    pub suppressed: bool,
}

/// Walk the effect script for one frame.
///
/// PORT: FUN_801DEA50 (`0x801dec4c..0x801df53c`).
///
/// `frame` is the action's frame counter (the stepper's third argument); a
/// record whose `+0x00` gate is above `frame + 1` stops the walk for this call
/// without advancing the cursor, and a gate of `0` also stops it. Otherwise the
/// record spawns, the cursor advances, and the walk continues while the cursor
/// is below [`MAX_CURSOR`].
///
/// **A terminator does not break the loop.** Retail's terminator arm only sets
/// the local flag that enables the move-power install; the cursor still
/// advances and the walk still continues (`0x801df50c..0x801df538` is the sole
/// loop back-edge). What actually stops it one record later is the `+0x00 == 0`
/// gate on the zero-filled tail. The port keeps that structure, so a script
/// with two terminators installs twice - which is what the bytes do.
///
/// Wired: `World::tick_battle_animations` drives this once per battle frame
/// per actor with a committed clip, mirroring the retail cadence (the two
/// `jal` sites `0x800478B8` / `0x80047C08` inside `FUN_80047430`, the
/// per-frame anim-node tick dispatched from `FUN_8002519C`). The block is the
/// committed clip's disc entry head, the cursor persists on
/// `world::Actor::battle_effect_cursor`, and the spawns queue as
/// `world::BattleEffectSpawn`s for the host FX layer. Still open: the retail
/// call sites' `_DAT_8007BD71 == 0xFF` effect-VM-ready gate has no checked
/// engine model (the engine steps whenever a clip is committed), and the
/// terminator's two context installs surface without a consumer (module
/// note).
pub fn step_effect_script<L: RotationLut>(
    lut: &L,
    block: &[u8],
    actor: EffectScriptActor,
    frame: u8,
    move_power_map: &[u8],
) -> EffectScriptStep {
    let mut out = EffectScriptStep {
        cursor: actor.cursor,
        spawns: Vec::new(),
        move_power_offset: None,
        homing_band: None,
        launch: None,
    };
    if actor.suppressed || actor.cursor >= MAX_CURSOR {
        return out;
    }
    while out.cursor < MAX_CURSOR {
        let Some(rec) = EffectRecord::at(block, out.cursor) else {
            break;
        };
        // Frame gate: `frame + 1 < rec.frame` defers, `rec.frame == 0` ends.
        if i32::from(frame) + 1 < i32::from(rec.frame) || rec.frame == 0 {
            break;
        }
        // The offset math runs for EVERY record, terminator included: retail
        // re-seeds the stack pair from the actor position and rotates it
        // before the terminator test, so the terminator's own placement is
        // what the homing seed copies out as the launch position.
        let sx = scale_offset(rec.off_x, actor.scale);
        let sy = scale_offset(rec.off_y, actor.scale);
        let sz = scale_offset(rec.off_z, actor.scale);
        let (dx, dz) = rotate_offset(
            lut,
            actor.facing,
            FacingBias::for_effect(rec.effect),
            sx,
            sz,
        );
        // Y is subtracted (`0x801ded38`: `subu v0,v0,v1`).
        let at = (actor.world.0 + dx, actor.world.1 - sy, actor.world.2 + dz);
        if rec.is_terminator() {
            out.move_power_offset = move_power_record_offset(move_power_map, actor.action);
            out.homing_band = Some(TargetBand::from_scope(actor.scope));
            out.launch = Some(at);
        } else {
            out.spawns.push(EffectSpawn {
                cursor: out.cursor,
                effect: rec.effect,
                direct: rec.is_direct(),
                at,
            });
        }
        out.cursor += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rotation LUT in the retail fixed point. Generated, not lifted.
    struct Lut {
        t: Vec<i16>,
    }

    impl Lut {
        fn new() -> Self {
            let one = f64::from(1 << ROT_SHIFT);
            Self {
                t: (0..4096)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 4096.0;
                        (a.sin() * one).round() as i16
                    })
                    .collect(),
            }
        }
    }

    impl RotationLut for Lut {
        fn a(&self, angle: i32) -> i32 {
            i32::from(self.t[(angle & ANGLE_MASK) as usize])
        }
        fn b(&self, angle: i32) -> i32 {
            i32::from(self.t[((angle + 1024) & ANGLE_MASK) as usize])
        }
    }

    fn actor() -> EffectScriptActor {
        EffectScriptActor {
            cursor: 0,
            facing: 0,
            world: (1000, 200, 3000),
            scale: 1 << ROT_SHIFT,
            scope: 9,
            action: 5,
            suppressed: false,
        }
    }

    /// `[frame, effect, x, y, z]` per record, laid out at `RECORD_BASE`.
    fn block(records: &[(u8, u8, i16, i16, i16)]) -> Vec<u8> {
        let mut b = vec![0u8; RECORD_BASE + records.len() * RECORD_STRIDE];
        for (i, &(f, e, x, y, z)) in records.iter().enumerate() {
            let o = RECORD_BASE + i * RECORD_STRIDE;
            b[o] = f;
            b[o + 1] = e;
            b[o + 2..o + 4].copy_from_slice(&x.to_le_bytes());
            b[o + 4..o + 6].copy_from_slice(&y.to_le_bytes());
            b[o + 6..o + 8].copy_from_slice(&z.to_le_bytes());
        }
        b
    }

    #[test]
    fn retail_lut_is_the_truncated_sine_pair() {
        let lut = retail_rotation_lut();
        // Cardinal points of the 12-bit angle space.
        assert_eq!(lut.b(0), 0);
        assert_eq!(lut.b(1024), 4096); // sin(90 deg) at 1<<12 fixed point
        assert_eq!(lut.b(2048), 0);
        assert_eq!(lut.b(3072), -4096);
        // `a` is `b` advanced a quarter revolution (the retail +0x800-byte
        // pointer offset): cosine.
        assert_eq!(lut.a(0), 4096);
        assert_eq!(lut.a(1024), 0);
        assert_eq!(lut.a(2048), -4096);
        for angle in [1, 7, 100, 2047, 4095] {
            assert_eq!(lut.a(angle), lut.b((angle + 1024) & 0xFFF), "{angle}");
        }
        // Truncation toward zero, not rounding: entry 1 is
        // trunc(sin(2pi/4096)*4096) = trunc(6.283) = 6 (rounding would give
        // the same here, but entry 3 separates them: trunc(18.849) = 18).
        assert_eq!(lut.b(1), 6);
        assert_eq!(lut.b(3), 18);
        // A retail-LUT rotation preserves length like the synthetic one.
        let r = rotate_offset(lut, 300, FacingBias::Half, 1000, 250);
        let len = ((r.0 * r.0 + r.1 * r.1) as f64).sqrt();
        let src = ((1000.0f64 * 1000.0) + (250.0 * 250.0)).sqrt();
        assert!((len - src).abs() <= 4.0, "{r:?}");
    }

    #[test]
    fn scope_bytes_pick_a_row_or_a_single_slot() {
        assert_eq!(
            TargetBand::from_scope(9).slots().collect::<Vec<_>>(),
            vec![3, 4, 5, 6]
        );
        assert_eq!(
            TargetBand::from_scope(8).slots().collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        // The `0..=7` arm is a single slot, and it always runs once.
        for s in 0..8u8 {
            assert_eq!(
                TargetBand::from_scope(s).slots().collect::<Vec<_>>(),
                vec![s]
            );
        }
        // Out-of-range bytes fall through to the single-slot form.
        assert_eq!(
            TargetBand::from_scope(20).slots().collect::<Vec<_>>(),
            vec![20]
        );
    }

    #[test]
    fn a_unit_scale_passes_the_offset_through_and_truncates_toward_zero() {
        assert_eq!(scale_offset(100, 1 << ROT_SHIFT), 100);
        assert_eq!(scale_offset(-100, 1 << ROT_SHIFT), -100);
        // Half scale: 100 -> 50, -100 -> -50 (truncate, not floor).
        assert_eq!(scale_offset(100, 1 << (ROT_SHIFT - 1)), 50);
        assert_eq!(scale_offset(-100, 1 << (ROT_SHIFT - 1)), -50);
        // A fractional negative rounds toward zero, so a small offset vanishes
        // rather than becoming -1.
        assert_eq!(scale_offset(-1, 1), 0);
    }

    #[test]
    fn the_rotation_is_facing_dependent_and_preserves_length() {
        let lut = Lut::new();
        let len = |(x, z): (i32, i32)| (((x * x + z * z) as f64).sqrt()) as i32;
        let base = rotate_offset(&lut, 0, FacingBias::None, 1000, 0);
        let quarter = rotate_offset(&lut, 1024, FacingBias::None, 1000, 0);
        assert_ne!(base, quarter, "a quarter turn must move the offset");
        // The four-read pair is a rotation, so the magnitude survives (within
        // the fixed point's rounding).
        assert!(
            (len(base) - len(quarter)).abs() <= 4,
            "{base:?} {quarter:?}"
        );
        assert!((len(base) - 1000).abs() <= 4, "{base:?}");
    }

    #[test]
    fn the_half_bias_is_the_opposite_facing() {
        let lut = Lut::new();
        let none = rotate_offset(&lut, 0, FacingBias::None, 1000, 500);
        let half = rotate_offset(&lut, 0, FacingBias::Half, 1000, 500);
        // A half-revolution bias negates the rotated offset (to within the
        // fixed point's independent per-product rounding).
        assert!((none.0 + half.0).abs() <= 4, "{none:?} {half:?}");
        assert!((none.1 + half.1).abs() <= 4, "{none:?} {half:?}");
        // And the bias a record picks follows its direct bit.
        assert_eq!(FacingBias::for_effect(0x10), FacingBias::Half);
        assert_eq!(
            FacingBias::for_effect(EFFECT_DIRECT_BIT | 0x10),
            FacingBias::None
        );
    }

    #[test]
    fn move_power_index_reads_one_before_the_action() {
        let map = [7u8, 9, 11, 13];
        // action 1 -> map[0] = 7.
        assert_eq!(
            move_power_record_offset(&map, 1),
            Some(7 * MOVE_POWER_STRIDE)
        );
        // action 4 -> map[3] = 13.
        assert_eq!(
            move_power_record_offset(&map, 4),
            Some(13 * MOVE_POWER_STRIDE)
        );
        // action 0 has no record.
        assert_eq!(move_power_record_offset(&map, 0), None);
        // past the map end.
        assert_eq!(move_power_record_offset(&map, 9), None);
    }

    #[test]
    fn the_walk_spawns_then_terminates_and_installs_the_move_power() {
        let lut = Lut::new();
        let b = block(&[
            (1, 0x10, 100, 0, 0),
            (1, 0x11, 0, 0, 100),
            (1, 0xFF, 0, 0, 0),
        ]);
        let map = [0u8, 0, 0, 0, 3];
        let s = step_effect_script(&lut, &b, actor(), 4, &map);
        assert_eq!(s.spawns.len(), 2);
        // The terminator advances the cursor too, then the zero-filled tail's
        // `+0x00 == 0` gate stops the walk.
        assert_eq!(s.cursor, 3);
        assert_eq!(s.move_power_offset, Some(3 * MOVE_POWER_STRIDE));
        assert_eq!(s.homing_band, Some(TargetBand { first: 3, last: 6 }));
        assert!(!s.spawns[0].direct);
    }

    #[test]
    fn the_terminator_carries_its_own_placement_as_the_launch_point() {
        let lut = Lut::new();
        // A terminator with a non-zero offset: retail rotates it like any
        // other record before it notices the record ends the stream, so the
        // launch point is NOT the bare actor position.
        let b = block(&[(1, 0xFF, 100, 40, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 4, &[0u8; 8]);
        let launch = s.launch.expect("terminator seeds a launch point");
        assert_ne!(launch, actor().world, "the offset is applied");
        // Y is subtracted, unrotated.
        assert_eq!(launch.1, actor().world.1 - 40);
        // A zero-offset terminator does land on the actor.
        let b0 = block(&[(1, 0xFF, 0, 0, 0)]);
        let s0 = step_effect_script(&lut, &b0, actor(), 4, &[0u8; 8]);
        assert_eq!(s0.launch, Some(actor().world));
    }

    #[test]
    fn a_step_that_does_not_terminate_seeds_nothing() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 4, &[0u8; 8]);
        assert_eq!(s.launch, None);
        let mut blk = MoveFxStreak::default();
        assert!(!blk.install(&s, Some(0x300)));
        assert!(!blk.is_armed());
        assert_eq!(blk, MoveFxStreak::default());
    }

    #[test]
    fn the_terminator_installs_the_streak_block() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0), (1, 0xFF, 0, 0, 0)]);
        let map = [0u8, 0, 0, 0, 3];
        let s = step_effect_script(&lut, &b, actor(), 4, &map);

        let mut blk = MoveFxStreak::default();
        assert!(blk.install(&s, Some(0x300)));
        // `ctx[+0x1014]` as an id: the offset the terminator resolved, back
        // through the table stride.
        assert_eq!(blk.move_id, Some(3));
        // `ctx[+0x6C6]` verbatim, and the projector's half-width from it.
        assert_eq!(blk.counter_word, 0x300);
        assert_eq!(blk.half_width(), 0x100);
        assert_eq!(blk.launch, s.launch);
        assert_eq!(blk.phase, 1);
        assert_eq!(blk.band, s.homing_band);
        assert!(blk.is_armed());

        // No catalog: the `sh` is skipped and the previous word stands.
        let mut again = blk;
        assert!(again.install(&s, None));
        assert_eq!(again.counter_word, 0x300);

        blk.clear();
        assert!(!blk.is_armed());
    }

    #[test]
    fn half_width_is_the_context_word_minus_0x200_and_wraps() {
        // The projector argument is a signed 16-bit subtract, so a small
        // counter yields a negative half-width - retail passes it through.
        let mut blk = MoveFxStreak {
            counter_word: 0x100,
            ..Default::default()
        };
        assert_eq!(blk.half_width(), -0x100);
        blk.counter_word = 0x200;
        assert_eq!(blk.half_width(), 0);
    }

    #[test]
    fn both_terminator_encodings_end_the_stream() {
        // The direct branch's `0xFF` and the table branch's `0x7F` are the
        // same marker seen through the bit-0x80 branch split.
        for e in [0x7Fu8, 0xFF] {
            let lut = Lut::new();
            let b = block(&[(1, e, 0, 0, 0)]);
            let s = step_effect_script(&lut, &b, actor(), 4, &[0u8, 0, 0, 0, 1]);
            assert!(s.spawns.is_empty(), "effect {e:#x} should not spawn");
            assert!(s.move_power_offset.is_some(), "effect {e:#x}");
        }
        // A neighbouring id is an ordinary effect.
        let lut = Lut::new();
        let b = block(&[(1, 0x7E, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 4, &[]);
        assert_eq!(s.spawns.len(), 1);
        assert!(s.move_power_offset.is_none());
    }

    #[test]
    fn the_direct_bit_routes_to_the_other_spawn_form() {
        let lut = Lut::new();
        let b = block(&[(1, EFFECT_DIRECT_BIT | 0x13, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 1, &[]);
        assert_eq!(s.spawns.len(), 1);
        assert!(s.spawns[0].direct);
        assert_eq!(s.spawns[0].effect, EFFECT_DIRECT_BIT | 0x13);
    }

    #[test]
    fn a_future_frame_gate_defers_without_advancing_the_cursor() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0), (9, 0x11, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 0, &[]);
        // frame 0 + 1 == 1 clears the first gate but not the second.
        assert_eq!(s.spawns.len(), 1);
        assert_eq!(s.cursor, 1);
        assert_eq!(s.move_power_offset, None);
    }

    #[test]
    fn a_suppressed_actor_does_nothing() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0)]);
        let mut a = actor();
        a.suppressed = true;
        let s = step_effect_script(&lut, &b, a, 4, &[]);
        assert!(s.spawns.is_empty());
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn an_exhausted_cursor_does_nothing() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0)]);
        let mut a = actor();
        a.cursor = MAX_CURSOR;
        let s = step_effect_script(&lut, &b, a, 4, &[]);
        assert!(s.spawns.is_empty());
        assert_eq!(s.cursor, MAX_CURSOR);
    }

    #[test]
    fn the_y_offset_is_subtracted_not_added() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 300, 0)]);
        let a = actor();
        let s = step_effect_script(&lut, &b, a, 1, &[]);
        assert_eq!(s.spawns[0].at.1, a.world.1 - 300);
    }
}
