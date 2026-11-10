//! The **capture-class** cast modules' camera arms (PROT 0935..0966): the
//! shots and drifts a monster's special arms while the capture band's `0x70`
//! re-enters it.
//!
//! `0x70` calls no framing case (`0x801E50C8..0x801E50E8`: the module tick
//! `FUN_801F2160`, then `0x71` once it returns zero), so the camera through
//! the module's run is whatever the module does to it - the last case-6 pose
//! `0x6F` armed, then the module's own `FUN_801D829C` shots and the drifts it
//! adds straight into the camera globals. The module's phase chain (its
//! simulation) is ported in [`crate::cast_module_ticks`]; this is the camera
//! half, run beside it on the phase the body is about to run.
//!
//! A drift is per battle frame `k * delta` with `delta = *(0x1F800393)`, the
//! vsyncs a battle frame spans; the engine ticks once per vsync, so a drift
//! here is `k` per tick.

use super::{
    ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, bearing_12bit_approx, focus_on,
    yaw_from,
};

/// A capture arm's direct writes into the live camera globals, every pass the
/// arm runs (held or not): pitch / yaw `0x8007B790` / `0x8007B792`, TR y / z
/// `0x800840BC` / `0x800840C0` (the TR z delta in the global's own prescaled
/// units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureDrift {
    pub pitch: i16,
    pub yaw: i16,
    pub tr_y: i16,
    pub tr_z: i16,
    /// TR x `0x800840B8` - walked by PROT 0966's arms only.
    pub tr_x: i16,
}

/// A capture arm's stores into the parts of `records` it holds handles to:
/// first `dx` / `dy` added to `+0x14` / `+0x16`, then `x` / `y` stored over
/// them. The adds run on every pass, the stores on the arm's passing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapturePartWrite {
    pub records: &'static [u32],
    pub dx: i16,
    pub dy: i16,
    pub x: Option<i16>,
    pub y: Option<i16>,
    /// A word stored into both packed colours `+0xA0` / `+0xA4`.
    pub colours: Option<u32>,
    /// `+0x84` (the spin rate the move VM's op `0x04` seeds) shifted left by
    /// this many bits.
    pub spin_84_shl: u8,
    /// A store into the depth-cue rate `+0x90`.
    pub level_rate: Option<i16>,
}

/// What a capture module's camera does on one pass of one arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureCamArm {
    /// The arm's pass stores the caster's size class (record `+0x1F`) and body
    /// radius (`+0x58` behind `+0x22C`), the words the range poll and the
    /// separation pass read.
    pub caster_reach: Option<(u8, i16)>,
    /// The arm's pass stores the caster's body radius alone (`+0x58` behind
    /// `+0x22C`), the size class left as it stands: PROT 0958's arm 8 zeroes
    /// it ahead of the caster's runs through its victim (`0x801F76A8`).
    pub caster_radius: Option<i16>,
    /// The cue the arm's pass hands the dispatcher (`FUN_8004FCC8(id)`), for
    /// a call the module makes on its own and no hit or clip sounds: PROT
    /// 0958's arm 21 (`0x56` at `0x801F87BC`).
    pub cue: Option<u16>,
    /// The arm's pass puts the caster's body radius back on its size class
    /// (`record[+0x1F] << 5`): PROT 0958's `0xFF` arm (`0x801F8CCC..0x801F8CE4`).
    pub caster_radius_restore: bool,
    /// The arm's pass calls the seat installer `FUN_801F19EC`: the stream's
    /// record stands as seat 7, on the caster.
    pub seats_creature: bool,
    /// The arm's pass takes the seat off: node `+0x10 |= 8`, HP `0`, colour `0`.
    pub hides_creature: bool,
    /// The arm's pass halts the part pool (`FUN_80050E74`), for a module whose
    /// arm numbers mean different arms under another action.
    pub flushes_pool: bool,
    /// The cast opens on a stream the caster's turn made: its `readef.DAT`
    /// group's texture pages are in VRAM.
    pub element_stream: bool,
    /// The `FUN_801D829C` call the arm makes, if any.
    pub shot: Option<ModuleShot>,
    /// The drift the arm adds to the globals, if any.
    pub drift: Option<CaptureDrift>,
    /// The arm's countdown gate holds: the body's phase chain must not run
    /// this pass (retail's arm returns busy before any of its writes).
    pub hold: bool,
    /// For a body whose phase chain is **not** ported, the phase a passing
    /// arm moves to (`None` on the arm that finishes). The director then
    /// owns the module phase, the way a camera-only player director does; a
    /// ported body ignores it.
    pub next: Option<u8>,
    /// A store into the caster's battle-scoped latch word
    /// (`0x801C8FE0 + (ctx[+0x13] + 1) * 4`, the monster AI's ability
    /// cooldown `dat[m + 4]`), when the arm makes one.
    pub latch: Option<i32>,
    /// The arm took a branch that applies no damage: the band's fold owes
    /// this cast nothing.
    pub skips_fold: bool,
    /// Absolute stores the arm makes into the camera globals on this pass,
    /// applied after [`Self::drift`] and before [`Self::shot`] (whose tween
    /// builds from them).
    pub pose_set: Option<CapturePoseSet>,
    /// A drift of the focus z global `0x80089120` (world units, the
    /// un-negated focus), per tick like [`Self::drift`].
    pub focus_z: i16,
    /// The arm's stores into the caster seat `actor_table[ctx + 0x13]`.
    pub caster_write: CaptureSeatWrite,
    /// The arm's stores into the victim seat `actor_table[caster + 0x1DD]`.
    pub victim_write: CaptureSeatWrite,
    /// The arm stores `0x1000` into both backdrop records' depth-cue weight
    /// `+0x78` (`ctx[+0x106C]` / `ctx[+0x1070]`), which takes the pair off
    /// the draw while the stage byte holds the ramp's ceiling there.
    pub backdrop_off: bool,
    /// A literal store into the stage byte `ctx[+0x278]` on this pass.
    pub ctx_278: Option<u8>,
    /// `(clip, [count, start, end])`: the arm's stores into the loop-window
    /// bytes `+0x84..+0x86` of a monster caster's action entry `clip`
    /// (`*(record + 0x4C + clip * 4)`, the record off `0x801C9348 + (seat - 3) * 4`),
    /// which the clip commit seeds the loop counter `+0x21B` / budget `+0x176`
    /// from. Made ahead of the arm's caster stage.
    pub caster_clip_window: Option<(u8, [u8; 3])>,
    /// The arm drifts ahead of its gate and shoots after it, so a pass's
    /// cut lands on its own endpoint (the drift is spent before the cut).
    pub drift_first: bool,
    /// A damage-wrapper hit the arm lands on this pass, by its site index
    /// into the module's baked powers
    /// ([`crate::cast_module_ticks::damage_shape_for`]); the module then
    /// owns the cast's outcome and the band's fold is waived.
    pub hit_site: Option<u8>,
    /// The split hit ([`crate::cast_module_ticks::SplitHits`]) the arm lands
    /// on this pass, numbered from `1`: the band rolls and applies it
    /// (PROT 0937's arm 7).
    pub split_hit: Option<u8>,
    /// The arm framed the caster through the action SM's **case 6**
    /// (`FUN_801D5854(ctx[+0x13], 6)`) on this pass: the follow framing on
    /// the live yaw ladder `ctx[+0x6DA]` and depth `ctx[+0x6D0]`.
    pub follow_caster: bool,
    /// The framing style `ctx[+0xD]` this pass's case 6 reads, where the arm
    /// stores its own ahead of the call ([`super::ModuleFollow::style`]).
    pub follow_style: Option<u8>,
    /// The arm framed the caster through the action SM's **case 8**
    /// (`FUN_801D5854(ctx[+0x13], 8)`) on this pass: the post-action
    /// framing on the caster and its target.
    pub end_frame: bool,
    /// The arm calls the damage wrapper on this pass (a bespoke tick port
    /// whose hit the band's fold carries): the owed cast folds here, mid-band,
    /// rather than at the band's exit.
    pub folds_hit: bool,
    /// The arm's stores into every party seat `actor_table[0 .. ctx[+0]]`
    /// (PROT 0960 `0xA6`'s hide and show).
    pub party_write: CaptureSeatWrite,
    /// [`Self::party_write`] walks only the seats with HP and without Stone
    /// (PROT 0946's arm 7, `0x801F73A8..0x801F73C4`), read after the fold.
    pub party_live_only: bool,
    /// [`Self::party_write`] also skips a Stoned seat (`+0x16E & 4`).
    pub party_skip_stoned: bool,
    /// [`Self::party_write`] skips the victim's seat (PROT 0951 `0x36`'s
    /// arm 2 walks `0 .. ctx[+0x00]` past `caster[+0x1DD]`,
    /// `0x801F6E48..0x801F6E90`).
    pub party_skip_victim: bool,
    /// The pass's fold turns each seat it reaches on the caster:
    /// `FUN_80019B28(caster[+0x38], caster[+0x34], seat[+0x38], seat[+0x34])
    /// + 0x800` into `+0x46` (PROT 0957's arm 5, `0x801F75A4..0x801F75CC`).
    pub party_faces_caster: bool,
    /// Units a vsync every party seat is pulled along its bearing from the
    /// caster plus `0x800`, on every pass of the arm:
    /// PROT 0938's `0x4E` arm 1 (`0x801F74C0..0x801F7564`).
    pub party_shove: Option<i16>,
    /// The pass seats [`crate::cast_module_camera::STORM_SCATTER_PAIRS`] pairs of
    /// [`crate::cast_module_camera::STORM_SCATTER_RECORDS`] on `rand()` points
    /// (PROT 0957's arms 4 to 6): `x`, `y` each `0x80 - rand() % 255`, `z = 0x100`.
    pub scatter_pairs: bool,
    /// [`Self::ribbons`] take their height off the `y` the scatter's last pair
    /// drew: the two loops share one stack trio (PROT 0957's arm 5).
    pub ribbons_on_scatter_y: bool,
    /// The arm stores this into every live part's level `+0x78`
    /// (PROT 0957's arm 6, `0x801F78B8..0x801F78E4`).
    pub parts_level: Option<u16>,
    /// The arm's stores into the position halfwords `+0x14` / `+0x16` of the
    /// parts it seated through handles (PROT 0942 `0xAA`'s vortex).
    pub part_write: Option<CapturePartWrite>,
    /// The arm's stores into every party seat and every live monster, made
    /// ahead of its [`Self::caster_write`] / [`Self::victim_write`] (PROT
    /// 0947 hides the field, then shows the caster). Waits for the band's
    /// settle on a finishing arm.
    pub others_write: CaptureSeatWrite,
    /// [`Self::others_write`] skips a downed party seat as it does a downed
    /// monster (PROT 0955 `0x70` walks seven seats behind one HP test).
    pub others_live_only: bool,
    /// The `MoveImage` (`FUN_80058490`) the arm issues on this pass.
    pub vram_move: Option<super::ModuleVramMove>,
    /// PROT 0935's arms 4 and 5: the pass's rock pair at a point off the
    /// quadrant in the pass word, on the host's `rand()` draws
    /// ([`super::rock_point`]).
    pub rock_burst: Option<u8>,
    /// PROT 0964's Thunder `k = 5`: these two records, on the host's draws, at
    /// `(0x80 - r % 257, 0x80 - r % 257, 0x100)` (`0x801F767C..0x801F7708`).
    pub spray: Option<[u32; 2]>,
    /// PROT 0941's arm 1: three records at each of eight points round the
    /// origin, `(sin / 2, 0, cos / 2)` at `i * 0x200`, the rot trio beside them,
    /// on the host's `rand() % 8` slot draws (`0x801F6CD8..0x801F6E28`).
    pub ring: Option<([u32; 3], [i16; 3])>,
    /// `(record, first)`: this pass seats two copies of a record at the
    /// origin, each on three `rand()` draws stored over it - `+0x84 =
    /// r % 3 - 1`, `+0xB4 = r % 0x400 + 0x80`, `+0x28 = n * 0x600 + r % 0x200`
    /// for its running index `n` from `first` (PROT 0953's arm 6,
    /// `0x801F711C..0x801F71F4`).
    pub trail: Option<(u32, u16)>,
    /// `(record, count)`: this pass seats `count` copies of a record each on
    /// five `rand()` draws (PROT 0951's arm 4, `0x801F7E34..0x801F7F14`:
    /// `x = r % 64 - 32`, `y = r % 64 - 16`, `z = 0x100`, then the angle trio
    /// roll, pitch, yaw, each `r % 4095`).
    pub scatter: Option<(u32, u8)>,
    /// The `FUN_80021B04` calls this pass makes at points the arm works out,
    /// held or not. Seated on the tick that opens a battle frame.
    pub pass_spawns: [Option<super::ModuleSpawn>; 5],
    /// A passing arm's larger run of the same calls (PROT 0948's arm 1).
    pub charge_spawns: [Option<super::ModuleSpawn>; 8],
    /// The arm stores `0x800` into both backdrop records' `+0x78`, the
    /// indoor ceiling: the pair back on the draw (PROT 0965 arms 7, `0x0B`).
    pub backdrop_on: bool,
    /// The arm lines the party seats up for a row sweep (PROT 0965 arm
    /// `0x0B`, [`super::DOOMSDAY_LINE_UP_X`]), the host keeping each seat's
    /// pair and heading for the body's `0xFF` arm to restore.
    pub party_line_up: bool,
    /// The chained streak ribbons (`FUN_801E1D98`) the arm draws on this
    /// pass, ahead of its gate: PROT 0962 `0xA3`'s arm-4 ring
    /// ([`super::thunder_needle_ring`]). A per-pass call in retail, so the
    /// host keeps it for this tick only.
    pub ribbons: Option<CastRibbonRing>,
}

/// One pass's streak-ribbon calls from a module arm: each point is a
/// battle-world ground point handed to `FUN_801E1D98` with `trail_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CastRibbonRing {
    pub points: [[i16; 3]; 16],
    pub trail_id: u8,
    /// How many of [`Self::points`] the pass draws.
    pub count: u8,
}

/// Absolute stores a capture arm makes into the camera globals: yaw
/// `0x8007B792`, TR y / z `0x800840BC` / `0x800840C0` (TR z in the global's
/// own prescaled units) and the focus pair `0x80089118` / `0x80089120`
/// (world units, un-negated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapturePoseSet {
    pub yaw: Option<i16>,
    pub tr_y: Option<i32>,
    pub tr_z: Option<i32>,
    pub focus_xz: Option<[i16; 2]>,
}

/// A capture arm's stores into one battle seat: the live pair `+0x34` /
/// `+0x38` (absolute, or `dz` added), the height `+0x36` (absolute, or `dy`
/// added; an absolute store lands after the add), the heading `+0x46`, the
/// colour word `+0x04`, the render flag `+0x21C`, the anim rate `+0x21D`
/// and the anim node's cursor `*(+0x22C) + 0x68` zeroed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureSeatWrite {
    pub rate: Option<u8>,
    /// A clip staged onto `+0x1DA`.
    pub stage: Option<u8>,
    /// The [`Self::stage`] store came with the `+0x1DC` bump, which cuts the
    /// playing clip at once; without it the byte waits for the clip's end.
    pub restage: bool,
    /// The loop counter `+0x21B` / budget `+0x176` cleared: the playing
    /// clip's park is released.
    pub loop_release: bool,
    pub x: Option<i16>,
    pub z: Option<i16>,
    pub dz: i16,
    pub y: Option<i16>,
    pub dy: i16,
    pub facing: Option<u16>,
    pub colour: Option<u32>,
    /// The tint weight `+0x0C` the colour word draws under.
    pub blend: Option<u32>,
    pub render_flag: Option<u8>,
    pub cursor_reset: bool,
    /// A store into the seat's node cursor `+0x68` (12.4).
    pub seek: Option<u16>,
    /// The seat's node cursor `+0x68` raised to this where it reads under
    /// it (`slti; beq; sh`): PROT 0958's arms 10, 13 and 16 on the victim.
    pub seek_floor: Option<u16>,
    /// The seat's own knockdown slot `+0x1F1` staged onto `+0x1DA` with the
    /// `+0x1DC` bump (`lbu a0,0x1f1; sb a0,0x1da`): the reaction a body
    /// stages itself beside its damage call (PROT 0958's arms 6, 9, 12, 15).
    pub stage_knockdown: bool,
    /// The seat's committed-clip ring `+0x1FB..+0x21A` zeroed: the
    /// after-image walk `FUN_80049348` gates each ghost on its slot's id, so
    /// the trail the playing clip left goes with the store (PROT 0958's
    /// arm 7 on the caster).
    pub ghost_ring_clear: bool,
}

impl CaptureSeatWrite {
    /// Whether the arm writes nothing into the seat.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One capture body's camera arms, by the module phase `ctx[+0x279]` the
/// body is about to run.
pub type CaptureCamDirector = fn(&mut ModuleCamState, u8, ModuleCamSeats) -> CaptureCamArm;

/// The camera director of a capture-class body, keyed on `(entry, body)` as
/// the trampoline map is ([`crate::cast_module_ticks::capture_tick_body`]).
/// A module with no trampoline has one tick body and is keyed on
/// [`SINGLE_BODY`]. `None` for a body whose camera arms are not ported: the
/// camera then holds the pose `0x6F` left.
pub fn capture_camera_director(entry: u32, body: u32) -> Option<CaptureCamDirector> {
    match (entry, body) {
        (940, MYSTIC_SHIELD_BODY) => Some(mystic_shield_camera),
        (940, GLARE_BODY) => Some(glare_camera),
        (940, crate::cast_arm_ticks::GLARE_DIVIDE_SPLIT_TICK) => Some(slime_divide_camera),
        (952, super::BLOODY_HORNS_BODY) => Some(super::bloody_horns_camera),
        (952, super::ASTRAL_SLASH_BODY) => Some(super::astral_slash_camera),
        (945, crate::cast_module_ticks::WATER_COLUMN_TICK) => Some(super::water_column_camera),
        (943, crate::cast_arm_ticks::CURSE_MP_DRAIN_TICK) => Some(super::lapis_wave_camera),
        (956, crate::cast_arm_ticks::WATER_HAZARD_TICK) => Some(super::water_hazard_camera),
        (956, super::PARALYZING_WAVE_BODY) => Some(super::paralyzing_wave_camera),
        (949, SINGLE_BODY) => Some(super::water_crystals_camera),
        (953, SINGLE_BODY) => Some(terio_punch_camera),
        (939, SINGLE_BODY) => Some(super::spore_gas_camera),
        (944, GUILTY_CROSS_BODY) => Some(guilty_cross_camera),
        (962, ULTRA_CHARGE_BODY) => Some(ultra_charge_camera),
        (961, DEAD_END_CRISIS_BODY) => Some(dead_end_crisis_camera),
        (938, MYSTIC_CIRCLE_BODY) => Some(mystic_circle_camera),
        (938, super::CHAOS_BREATH_BODY) => Some(super::chaos_breath_camera),
        (946, SINGLE_BODY) => Some(wave_camera),
        (966, SINGLE_BODY) => Some(super::evil_seru_magic_camera),
        (958, super::BLAZING_SLASH_BODY) => Some(super::blazing_slash_camera),
        (959, super::MEGATON_PRESS_BODY) => Some(super::megaton_press_camera),
        (960, super::PLASMA_STRIKE_BODY) => Some(super::plasma_strike_camera),
        (960, super::NEO_STAR_SLASH_BODY) => Some(super::neo_star_slash_camera),
        (962, super::BLADE_BREATH_BODY) => Some(super::blade_breath_camera),
        (962, super::THUNDER_NEEDLE_BODY) => Some(super::thunder_needle_camera),
        (962, super::GIGATON_PRESS_BODY) => Some(super::gigaton_press_camera),
        (965, super::DOOMSDAY_BODY) => Some(super::doomsday_camera),
        (955, super::KISS_OF_DEATH_BODY) => Some(super::kiss_of_death_camera),
        (947, SINGLE_BODY) => Some(super::v_windhash_camera),
        (948, SINGLE_BODY) => Some(super::cross_beam_camera),
        (955, super::TERROR_SCREAM_BODY) => Some(super::terror_scream_camera),
        (955, super::MELT_SPRAY_BODY) => Some(super::melt_spray_camera),
        (950, super::SHADOW_BREAK_BODY) => Some(super::shadow_break_camera),
        (950, super::ROLLING_FLARE_BODY) => Some(super::rolling_flare_camera),
        (942, super::POWER_UP_BODY) => Some(super::power_up_camera),
        (942, super::DARK_TYPHOON_BODY) => Some(super::dark_typhoon_camera),
        (935, SINGLE_BODY) => Some(super::earthquake_camera),
        (936, SINGLE_BODY) => Some(super::hyper_crush_camera),
        (937, SINGLE_BODY) => Some(super::hyper_lightning_camera),
        (941, super::STONE_CIRCLE_BODY) => Some(super::stone_circle_camera),
        (964, super::ELEMENT_CHANGE_BODY) => Some(super::element_change_camera),
        (964, super::ELEMENT_STRIKE_BODY) => Some(super::element_strike_camera),
        (951, super::SCYTHE_WIND_BODY) => Some(super::scythe_wind_camera),
        (951, super::CHAOS_FLARE_BODY) => Some(super::chaos_flare_camera),
        (957, super::DEATH_GAME_BODY) => Some(super::death_game_camera),
        (957, super::THUNDER_STORM_BODY) => Some(super::thunder_storm_camera),
        (941, super::STEAL_BODY) => Some(super::steal_camera),
        (944, super::CURSE_ALL_BODY) => Some(super::curse_all_camera),
        (955, super::VOID_ACCESSORIES_BODY) => Some(super::void_accessories_camera),
        (955, super::WHITE_SHIELD_BODY) => Some(super::white_shield_camera),
        (955, super::POWER_CHARGE_BODY) => Some(super::power_charge_camera),
        _ => None,
    }
}

/// Whether a capture body's arm `phase` re-arms its shot on **every pass**,
/// ahead of the arm's countdown gate, so the tween table the RAM holds was
/// built on the pass the RAM is in, off the gap that pass saw, and each pass
/// before it stood the re-arm's wider share back - not a table built once and
/// walked since. Each listed arm calls the builder `FUN_801D829C` (`a3 =
/// 0xC`) before its `bgtz` / `blez` hold:
///
/// | PROT | body | arms | builder calls |
/// |---|---|---|---|
/// | 0951 | `0x801F77E8` (`0x5B` Scythe Wind) | 3..=5 | `0x801F7CD4`, `0x801F7E08`, `0x801F80B0` |
/// | 0952 | `0x801F7118` (`0x5C` Bloody Horns) | 3..=6 | `0x801F7624`, `0x801F7798`, `0x801F78EC`, `0x801F7A7C` |
/// | 0952 | `0x801F6A0C` (`0xB8` Astral Slash) | 3, 4 | `0x801F6F8C`, `0x801F70BC` |
///
/// The first shot of each (arm 2, `0x801F7BCC` / `0x801F74F4`) is built
/// once. Kiss of Death (PROT 0955) builds every shot once: arm 0 at
/// `0x801F883C`, and arms 1 and 3 behind their gates (`0x801F8A10` past the
/// `bgtz` at `0x801F89A0`, `0x801F8C1C` past `0x801F8BAC`).
pub fn capture_shot_rearms_every_pass(entry: u32, body: u32, phase: u8) -> bool {
    match (entry, body) {
        (951, super::SCYTHE_WIND_BODY) => (3..=5).contains(&phase),
        (952, super::BLOODY_HORNS_BODY) => (3..=6).contains(&phase),
        (952, super::ASTRAL_SLASH_BODY) => (3..=4).contains(&phase),
        _ => false,
    }
}

/// The yaw counter `ctx[+0x6DA]` a capture body's **finishing arm** leaves:
/// most bodies end on the same three stores - the busy result cleared, `sb
/// zero,0xd` (the framing style) and `li 0x780` / `sh` into `ctx[+0x6DA]`
/// (directly, or as `+2` off the `ctx + 0x6D8` base the body holds) - so
/// the Done band's case 6 that follows frames from `0x780` plus its own
/// drift, whatever the module did to the counter before. Keyed on `(entry,
/// body)` like [`capture_camera_director`] (`SINGLE_BODY` for a module with
/// no trampoline). `None` for a body whose finish leaves the counter alone.
///
/// | PROT | body | store |
/// |---|---|---|
/// | 0938 | `0x801F69EC` (`0xB7`) | `0x801F723C` |
/// | 0940 | `0x801F69F8` (`0x3C`), `0x801F7240` (`0xAC`) | `0x801F7208`, `0x801F788C` |
/// | 0941 | `0x801F6A04` (`0xB9`) | `0x801F72D4` |
/// | 0942 | `0x801F69F4` (`0xAA`) | `0x801F7C70` |
/// | 0943 | `0x801F6A04` (`0xB5`) | `0x801F6ECC` |
/// | 0944 | `0x801F6A04` (`0x37`) | `0x801F7438` |
/// | 0945 | `0x801F69F8` (`0xBA`) | `0x801F6EAC` |
/// | 0950 | `0x801F6A24` (`0xAB`) | `0x801F7984` |
/// | 0951 | `0x801F6A20` (`0x36`) | `0x801F77B0` |
/// | 0952 | `0x801F6A0C` (`0xB8`) | `0x801F70E8` |
/// | 0956 | `0x801F69D8` (`0x75`) | `0x801F7218` |
/// | 0957 | `0x801F6A14` (`0x77`) | `0x801F790C` |
/// | 0961 | `0x801F69D8` | `0x801F7840` |
/// | 0963 | `0x801F6A20` | `0x801F8110` |
/// | 0964 | `0x801F69D8` (`0xB0..=0xB2`), `0x801F88EC` (`0xAF`) | `0x801F8874`, `0x801F8BD0` |
/// | 0965 | `0x801F69D8` | `0x801F7AA4` |
/// | 0966 | single body `0x801F6A74` | `0x801F8A90` |
///
/// Four bodies zero the counter instead (`sh zero,0x2(base)`): PROT 0958
/// Blazing Slash (`0x801F8C58`, on its arm 25), PROT 0959 Megaton Press
/// (`0x801F8214`), PROT 0960 Plasma Strike (`0x801F85FC`) and Neo Star Slash
/// (`0x801F74A8`), the last three beside their `sb zero,0xd`. Retail films of
/// all four read the counter `0` on `0x71`, and the first Done pass adds its
/// drift.
///
/// The bodies of 0946..0948 and 0953 clear `ctx[+0x0D]` without the store
/// ([`capture_exit_clears_style`]).
pub fn capture_exit_yaw_base(entry: u32, body: u32) -> Option<i32> {
    const YAW: i32 = 0x780;
    match (entry, body) {
        (938, 0x801F_69EC)
        | (940, 0x801F_69F8 | 0x801F_7240)
        | (941 | 943 | 944, 0x801F_6A04)
        | (942, 0x801F_69F4)
        | (945, 0x801F_69F8)
        | (950, 0x801F_6A24)
        | (951, 0x801F_6A20)
        | (952, 0x801F_6A0C)
        | (956 | 961 | 965, 0x801F_69D8)
        | (957, 0x801F_6A14)
        | (963, 0x801F_6A20)
        | (964, 0x801F_69D8 | 0x801F_88EC)
        | (966, SINGLE_BODY) => Some(YAW),
        (958, super::BLAZING_SLASH_BODY)
        | (959, super::MEGATON_PRESS_BODY)
        | (960, super::PLASMA_STRIKE_BODY | super::NEO_STAR_SLASH_BODY) => Some(0),
        _ => None,
    }
}

/// Whether a capture module's finishing arm clears the framing style
/// `ctx[+0x0D]` that state `0x70` stores `1` on every pass. PROT 0935, 0936,
/// 0937, 0939 and 0962 hold no `sb zero,0xd`, so their Done band's case 6 frames
/// from half a turn round (`teien_berserker__spore_gas`'s `0x51` endpoint reads
/// `ctx[+0x6DA] - facing + 0x800`). PROT 0941's Steal (`0x51`) zeroes it
/// only through its arm 2 (`0x801F7B44`) and stores the `1` back on that arm's
/// pass (`0x801F7BF8`), so it leaves the style set too; the module's `0xB9`
/// body clears it on its last arm (`0x801F72CC`).
///
/// Three more bodies of split modules finish with no `sb zero,0xd` at all,
/// where their module's other body has one: PROT 0945's Water Column (`0x54`,
/// `0x801F6EDC`, finishing at `0x801F769C..0x801F76C4`), PROT 0938's Chaos
/// Breath (`0x4E`, `0x801F726C`) and PROT 0940's Slime Divide (`0x50` / `0xAE`,
/// `0x801F78B8`). A film of Water Column reads the half turn: the Done band's
/// yaw endpoint `4177` against `ctx[+0x6DA] - facing = 2129`.
pub fn capture_exit_clears_style(entry: u32, action: u8) -> bool {
    !matches!(entry, 935 | 936 | 937 | 939 | 962)
        && !matches!(
            (entry, action),
            (938, 0x4E) | (940, 0x50 | 0xAE) | (941, 0x51) | (945, 0x54)
        )
}

/// The module-resident countdown word a directed capture body gates its arms
/// on, by the caster's queued action id `+0x1DF` - what a save state's RAM
/// holds of where in an arm the module is.
pub fn capture_countdown_va(action: u8) -> Option<u32> {
    match action {
        0xAC | 0x3C => Some(MYSTIC_SHIELD_COUNTDOWN),
        // PROT 0940's split (`lui 0x8020` / `-0x79B4`).
        0x50 | 0xAE => Some(0x801F_864C),
        0x57 | 0xA7 => Some(super::V_WINDHASH_COUNTDOWN),
        0x58 => Some(super::CROSS_BEAM_COUNTDOWN),
        0x52 => Some(super::POWER_UP_COUNTDOWN),
        0x4A => Some(super::EARTHQUAKE_COUNTDOWN),
        0xB9 => Some(super::STONE_CIRCLE_COUNTDOWN),
        0xAF..=0xB2 => Some(super::ELEMENT_CHANGE_COUNTDOWN),
        // PROT 0954's word (`lui 0x8020` / `-0x6FE8`), mirrored by the host.
        0x5F => Some(0x801F_9018),
        0x5B => Some(super::SCYTHE_WIND_COUNTDOWN),
        // PROT 0955's cell shares the word (`lui 0x801F` / `-0x62D8`).
        0x60 | 0x70 | 0x73 => Some(super::KISS_OF_DEATH_COUNTDOWN),
        0x37 => Some(GUILTY_CROSS_COUNTDOWN),
        0xA5 => Some(ULTRA_CHARGE_COUNTDOWN),
        0xA1 | 0xB4 => Some(DEAD_END_CRISIS_COUNTDOWN),
        0xB7 => Some(MYSTIC_CIRCLE_COUNTDOWN),
        0x55 | 0x56 => Some(WAVE_COUNTDOWN),
        0xAD => Some(super::EVIL_SERU_MAGIC_COUNTDOWN),
        0xA6 => Some(NEO_STAR_SLASH_COUNTDOWN),
        0xB6 => Some(DOOMSDAY_COUNTDOWN),
        0x6E => Some(super::KISS_OF_DEATH_COUNTDOWN),
        _ => None,
    }
}

/// What one unit of [`capture_countdown_va`]'s word is worth in the
/// director's countdown ([`super::ModuleCountdown`]): `1` for a word drained
/// `scalar * delta` a pass, as the directors' own are, and
/// [`SPEED_SCALAR`] for PROT 0965's, which counts vsyncs (each arm drains it
/// by the frame step alone) and which its director keeps scaled.
pub fn capture_countdown_units(action: u8) -> i32 {
    match action {
        0xB6 => SPEED_SCALAR,
        _ => 1,
    }
}

/// PROT 0960's `0xA6` countdown word (Neo Star Slash): seeded `scalar << 5`
/// by arm 0, drained `scalar * delta` a pass.
pub const NEO_STAR_SLASH_COUNTDOWN: u32 = 0x801F_8E50;
/// PROT 0965's countdown word (Doomsday), in vsyncs: `0x20` from arm 0.
pub const DOOMSDAY_COUNTDOWN: u32 = 0x801F_8370;

/// PROT 0940's countdown word (`lui 0x8020` / `-0x79B4`).
pub const MYSTIC_SHIELD_COUNTDOWN: u32 = 0x801F_864C;

/// The key [`capture_camera_director`] takes for a module with no trampoline
/// (one tick body, the module's own head-table dispatch).
pub const SINGLE_BODY: u32 = 0;

/// PROT 0940's `0xAC` body (Cort's Mystic Shield).
pub const MYSTIC_SHIELD_BODY: u32 = 0x801F_7240;

/// Cort's **Mystic Shield** (PROT 0940, body `0x801F7240`, eight arms off the
/// table at the image head):
///
/// | arm | camera |
/// |---|---|
/// | 0 (`0x801F72E0`) | shot: pitch `0x200`, yaw `0x800 - caster[+0x46]`, TR `(0, -0x100, 0x400)`, focus the caster's seat, `0x40` frames |
/// | 2 (`0x801F73C4`) | drift: TR z `+12`, TR y `+6`, pitch `-2` |
/// | 4, 5 (`0x801F7584`, `0x801F76E8`) | drift: TR z `-6`, TR y `+1`, yaw `-16` |
/// | 6 (`0x801F77BC`) | drift: TR z `-8`, TR y `+2` |
///
/// Every arm from 1 on is gated on the module countdown `0x801F864C`
/// (drained `scalar * delta`, holding while it stays positive), and re-arms
/// it as it passes: arm 0 seeds `scalar * 0x60`, arm 1 adds `scalar << 8`,
/// arms 2 / 3 / 5 / 6 `scalar << 6` and arm 4 `scalar * 0xC0`; arm 7 is the
/// last gate before the body finishes. Arms 1, 3 and 7 write no camera. The
/// `cort_mystic_shield_mid_cast` capture sits in arm 1 with `496` of the
/// word left - `34` vsyncs into arm 0's `0x40`-frame shot: pitch `288` of
/// `0x200`, TR y `416` of `-0x100`.
///
/// PORT: FUN_801F7240 (PROT 0940; the camera arms 0/2/4/5/6 and the
/// countdown gates of arms 1..7)
pub fn mystic_shield_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    // The re-arm each gate makes as it passes, `scalar * n`.
    let rearm = |phase: u8| match phase {
        1 => Some(0x100),
        2 | 3 | 5 | 6 => Some(0x40),
        4 => Some(0xC0),
        _ => None,
    };
    let drift = match phase {
        2 => Some(drift(-2, 0, 6, 12)),
        4 | 5 => Some(drift(0, -16, 1, -6)),
        6 => Some(drift(0, 0, 2, -8)),
        _ => None,
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x60;
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0x200, yaw_from(0x800, c.facing), 0],
                    tr: [0, -0x100, 0x400],
                    focus: focus_on(c),
                    frames: 0x40,
                }),
                drift: None,
                hold: false,
                next: None,
                ..Default::default()
            }
        }
        1..=7 => {
            let hold = st.countdown.drain_above(0);
            if !hold && let Some(n) = rearm(phase) {
                st.countdown.add(n);
            }
            CaptureCamArm {
                shot: None,
                drift,
                hold,
                next: None,
                ..Default::default()
            }
        }
        _ => CaptureCamArm::default(),
    }
}

fn drift(pitch: i16, yaw: i16, tr_y: i16, tr_z: i16) -> CaptureDrift {
    CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x: 0,
    }
}

/// PROT 0944's `0x37` body (Cort's Guilty Cross).
pub const GUILTY_CROSS_BODY: u32 = 0x801F_6A04;
/// PROT 0944's countdown word (`lui 0x8020` / `-0x7CA0`).
pub const GUILTY_CROSS_COUNTDOWN: u32 = 0x801F_8360;

/// Cort's **Guilty Cross** (PROT 0944, body `0x801F6A04`, six arms off the
/// table at the image head), every arm from 1 on gated on the countdown
/// `0x801F8360` (drained `scalar * delta`):
///
/// | arm | camera | countdown as it passes |
/// |---|---|---|
/// | 0 (`0x801F6AC8`) | shot: pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x800)`, focus the caster, `0x20` frames | `= scalar << 7` |
/// | 1 (`0x801F6CC4`) | drift TR z `+4` | `+= scalar << 7` |
/// | 2 (`0x801F6E04`) | drift TR z `+4`; then a cut: pitch `0x100`, yaw `0x800 - victim[+0x46]`, TR `(0, 0x400, 0xA00)`, focus the victim | `+= scalar << 7` |
/// | 3 (`0x801F6FD4`) | drift TR z `+32` | `+= scalar << 8` |
/// | 4 (`0x801F7238`) | a cut back to arm 0's framing | `+= scalar * 0xC0` |
/// | 5 (`0x801F7378`) | drift TR z `+4` | - (the body finishes) |
///
/// The `cort_guilty_cross_mid_cast` capture sits in arm 1 with `456` of the
/// word left, on arm 0's framing.
///
/// PORT: FUN_801F6A04 (PROT 0944; the camera arms and the countdown gates)
pub fn guilty_cross_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let v = seats.victim;
    let behind_caster = |frames: u16| ModuleShot {
        angles: [0, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x600, 0x800],
        focus: focus_on(c),
        frames,
    };
    if phase == 0 {
        st.countdown.0 = SPEED_SCALAR << 7;
        return CaptureCamArm {
            shot: Some(behind_caster(0x20)),
            drift: None,
            hold: false,
            next: None,
            ..Default::default()
        };
    }
    let drift = match phase {
        1 | 2 | 5 => Some(drift(0, 0, 0, 4)),
        3 => Some(drift(0, 0, 0, 32)),
        _ => None,
    };
    if !(1..=5).contains(&phase) {
        return CaptureCamArm::default();
    }
    let hold = st.countdown.drain_above(0);
    let mut shot = None;
    let mut arm = CaptureCamArm::default();
    if !hold {
        match phase {
            1 => st.countdown.add(1 << 7),
            2 => {
                st.countdown.add(1 << 7);
                shot = Some(ModuleShot {
                    angles: [0x100, yaw_from(0x800, v.facing), 0],
                    tr: [0, 0x400, 0xA00],
                    focus: focus_on(v),
                    frames: 1,
                });
                // The cut to the victim hides the caster and shows the victim
                // by colour word as well as flag (the tick port's), and lets
                // the caster's clip-8 loop window go (`0x801F6F08..0x801F6F1C`:
                // `sw zero,0x4(s5)`, `sw 0x20080200,0x4(s6)`, `sb zero,0x21b(s5)`,
                // `sh zero,0x176(s5)`), so the clip runs on to its end and parks.
                arm.caster_write = CaptureSeatWrite {
                    colour: Some(0),
                    loop_release: true,
                    ..Default::default()
                };
                arm.victim_write = CaptureSeatWrite {
                    colour: Some(GUILTY_CROSS_NEUTRAL),
                    ..Default::default()
                };
            }
            3 => st.countdown.add(1 << 8),
            4 => {
                st.countdown.add(0xC0);
                shot = Some(behind_caster(1));
                // The cut back shows the caster again (`0x801F7320`).
                arm.caster_write = CaptureSeatWrite {
                    colour: Some(GUILTY_CROSS_NEUTRAL),
                    ..Default::default()
                };
            }
            5 => {
                // Every seat with HP back on the neutral word, flag `0` and
                // rate `8` (`0x801F73C4..0x801F7418`, seats `0..7`).
                arm.others_write = CaptureSeatWrite {
                    colour: Some(GUILTY_CROSS_NEUTRAL),
                    render_flag: Some(0),
                    rate: Some(8),
                    ..Default::default()
                };
                arm.others_live_only = true;
            }
            _ => {}
        }
    }
    CaptureCamArm {
        shot,
        drift,
        hold,
        next: None,
        ..arm
    }
}

/// The neutral colour word PROT 0944's arms store to show a seat.
const GUILTY_CROSS_NEUTRAL: u32 = 0x2008_0200;

/// PROT 0962's `0xA5` body (evolved Cort's Ultra Charge).
pub const ULTRA_CHARGE_BODY: u32 = 0x801F_69D8;
/// PROT 0962's `0xA5` countdown word (`lui 0x8020` / `-0x7654`).
pub const ULTRA_CHARGE_COUNTDOWN: u32 = 0x801F_89AC;

/// **Ultra Charge** (PROT 0962, body `0x801F69D8`, a `beq` chain over
/// `{0, 1, 0xFF}`):
///
/// - arm 0 (`0x801F6A78`) - a shot behind the caster: pitch `0x10`, yaw
///   `0x800 - caster[+0x46]`, TR `(0, h, 0xC00)` over `0xC` frames, `h` being
///   `0xC00` when the formation's first monster is `0xB5` and `0x240`
///   otherwise; the countdown `= scalar * 0x180`;
/// - arm 1 (`0x801F6C1C`) - TR z drifts out by `scalar * delta` a pass while
///   the countdown, drained by the same product, stays positive; then
///   `0xFF`.
///
/// The `cort_evolved_ultra_charge_mid_cast` capture is in arm 1, on the
/// `0xB5` framing (TR y `0xC00`), its TR z `2400` drifted past the shot's.
///
/// The body's phase chain is not ported elsewhere, so this director also
/// owns its phase ([`CaptureCamArm::next`]): `0 -> 1 -> 0xFF`, finishing on
/// `0xFF`.
///
/// PORT: FUN_801F69D8 (PROT 0962 `0xA5`; the camera arms, the countdown and
/// the phase chain)
pub fn ultra_charge_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x180;
            let h = if seats.first_monster == 0xB5 {
                0xC00
            } else {
                0x240
            };
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0x10, yaw_from(0x800, c.facing), 0],
                    tr: [0, h, 0xC00],
                    focus: focus_on(c),
                    frames: 0xC,
                }),
                drift: None,
                hold: false,
                next: Some(1),
                ..Default::default()
            }
        }
        1 => CaptureCamArm {
            shot: None,
            drift: Some(drift(0, 0, 0, super::MODULE_DRAIN_PER_TICK as i16)),
            hold: st.countdown.drain_above(0),
            next: Some(0xFF),
            ..Default::default()
        },
        _ => CaptureCamArm::default(),
    }
}

/// PROT 0961's body (Dead End Crisis `0xA1` / Final Crisis `0xB4`).
pub const DEAD_END_CRISIS_BODY: u32 = 0x801F_69D8;
/// PROT 0961's countdown word (`lui 0x8020` / `-0x7D1C`).
pub const DEAD_END_CRISIS_COUNTDOWN: u32 = 0x801F_82E4;

/// **Dead End Crisis / Final Crisis** (PROT 0961, body `0x801F69D8`, a
/// `beq` / `slti` chain over `0..=6` and `0xFF`). Every shot is a
/// `FUN_801D829C` cut (`a3 = 1`) focused on the caster, whose arm 0 has just
/// stood it at `(0, 0x400)` facing `0x800`; the depth is `ctx[+0x6D0]`. The
/// formation fork is the first monster id `0x8007BD0C == 0xB5`:
///
/// | arm | camera |
/// |---|---|
/// | 0 (`0x801F6AC0`) | cut: pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, h, depth)`, `h` `0x800` in `0xB5`, else `0x200` |
/// | 1 (`0x801F6C8C`) | drift: TR z `+s`, TR y `-s/4`, pitch `+s/8` (`s = scalar * delta`); passing, `0xB5`: pitch `-0x70`, yaw `0x800 - facing`, TR `(0, 0xA20, depth - 0x280)`; else pitch `-0x80`, yaw `0x980 - facing`, TR `(0x200, 0x300, depth)` |
/// | 2 (`0x801F6E78`) | drift in `0xB5` only: TR y `+delta`, pitch `+delta`, TR z `+s`; passing: pitch `-0x80`, yaw `0x800 - facing`, TR `(0, 0xA00, depth + 0x400)` in `0xB5`, else `(0, 0x600, depth)` |
/// | 3 (`0x801F7098`) | drift: TR z `+3s`, TR y `-s`, pitch `+s/8` |
/// | 5 (`0x801F7520`) | passing with a party seat standing: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x800, depth)` |
/// | 6 (`0x801F7780`) | drift: TR z `+s` |
/// | `0xFF` (`0x801F77D4`) | `ctx[+0x6DA] = 0x780`, `ctx[+0x0D] = 0` |
///
/// The drifts are stored before each gate tests the countdown, so they run
/// on every pass, held or not; `s / 4` and `s / 8` are `sra` of the product
/// with the round-toward-zero bias. The `cort_evolved_final_crisis_mid_cast`
/// capture sits in arm 1 of the `0xB5` path: pitch `16`, TR y `0x800 - 32`,
/// TR z `prescale(0x9F0) + 128` - sixteen vsyncs of drift past arm 0's cut.
///
/// The countdown gates are the body's table
/// ([`super::capture_countdown::DEAD_END_CRISIS`]), run here because a
/// director owns its body's gate; the phase chain is ported
/// ([`crate::cast_module_ticks::DEAD_END_CRISIS_CHAIN`]), so `next` stays
/// `None`. Arm 5's no-one-standing branch frames nothing and ends the
/// battle; the director cuts on every pass of it, which the battle end
/// overrides.
///
/// PORT: FUN_801F69D8 (PROT 0961; the camera arms)
pub fn dead_end_crisis_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let b5 = seats.first_monster == 0xB5;
    let depth = seats.depth_raw;
    // `scalar * delta` a battle frame; one vsync a tick here.
    let s = super::MODULE_DRAIN_PER_TICK as i16;
    let hold =
        super::capture_countdown::arm_countdown(super::capture_countdown::DEAD_END_CRISIS, phase)
            .is_some_and(|a| {
                let held = a.holds(&mut st.countdown);
                if !held {
                    a.pass(&mut st.countdown);
                }
                held
            });
    let cut = |pitch: i16, yaw: i32, tr: [i32; 3]| {
        Some(ModuleShot {
            angles: [pitch, yaw_from(yaw, c.facing), 0],
            tr: tr.map(|v| v as i16),
            focus: focus_on(c),
            frames: 1,
        })
    };
    let (drift, shot) = match phase {
        0 => (
            None,
            cut(0, 0x800, [0, if b5 { 0x800 } else { 0x200 }, depth]),
        ),
        1 => (
            Some(drift(s / 8, 0, -(s / 4), s)),
            if b5 {
                cut(-0x70, 0x800, [0, 0xA20, depth - 0x280])
            } else {
                cut(-0x80, 0x980, [0x200, 0x300, depth])
            },
        ),
        2 => (
            // `delta` alone on TR y and pitch (`move v1,v0` keeps the step).
            b5.then(|| drift(1, 0, 1, s)),
            if b5 {
                cut(-0x80, 0x800, [0, 0xA00, depth + 0x400])
            } else {
                cut(-0x80, 0x800, [0, 0x600, depth])
            },
        ),
        3 => (Some(drift(s / 8, 0, -s, 3 * s)), None),
        5 => (None, cut(0, 0x800, [0, 0x800, depth])),
        6 => (Some(drift(0, 0, 0, s)), None),
        DEAD_END_CRISIS_DONE => {
            st.yaw_base = 0x780;
            (None, None)
        }
        _ => (None, None),
    };
    // The party's rate: `2` as arm 3 lands the hit (`0x801F7418`), on every
    // pass of arm 4 (`li v0,2` in the dispatch's delay slot, `0x801F6AA0`), and
    // `2` again on every pass of arm 5, which reloads it (`li v0,0x2` at
    // `0x801F7524`) before its stores at `0x801F7528..0x801F7540`. The Done
    // band's restore `FUN_801E93C8` puts every seat back.
    let party_rate = match phase {
        3 if !hold => Some(2),
        4 | 5 => Some(2),
        _ => None,
    };
    CaptureCamArm {
        shot: if hold { None } else { shot },
        drift,
        hold,
        next: None,
        party_write: CaptureSeatWrite {
            rate: party_rate,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// PROT 0961's terminal arm.
const DEAD_END_CRISIS_DONE: u8 = 0xFF;

/// The arm whose pass lines the seats up ([`dead_end_crisis_line_up`]).
pub const DEAD_END_CRISIS_LINE_UP_ARM: u8 = 0;
/// The arm whose pass puts them back ([`dead_end_crisis_restore`]).
pub const DEAD_END_CRISIS_RESTORE_ARM: u8 = DEAD_END_CRISIS_DONE;
/// How many seats the line-up walks: `actor_table[0..4]`, the party row and
/// the first monster seat (`sltiu v0,s1,0x4` at `0x801F6BCC`).
pub const DEAD_END_CRISIS_LINE_UP_SEATS: usize = 4;

/// One seat's pose as PROT 0961 saves it: the live trio and the halfword after
/// it (`+0x34..+0x3B`, copied as two unaligned words) and the facing `+0x46`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineUpPose {
    /// `+0x34` x, `+0x36` y, `+0x38` z, `+0x3A`.
    pub pos: [i16; 4],
    /// `+0x46`.
    pub facing: u16,
}

/// PROT 0961's arm 0 line-up (`0x801F6ADC..0x801F6BD4`), on the arm's one
/// pass, ahead of its camera cut. For each of `actor_table[0..4]` it saves the
/// pose into the module's own words (`+0x34..+0x3B` to `0x801F82BC + s * 8`,
/// `+0x46` to `0x801F82DC + s * 2`) and then stands the seat:
///
/// * seats `0..3`: `z = -0x400`, `x = row_x[s] / 2` (`sra` with the
///   round-toward-zero bias at `0x801F6B70..0x801F6B84`), facing `0`. `row_x`
///   is the `x` of the one-monster row of the seat table at `0x80077628`
///   (stride `8`, `addiu a3,a3,0x8`);
/// * seat `3`, the first monster seat: `z = 0x400`, `x = 0`, facing `0x800`
///   (`0x801F6B98..0x801F6BB8`).
///
/// `y` and the display pair `+0x3C` / `+0x40` are left alone. The seats are
/// written whether or not they are seated. Returns the saved poses, which
/// [`dead_end_crisis_restore`] puts back. The `cort_evolved_final_crisis_mid_cast`
/// capture (arm 1) reads the party at `(0, -0x400)`, `(-300, -0x400)`,
/// `(300, -0x400)` facing `0` and the evolved Cort at `(0, 0x400)` facing
/// `0x800`, with the pre-cast poses in the saved words.
///
/// PORT: FUN_801F69D8 (PROT 0961; arm 0's line-up, `0x801F6ADC..0x801F6BD4`)
pub fn dead_end_crisis_line_up(
    seats: &mut [LineUpPose; DEAD_END_CRISIS_LINE_UP_SEATS],
    row_x: [i16; 3],
) -> [LineUpPose; DEAD_END_CRISIS_LINE_UP_SEATS] {
    let saved = *seats;
    for (s, p) in seats.iter_mut().enumerate() {
        if let Some(&x) = row_x.get(s) {
            p.pos[2] = -0x400;
            p.pos[0] = x / 2;
            p.facing = 0;
        } else {
            p.pos[2] = 0x400;
            p.pos[0] = 0;
            p.facing = 0x800;
        }
    }
    saved
}

/// PROT 0961's `0xFF` arm restore (`0x801F77D4..0x801F782C`): each of
/// `actor_table[0..4]` takes back the eight bytes `+0x34..+0x3B` and the
/// facing [`dead_end_crisis_line_up`] saved.
///
/// PORT: FUN_801F69D8 (PROT 0961; the `0xFF` arm's restore, `0x801F77D4..0x801F782C`)
pub fn dead_end_crisis_restore(
    seats: &mut [LineUpPose; DEAD_END_CRISIS_LINE_UP_SEATS],
    saved: &[LineUpPose; DEAD_END_CRISIS_LINE_UP_SEATS],
) {
    *seats = *saved;
}

/// PROT 0938's `0xB7` body (Cort's Mystic Circle).
pub const MYSTIC_CIRCLE_BODY: u32 = 0x801F_69EC;
/// PROT 0938's `0xB7` countdown word (`lui 0x8020` / `-0x7FC0`).
pub const MYSTIC_CIRCLE_COUNTDOWN: u32 = 0x801F_8040;

/// Cort's **Mystic Circle** (PROT 0938, body `0x801F69EC`, five arms off the
/// table at the image head). Unlike the other directed bodies its countdown
/// `0x801F8040` is absolute, not scalar-scaled: arm 0 stores `0x800`, every
/// later arm drains `8 * delta` (`8` a vsync) and holds while it stays
/// positive, and arms 1 / 2 / 3 re-arm `+0x400` / `+0x400` / `+0x600`.
///
/// | arm | camera |
/// |---|---|
/// | 0 (`0x801F6AA4`) | turns the caster to its target, then a cut: pitch `-0x40`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x600)`, focus the caster |
/// | 1, 2 (`0x801F6C00`, `0x801F6D88`) | drift: TR z `+4`, TR y `-1`; arm 2's exit cuts to pitch `0x180`, yaw `-caster[+0x46]`, TR `(0, 0x600, 0xC00)`; the caster's rate `+0x21D = 4` as arm 1 passes and through arm 2 |
/// | 3 (`0x801F6F3C`) | drift: TR z `+96`, TR y `-29` |
/// | 4 (`0x801F71F8`) | the last gate |
///
/// Arm 0's turn (toward the target, or the party's centre for the
/// all-party target `8`) is the cast's own; the director frames off the
/// caster's facing as the engine left it. The `cort_mystic_circle_mid_cast`
/// capture sits in arm 1 with `96` of the word left: `244` vsyncs of drift,
/// TR y `1292` of `0x600`.
///
/// PORT: FUN_801F69EC (PROT 0938 `0xB7`; the camera arms and the countdown)
pub fn mystic_circle_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    const DRAIN: i32 = 8;
    let c = seats.caster;
    if phase == 0 {
        // The caster first turns on its target - the party's centroid
        // (`FUN_801DCEAC`) for group code `8`, a seat's live pair otherwise
        // - plus a half turn (`0x801F6AAC..0x801F6B00`); the shot frames the
        // new heading.
        let v = seats.victim;
        let onto = match st.target_aim {
            Some(a) => bearing_12bit_approx(
                a.centroid_z.wrapping_neg(),
                a.centroid_x.wrapping_neg(),
                c.z,
                c.x,
            ),
            None => bearing_12bit_approx(v.z, v.x, c.z, c.x),
        }
        .wrapping_add(0x800)
            & 0xFFF;
        st.countdown.0 = 0x800;
        return CaptureCamArm {
            shot: Some(ModuleShot {
                angles: [-0x40, yaw_from(0x800, onto), 0],
                tr: [0, 0x600, 0x600],
                focus: focus_on(c),
                frames: 1,
            }),
            caster_write: CaptureSeatWrite {
                facing: Some(onto),
                ..Default::default()
            },
            ..Default::default()
        };
    }
    if !(1..=4).contains(&phase) {
        return CaptureCamArm::default();
    }
    let drift = match phase {
        1 | 2 => Some(drift(0, 0, -1, 4)),
        3 => Some(drift(0, 0, -29, 96)),
        _ => None,
    };
    let hold = st.countdown.holds_after_drain(DRAIN, 0);
    let mut shot = None;
    // The caster's anim rate `+0x21D = 4`: stored beside the clip `7` arm 1
    // stages as it passes (`0x801F6CC8..0x801F6CE4`) and again on every pass
    // of arm 2, ahead of its gate (`0x801F6D90..0x801F6D9C`), so the clip
    // plays at half speed through the circle's rise.
    let mut caster_write = CaptureSeatWrite::default();
    if phase == 2 || (phase == 1 && !hold) {
        caster_write.rate = Some(4);
    }
    if !hold {
        match phase {
            1 => st.countdown.0 += 0x400,
            2 => {
                st.countdown.0 += 0x400;
                shot = Some(ModuleShot {
                    angles: [0x180, yaw_from(0, c.facing), 0],
                    tr: [0, 0x600, 0xC00],
                    focus: focus_on(c),
                    frames: 1,
                });
            }
            3 => st.countdown.0 += 0x600,
            _ => {}
        }
    }
    CaptureCamArm {
        shot,
        drift,
        hold,
        next: None,
        caster_write,
        ..Default::default()
    }
}

/// PROT 0946's Big Wave stage store: `sb v0,0x278(a0)` with `v0 = 2` at
/// `0x801F6BF8`, on arm 0's branch taken while the per-seat word
/// `0x801CEFE0 + (seat+1)*4` is set (`beq v0,zero,0x801F6C38` at
/// `0x801F6B00` jumps to Call Wave's path instead). The phase walk places it
/// on arm 0 whichever branch runs; [`wave_camera`] keys the branch on the
/// action id, and so does this.
pub const WAVE_BIG_STAGE_STORE: u32 = 0x801F_6BF8;

/// Whether a stage-byte store the presentation decode placed on a phase arm
/// runs for `action`: false only for a store on a choreography branch the
/// action does not take ([`WAVE_BIG_STAGE_STORE`], Big Wave's alone).
pub fn stage_store_runs(entry: u32, va: u32, action: Option<u8>) -> bool {
    !(entry == 946
        && (va == WAVE_STAGE_RESTORE || (va == WAVE_BIG_STAGE_STORE && action != Some(0x56))))
}

/// PROT 0946's stage restore: `sb zero,0x278(v0)` at `0x801F7638`, behind
/// `s3`, which only arm 8 raises and only while every party seat has settled
/// (`0x801F7510..0x801F758C`). It is the arm's exit pass, not a store on the
/// arm's own entry: the backdrop stays off through the party's get-up.
/// [`wave_camera`] makes it.
pub const WAVE_STAGE_RESTORE: u32 = 0x801F_7638;

/// PROT 0946's countdown word (`lui 0x801F` / `+0x7F20`).
pub const WAVE_COUNTDOWN: u32 = 0x801F_7F20;

/// A Big Wave victim's animation rate: arm 7 halves `+0x21D` (`srl 1` at
/// `0x801F7418..0x801F7424`) off the seat's full `8`.
const WAVE_HIT_RATE: u8 = 4;

/// Zeto's **Call Wave** (`0x55`) and **Big Wave** (`0x56`), PROT 0946's one
/// tick body `0x801F69FC` (nine arms off the image-head table). Retail picks
/// the choreography at arm 0 off a per-seat word (`0x801CEFE0 + (seat+1)*4`)
/// it toggles on every cast; the two library captures pin which branch each
/// spell takes, so the port keys on the action id:
///
/// | arm | Call Wave | Big Wave |
/// |---|---|---|
/// | 0 (`0x801F6AD8`) | cut: pitch `0x100`, TR `(0, 0x600, 2z)`; to arm 1 | cut: pitch `0x20`, TR `(0, 0x600, z/2)`; to arm 4 |
/// | 1 (`0x801F6D0C`) | pan: pitch `0`, TR `(0, 0x600, z/2)` over `0x100` frames; countdown `+= scalar << 8` | - |
/// | 2 (`0x801F6D9C`) | gate; then pitch `0`, TR `(0, 0x600, z)` over `0x40`; `+= scalar * 0x60` | - |
/// | 3 (`0x801F6E48`) | gate; the body finishes | - |
/// | 4 (`0x801F6E90`) | - | pan: pitch `0x20`, TR `(0, 0x600, 2z)` over `0x100`; `+= scalar << 6` |
/// | 5 (`0x801F6F30`) | - | gate; the wave's effects; `+= scalar * 0xC0` (`3 << 6`); to arm 6 |
/// | 6 (`0x801F7104`) | - | gate; cut: pitch `0x180`, yaw `0xF00 - caster[+0x46]`, TR `(0, 0x600, 3z/2)`; `+= scalar * 0x60`; to arm 7 |
/// | 7 (`0x801F72DC`) | - | yaw `+= delta * scalar / 4` every pass; gate; the party's hits; `+= scalar * 0xA0`; to arm 8 |
/// | 8 (`0x801F74E4`) | - | the same yaw spin; gate; the body finishes |
///
/// Arms 0..5's shots are behind the caster (yaw `0x800 - caster[+0x46]`,
/// focus the caster) and `z` is `ctx[+0x6D0]`. The body has no other port, so
/// this director owns its phase and finishes the module at arm 8, whose exit
/// also waits on every party member's clip. The `zeto_call_wave_mid_cast` capture is in arm 2 on arm 1's
/// pan, `zeto_big_wave_mid_cast` in arm 5 on arm 4's.
///
/// PORT: FUN_801F69FC, overlay_cast_call_wave_0946_801f69fc (PROT 0946; the camera arms 0..8, their countdown and
/// phase chain)
pub fn wave_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let c = seats.caster;
    let z = seats.depth_raw;
    let shot = |pitch: i16, tr_z: i32, frames: u16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x600, tr_z as i16],
        focus: focus_on(c),
        frames,
    };
    let pass = |shot: Option<ModuleShot>, next: Option<u8>| CaptureCamArm {
        shot,
        drift: None,
        hold: false,
        next,
        ..Default::default()
    };
    let gate = |st: &mut ModuleCamState| st.countdown.drain_above(0);
    let held = CaptureCamArm {
        hold: true,
        ..Default::default()
    };
    let tint = |flag: u8| CaptureSeatWrite {
        render_flag: Some(flag),
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = 0;
            if seats.action == 0x56 {
                pass(Some(shot(0x20, z / 2, 1)), Some(4))
            } else {
                pass(Some(shot(0x100, z * 2, 1)), Some(1))
            }
        }
        1 => {
            st.countdown.add(1 << 8);
            // Call Wave's charge tint: the caster's `+0x21C = 7`, the soft
            // red arm of the tint SM (`li v0,0x7` / `sb v0,0x21c(s4)` at
            // `0x801F6D8C..0x801F6D98`, `s4` the caster).
            CaptureCamArm {
                caster_write: tint(7),
                ..pass(Some(shot(0, z / 2, 0x100)), Some(2))
            }
        }
        2 => {
            if gate(st) {
                return held;
            }
            st.countdown.add(0x60);
            // The tint off once the pan's word is spent (`sb zero,0x21c(s4)`
            // at `0x801F6E44`), so arm 0 of the tint SM eases the body home
            // through arm 3.
            CaptureCamArm {
                caster_write: tint(0),
                ..pass(Some(shot(0, z, 0x40)), Some(3))
            }
        }
        3 => {
            if gate(st) {
                return held;
            }
            // The same clear again on the way out (`0x801F6E7C`).
            CaptureCamArm {
                caster_write: tint(0),
                ..pass(None, None)
            }
        }
        4 => {
            st.countdown.add(1 << 6);
            pass(Some(shot(0x20, z * 2, 0x100)), Some(5))
        }
        5 => {
            if gate(st) {
                return held;
            }
            // `3 * scalar << 6` (`0x801F6F78..0x801F6F94`).
            st.countdown.add(0xC0);
            pass(None, Some(6))
        }
        6 => {
            if gate(st) {
                return held;
            }
            // `3 * scalar << 5` (`0x801F71C8..0x801F71EC`), then the cut at
            // `0x801F7258..0x801F72CC`: `TR.z = 3 * ctx[+0x6D0] / 2`.
            st.countdown.add(0x60);
            let s = ModuleShot {
                angles: [0x180, yaw_from(0xF00, c.facing), 0],
                tr: [0, 0x600, (z * 3 / 2) as i16],
                focus: focus_on(c),
                frames: 1,
            };
            pass(Some(s), Some(7))
        }
        7 => {
            // `_DAT_8007B792 += (delta * scalar) / 4` ahead of the gate
            // (`0x801F7318..0x801F7368`): per vsync, a quarter of the scalar.
            let spin = Some(drift(0, (super::SPEED_SCALAR / 4) as i16, 0, 0));
            if gate(st) {
                return CaptureCamArm {
                    drift: spin,
                    ..held
                };
            }
            // The way out of the gate is the party's hits
            // (`0x801F7398..0x801F74A8`): `FUN_801DD4B0(0x120, ctx[+0x13], seat)`
            // for each seat with HP and without Stone, whose rate halves and
            // whose heading turns to `caster[+0x46] + 0x800`. Then
            // `5 * scalar << 5` (`0x801F74AC..0x801F74CC`).
            st.countdown.add(0xA0);
            CaptureCamArm {
                drift: spin,
                folds_hit: true,
                party_write: CaptureSeatWrite {
                    rate: Some(WAVE_HIT_RATE),
                    facing: Some(c.facing.wrapping_add(0x800) & 0xFFF),
                    ..Default::default()
                },
                party_live_only: true,
                party_skip_stoned: true,
                ..pass(None, Some(8))
            }
        }
        8 => {
            // The same spin (`0x801F74E4..0x801F7528`). The body returns `0`
            // once the word is spent and every party seat has settled
            // (`0x801F7534..0x801F75A0`: `+0x1D9 == 0`, a downed one `8`).
            let spin = Some(drift(0, (super::SPEED_SCALAR / 4) as i16, 0, 0));
            if (st.countdown.0 > 0 && gate(st)) || !st.party_settled {
                return CaptureCamArm {
                    drift: spin,
                    ..held
                };
            }
            CaptureCamArm {
                drift: spin,
                ctx_278: Some(0),
                ..pass(None, None)
            }
        }
        _ => pass(None, None),
    }
}

/// PROT 0940's `0x3C` body (Glare), the trampoline's first arm.
pub const GLARE_BODY: u32 = 0x801F_69F8;

/// PROT 0940's split (`0x801F78B8`, actions `0x50` Slime Divide and `0xAE`):
/// arm 0's shot on the caster at `0xC` frames (`0x801F7A30`), then arm 1's
/// lift at `0x100` (`0x801F7B34`) once its gate passes. Arms 2 and 3 frame
/// nothing; each arm's countdown is `capture_countdown::GLARE_DIVIDE_SPLIT`'s.
///
/// | arm | pitch | TR, `0x50` | TR, `0xAE` | frames |
/// |---|---|---|---|---|
/// | 0 | `0` | `(0, 0x300, 0x600)` | `(0, 0x400, 0x800)` | `0xC` |
/// | 1 | `0x100` | `(0, 0x300, 0x1000)` | `(0, 0x400, 0x1000)` | `0x100` |
///
/// PORT: overlay_cast_glare_divide_0940_801f78b8 (PROT 0940 `0x50` / `0xAE`; the camera arms)
pub fn slime_divide_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let Some(gate) = super::arm_countdown(super::capture_countdown::GLARE_DIVIDE_SPLIT, phase)
    else {
        return CaptureCamArm::default();
    };
    if gate.holds(&mut st.countdown) {
        return CaptureCamArm {
            hold: true,
            ..Default::default()
        };
    }
    gate.pass(&mut st.countdown);
    let y: i16 = if seats.action == 0x50 { 0x300 } else { 0x400 };
    let (pitch, z, frames): (i16, i16, u16) = match phase {
        0 if seats.action == 0x50 => (0, 0x600, 0xC),
        0 => (0, 0x800, 0xC),
        1 => (0x100, 0x1000, 0x100),
        _ => return CaptureCamArm::default(),
    };
    CaptureCamArm {
        shot: Some(ModuleShot {
            angles: [pitch, yaw_from(0x800, seats.caster.facing), 0],
            tr: [0, y, z],
            focus: focus_on(seats.caster),
            frames,
        }),
        ..Default::default()
    }
}

/// **Glare** (PROT 0940, body `0x801F69F8`, a `beq` chain over `0..=3`; its
/// countdown is the module word `0x801F864C` Mystic Shield also uses):
///
/// | arm | camera | countdown as it passes |
/// |---|---|---|
/// | 0 (`0x801F6ACC`) | shot behind the caster: pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, 0x800)` for monster `0xA9` else `(0, 0x400, 0xA00)`, focus the caster, `0xC` frames | `= scalar << 8` |
/// | 1 (`0x801F6C7C`) | drift TR z `-8`, TR y `+2` a pass; once spent, a cut onto the victim: pitch `0`, yaw `0x800 - victim[+0x46]`, TR `(0, 0x400, 0xA00)` | `+= scalar << 7` |
/// | 2 (`0x801F6EEC`) | drift TR z `-8` | `+= scalar << 7` |
/// | 3 (`0x801F71B4`) | drift TR z `-8`; the last gate - the body returns `0` | - |
///
/// Each arm from 1 on drains the word by `scalar * delta` and holds while it
/// stays positive (`bgtz` to the epilogue with the busy `1`). The body has no
/// damage site: arm 2 raises the element `0x5B` and sets `ctx[+0x18]`, the
/// status half the band's fold owns, which the camera does not touch. The
/// body's phase chain has no other port, so this director owns the phase.
///
/// PORT: overlay_cast_glare_divide_0940_801f69f8 (PROT 0940 `0x3C`; the camera arms, the countdown and the phase chain)
pub fn glare_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let pull = drift(0, 0, 0, -8);
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR << 8;
            let tr = if seats.caster_monster == 0xA9 {
                [0, 0x600, 0x800]
            } else {
                [0, 0x400, 0xA00]
            };
            CaptureCamArm {
                shot: Some(ModuleShot {
                    angles: [0, yaw_from(0x800, seats.caster.facing), 0],
                    tr,
                    focus: focus_on(seats.caster),
                    frames: 0xC,
                }),
                drift: None,
                hold: false,
                next: Some(1),
                ..Default::default()
            }
        }
        1..=3 => {
            let d = if phase == 1 { drift(0, 0, 2, -8) } else { pull };
            let hold = st.countdown.drain_above(0);
            if hold {
                return CaptureCamArm {
                    shot: None,
                    drift: Some(d),
                    hold: true,
                    next: Some(phase),
                    ..Default::default()
                };
            }
            if phase < 3 {
                st.countdown.add(1 << 7);
            }
            let shot = (phase == 1).then(|| ModuleShot {
                angles: [0, yaw_from(0x800, seats.victim.facing), 0],
                tr: [0, 0x400, 0xA00],
                focus: focus_on(seats.victim),
                frames: 1,
            });
            CaptureCamArm {
                shot,
                drift: Some(d),
                hold: false,
                next: (phase < 3).then_some(phase + 1),
                ..Default::default()
            }
        }
        _ => CaptureCamArm::default(),
    }
}

/// PROT 0953's punch-branch records, seated at the world origin on zero
/// angles (`a0 = sp+0x20`, `a1 = sp+0x10`, both zeroed in the prologue;
/// `0x801F6B0C..0x801F6BB0`).
pub const TERIO_PUNCH_RECORDS: [u32; 7] = [
    0x801F_7BD0,
    0x801F_7C40,
    0x801F_7CB0,
    0x801F_7A3C,
    0x801F_7B48,
    0x801F_799C,
    0x801F_7A0C,
];
/// PROT 0953's charge-branch records, at the origin the same way
/// (`0x801F6CEC..0x801F6D48`).
pub const TERIO_CHARGE_RECORDS: [u32; 4] = [0x801F_7854, 0x801F_77C8, 0x801F_7838, 0x801F_78CC];
/// The trail record arm 6 seats twice a battle frame, held or not
/// (`0x801F711C..0x801F71F4`), each handle kept in the list at `0x801F7E98`.
pub const TERIO_TRAIL_RECORD: u32 = 0x801F_7AF8;
/// The record arm 6's passing pass seats ahead of the caster
/// (`0x801F72BC..0x801F7368`).
pub const TERIO_FIST_RECORD: u32 = 0x801F_7D20;

fn at_origin(record: u32) -> Option<super::ModuleSpawn> {
    Some(super::ModuleSpawn {
        record: super::SpawnRecord::Module(record),
        anchor: super::SpawnAnchor::Literal {
            pos: [0; 3],
            rot: [0; 3],
        },
    })
}

/// `caster + trunc(table * 620 / 4096)` on one axis: the arm's
/// `* 5 * 31 * 4` then `addiu 0xFFF` / `sra 0xC` for a negative product.
fn terio_reach(base: i16, table: i16) -> i16 {
    let p = i32::from(table) * 620;
    (i32::from(base) + p / 4096) as i16
}

/// PROT 0953 (Terio Punch), a single-body module: its `0x801CF56C` arm
/// points straight at the tick `0x801F69FC`, a nine-arm switch (`sltiu 9`,
/// table at the image head).
///
/// The body is **two casts in one**, forked at arm 0 on the caster's latch
/// word `0x801C8FE0 + (ctx[+0x13] + 1) * 4` - the word the monster AI
/// reads for monster `0x8B` (it picks `0x5D` when it is set, else `0x5E`):
///
/// * latch clear - the **charge** (`0x801F6CEC`): a cut behind the caster -
///   pitch `0`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x600, depth / 2)` -
///   the latch set, and phase 1; arm 1 (`0x801F6E74`) pulls out to pitch
///   `0x200`, TR `(0, 0, depth * 2)` over `0x100` frames and arms
///   `scalar << 8` on the module word `0x801F7E88`; arm 2 holds on it and
///   the body returns `0`. No damage site runs, so the fold owes nothing.
/// * latch set - the **punch** (`0x801F6B0C`): the caster turned to face
///   its victim (`h = heading(victim -> caster) + 0x800`), a cut - pitch
///   `0x20`, yaw `0x400 - h`, TR `(0, 0x600, depth)` - the latch cleared, and
///   phase 4. Arm 4 (`0x801F6FA0`): pitch `-0x40`, yaw `0x600 - h`, TR
///   `(0x200, 0xA00, depth / 6)` over `0x50` frames, `+= scalar * 80`. Arm 5:
///   held, then pitch `0`, yaw `0x800 - h`, TR `(0, 0x550, depth)` over
///   `0x40`, `+= scalar << 6`. Arm 6 drains every pass (spawning the trail
///   parts) and, once spent, pulls out to TR `(0, 0x550, 0x2800)` over
///   `0x80`, `+= scalar * 96`. Arm 7: held, then the **strike** -
///   `FUN_801DD6B4(0x274, caster, seat)` over every hittable party seat
///   (`0x801F7410`), which is the band's fold here - and `+= scalar * 160`.
///   Arm 8 holds out the word and the body returns `0`.
///
/// `depth` is `ctx[+0x6D0]`. Every framing's focus is the caster. Arm 8 also
/// waits for every party seat to leave its reaction clip, which this
/// director does not model.
///
/// PORT: overlay_cast_terio_punch_0953_801f69fc (PROT 0953; the camera arms, the countdown, the charge latch and the phase chain)
pub fn terio_punch_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let depth = seats.depth_raw as i16;
    let h = super::heading(seats.victim, c).wrapping_add(0x800) & 0xFFF;
    let shot = |angles: [i16; 3], tr: [i16; 3], frames: u16| {
        Some(ModuleShot {
            angles,
            tr,
            focus: focus_on(c),
            frames,
        })
    };
    let held = CaptureCamArm {
        hold: true,
        next: Some(phase),
        ..Default::default()
    };
    let step = |shot: Option<ModuleShot>, next: Option<u8>| CaptureCamArm {
        shot,
        next,
        ..Default::default()
    };
    match phase {
        0 if seats.caster_latch != 0 => {
            st.countdown.0 = 0;
            // The trail list's count is cleared (`sw zero,-0x7F68` and the
            // 128-word list at `0x801F7E98`, `0x801F6CB8..0x801F6CCC`).
            st.pass_count = 0;
            let mut charge_spawns = [None; 8];
            for (o, r) in charge_spawns.iter_mut().zip(TERIO_PUNCH_RECORDS) {
                *o = at_origin(r);
            }
            CaptureCamArm {
                latch: Some(0),
                charge_spawns,
                // `li 3; sb 0x278` and `li 1; sb 0x21d(s4)` at `0x801F6C94`. A
                // recording's cursor advances one a vsync under it, though the
                // byte itself reads `4` there.
                ctx_278: Some(3),
                caster_write: CaptureSeatWrite {
                    rate: Some(1),
                    facing: Some(h),
                    ..Default::default()
                },
                ..step(
                    shot([0x20, yaw_from(0x400, h), 0], [0, 0x600, depth], 1),
                    Some(4),
                )
            }
        }
        0 => CaptureCamArm {
            latch: Some(1),
            skips_fold: true,
            ctx_278: Some(3),
            pass_spawns: [
                at_origin(TERIO_CHARGE_RECORDS[0]),
                at_origin(TERIO_CHARGE_RECORDS[1]),
                at_origin(TERIO_CHARGE_RECORDS[2]),
                at_origin(TERIO_CHARGE_RECORDS[3]),
                None,
            ],
            // Every party seat hidden, no HP test (`0x801F6E14..0x801F6E54`).
            party_write: super::v_windhash::HIDDEN,
            ..step(
                shot([0, yaw_from(0x800, c.facing), 0], [0, 0x600, depth / 2], 1),
                Some(1),
            )
        },
        1 => {
            st.countdown.add(1 << 8);
            CaptureCamArm {
                // The caster's tint state `7` (`li v0,0x7` / `sb v0,0x21c(s4)`
                // at `0x801F6EE8`).
                caster_write: CaptureSeatWrite {
                    render_flag: Some(7),
                    ..Default::default()
                },
                ..step(
                    shot(
                        [0x200, yaw_from(0x800, c.facing), 0],
                        [0, 0, depth.wrapping_mul(2)],
                        0x100,
                    ),
                    Some(2),
                )
            }
        }
        2 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            // The tint off and every party seat shown again
            // (`0x801F6F2C..0x801F6F94`).
            CaptureCamArm {
                caster_write: CaptureSeatWrite {
                    render_flag: Some(0),
                    ..Default::default()
                },
                party_write: super::v_windhash::SHOWN,
                ..step(None, None)
            }
        }
        4 => {
            st.countdown.add(80);
            step(
                shot(
                    [-0x40, yaw_from(0x600, h), 0],
                    [0x200, 0xA00, depth / 6],
                    0x50,
                ),
                Some(5),
            )
        }
        5 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            st.countdown.add(1 << 6);
            step(
                shot([0, yaw_from(0x800, h), 0], [0, 0x550, depth], 0x40),
                Some(6),
            )
        }
        6 => {
            // Two trail records a battle frame, held or not, each on three
            // draws (the host's, `CaptureCamArm::trail`).
            let trail = st.opens_pass.then(|| {
                let first = st.pass_count as u16;
                st.pass_count = st.pass_count.wrapping_add(2);
                (TERIO_TRAIL_RECORD, first)
            });
            if st.countdown.drain_above(0) {
                return CaptureCamArm { trail, ..held };
            }
            st.countdown.add(96);
            // The fist ahead of the caster: `620` along its heading's
            // `(sin, cos)`, height `-0x154`, on its angles.
            let (sn, cs) = legaia_engine_system::trig::trig12(c.facing & 0xFFF);
            let mut pass_spawns = [None; 5];
            pass_spawns[0] = Some(super::ModuleSpawn {
                record: super::SpawnRecord::Module(TERIO_FIST_RECORD),
                anchor: super::SpawnAnchor::Literal {
                    pos: [terio_reach(c.x, sn), -0x154, terio_reach(c.z, cs)],
                    rot: [0, c.facing as i16, 0],
                },
            });
            CaptureCamArm {
                trail,
                pass_spawns,
                ..step(
                    shot([0, yaw_from(0x800, h), 0], [0, 0x550, 0x2800], 0x80),
                    Some(7),
                )
            }
        }
        7 => {
            if st.countdown.drain_above(0) {
                return held;
            }
            st.countdown.add(160);
            // The way out of the gate is the party's hits
            // (`0x801F73CC..0x801F74DC`): `FUN_801DD6B4(0x274, ctx[+0x13], seat)`
            // for each seat with HP and without Stone, whose rate halves and
            // whose heading turns to `caster[+0x46] + 0x800`. The caster's
            // clip park is released on the way (`0x801F73AC..0x801F73B8`).
            CaptureCamArm {
                folds_hit: true,
                party_write: CaptureSeatWrite {
                    rate: Some(WAVE_HIT_RATE),
                    facing: Some(c.facing.wrapping_add(0x800) & 0xFFF),
                    ..Default::default()
                },
                party_live_only: true,
                party_skip_stoned: true,
                caster_write: CaptureSeatWrite {
                    stage: Some(0),
                    loop_release: true,
                    ..Default::default()
                },
                ..step(None, Some(8))
            }
        }
        8 => {
            // The body returns `0` once the word is spent and every party
            // seat has settled (`0x801F7518..0x801F75EC`: `+0x1D9 == 0`, a
            // downed one `8`).
            if (st.countdown.0 > 0 && st.countdown.drain_above(0)) || !st.party_settled {
                return held;
            }
            CaptureCamArm {
                ctx_278: Some(0),
                ..step(None, None)
            }
        }
        _ => CaptureCamArm::default(),
    }
}

#[cfg(test)]
mod slime_divide_tests {
    use super::*;

    /// The arm-0 shot `battle_map01_green_slime_divide_mid_cast`'s table reads
    /// one pass in (pitch `0`, TR `(0, 768, 2457)` once prescaled, on the
    /// caster), and arm 1 on the gate the same state's word `96` stands on.
    #[test]
    fn slime_divide_frames_its_arms_on_the_body_gates() {
        let seats = ModuleCamSeats {
            action: 0x50,
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        let arm0 = slime_divide_camera(&mut st, 0, seats).shot.unwrap();
        assert_eq!(arm0.tr, [0, 0x300, 0x600]);
        assert_eq!(arm0.frames, 0xC);
        assert_eq!(st.countdown.0, SPEED_SCALAR * 12);
        let held = slime_divide_camera(&mut st, 1, seats);
        assert!(held.hold && held.shot.is_none());
        st.countdown.0 = 0;
        let arm1 = slime_divide_camera(&mut st, 1, seats).shot.unwrap();
        assert_eq!(arm1.angles[0], 0x100);
        assert_eq!(arm1.tr, [0, 0x300, 0x1000]);
        // The other action of the same body stands higher and further out.
        let seats = ModuleCamSeats {
            action: 0xAE,
            ..seats
        };
        let mut st = ModuleCamState::default();
        let arm0 = slime_divide_camera(&mut st, 0, seats).shot.unwrap();
        assert_eq!(arm0.tr, [0, 0x400, 0x800]);
    }
}

#[cfg(test)]
mod exit_style_tests {
    use super::capture_exit_clears_style;

    /// A body with no `sb zero,0xd` leaves state `0x70`'s style `1` standing
    /// and the Done band frames half a turn round; the other body of the same
    /// module clears it.
    #[test]
    fn a_split_module_keys_the_style_clear_on_the_body_its_action_runs() {
        assert!(!capture_exit_clears_style(945, 0x54), "Water Column");
        assert!(capture_exit_clears_style(945, 0xBA), "all-stats surge");
        assert!(!capture_exit_clears_style(938, 0x4E), "Chaos Breath");
        assert!(capture_exit_clears_style(938, 0xB7), "Mystic Circle");
        assert!(!capture_exit_clears_style(940, 0x50), "Slime Divide");
        assert!(!capture_exit_clears_style(940, 0xAE), "Slime Divide");
        assert!(capture_exit_clears_style(940, 0x3C), "Glare");
        assert!(!capture_exit_clears_style(941, 0x51), "Steal");
        assert!(capture_exit_clears_style(941, 0xB9), "Stone Circle");
    }
}

#[cfg(test)]
mod terio_punch_tests {
    use super::super::SpawnRecord;
    use super::*;

    fn seats(latch: i32) -> ModuleCamSeats {
        ModuleCamSeats {
            caster: super::super::ModuleSeat {
                x: 0,
                y: 0,
                z: 800,
                facing: 0,
            },
            victim: super::super::ModuleSeat {
                x: 0,
                y: 0,
                z: -800,
                facing: 0x800,
            },
            caster_latch: latch,
            depth_raw: 3008,
            ..Default::default()
        }
    }

    /// The punch's arm 0 seats its seven records at the origin; the charge's
    /// seats four and hides the party, and its arm 2 shows it again.
    #[test]
    fn each_branch_seats_its_own_records() {
        let mut st = ModuleCamState::default();
        let punch = terio_punch_camera(&mut st, 0, seats(1));
        let recs: Vec<_> = punch
            .charge_spawns
            .iter()
            .flatten()
            .map(|s| s.record)
            .collect();
        assert_eq!(recs, TERIO_PUNCH_RECORDS.map(SpawnRecord::Module).to_vec());
        let mut st = ModuleCamState::default();
        let charge = terio_punch_camera(&mut st, 0, seats(0));
        assert_eq!(charge.pass_spawns.iter().flatten().count(), 4);
        assert_eq!(charge.party_write.render_flag, Some(0xFF));
        st.countdown.0 = 1;
        let shown = terio_punch_camera(&mut st, 2, seats(0));
        assert_eq!(shown.party_write.render_flag, Some(0));
    }

    /// Arm 6 asks for two trail records on every pass that opens a battle
    /// frame, their indices running on, and seats the fist on its pass.
    #[test]
    fn arm_6_trails_every_frame_and_seats_the_fist_on_its_pass() {
        let mut st = ModuleCamState {
            opens_pass: true,
            ..Default::default()
        };
        st.countdown.0 = SPEED_SCALAR * 2;
        let a = terio_punch_camera(&mut st, 6, seats(1));
        assert!(a.hold);
        assert_eq!(a.trail, Some((TERIO_TRAIL_RECORD, 0)));
        let b = terio_punch_camera(&mut st, 6, seats(1));
        assert!(!b.hold);
        assert_eq!(b.trail, Some((TERIO_TRAIL_RECORD, 2)));
        assert_eq!(
            b.pass_spawns[0].map(|s| s.record),
            Some(SpawnRecord::Module(TERIO_FIST_RECORD))
        );
    }
}
