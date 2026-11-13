//! The op-`0x20` stagers of the capture-class cells whose parts call back
//! into their module: PROT 0938 (Chaos Breath / Mystic Circle), PROT 0944
//! (Guilty Cross / Curse All), PROT 0946 (Zeto's Call Wave / Big Wave), PROT
//! 0959 (Che Delilas's Megaton Press), PROT 0960 (Lu Delilas's Plasma Strike)
//! and
//! PROT 0962 (the Blade Breath cell, whose
//! `0xA5` body is Cort's Ultra Charge). Each seats more records off the
//! calling part, the shape [`super::stager_spawns`] documents for the summon
//! band.

use super::stager_spawns::{StagerCaller, StagerSpawn};

fn on_caller(record: u32, caller: StagerCaller, scale: u16) -> StagerSpawn {
    StagerSpawn {
        record,
        pos: caller.pos,
        rot: caller.rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale,
    }
}

/// PROT 0938's stager `FUN_801F7AB8`, reached through `gp[+0x714]` from its
/// parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7BE0` on the caller (`FUN_80021B04`, `0x801F7B1C`) |
/// | 1 | eight `0x801F7C60` on the caller's position, each turned a further `0x200` about Y from the caller's `+0x26` (`0x801F7B2C..0x801F7B74`) |
/// | 2 | `0x801F7DF4` on the caller (`FUN_80050ED4`, `0x801F7B80`): Mystic Circle's rays, called every eleven frames by `0x801F7DC4` |
/// | 3 | `0x801F7F28` on the caller |
///
/// Every arm forwards the caller's `+0x72` as the seat's scale (`lhu` /
/// `lh a3,0x72(s1)`).
///
/// PORT: FUN_801F7AB8 (PROT 0938 stager)
pub fn chaos_breath_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let scale = caller.scale;
    match arm {
        0 => vec![on_caller(0x801F_7BE0, caller, scale)],
        1 => (1..=8u16)
            .map(|k| {
                let mut s = on_caller(0x801F_7C60, caller, scale);
                s.rot = [
                    0,
                    (caller.rot[1] as u16).wrapping_add(k * 0x200) as i16 & 0xFFF,
                    0,
                ];
                s
            })
            .collect(),
        2 => vec![on_caller(0x801F_7DF4, caller, scale)],
        3 => vec![on_caller(0x801F_7F28, caller, scale)],
        _ => Vec::new(),
    }
}

/// The stagers of PROT 0935, 0936, 0937, 0941, 0942, 0943, 0945, 0951, 0953, 0955, 0956 and 0964 (`gp[+0x714]` off the
/// 64-word table at `0x801F6734`), reached from their parts' op `0x20`:
///
/// | PROT | stager | arm: seats, on the caller |
/// |---|---|---|
/// | 0935 | `FUN_801F7FF0` | 0: `0x801F81EC` |
/// | 0936 | `FUN_801F7BD0` | 0: `0x801F7DE4`, then eight `0x801F7D84` on two draws each; 1: `0x801F8094`, `0x801F810C` |
/// | 0937 | `FUN_801F7850` | 0: `0x801F7AF4`, then eight `0x801F7A94` on two draws each; 1: `0x801F7D00`; 2: `0x801F7AF4`, then eight `0x801F7DB0` on three draws each |
/// | 0941 | `FUN_801F7DB0` | 1: `0x801F7F98`, 2: `0x801F8184`, 3: `0x801F81C8`, 4: `0x801F8238` |
/// | 0942 | `FUN_801F8118` | 0: `0x801F81D0`, 1: `0x801F8480`, 2: `0x801F84F8`, each at the caller's scale |
/// | 0943 | `FUN_801F769C` | 0: `0x801F7788`; 1: `0x801F784C`, `0x801F78A8`; 2: `0x801F791C`, each at the caller's scale |
/// | 0945 | `FUN_801F776C` | 0, 1: `0x801F7878` at the caller's scale; 2: `0x801F7CF0`; 3: `0x801F7DF0` |
/// | 0951 | `FUN_801F81DC` | 0: `0x801F82EC`; 1: that and `0x801F83B0`; 2: `0x801F84EC`; 3: `0x801F87DC`, `0x801F8908` (2 and 3 at the caller's scale) |
/// | 0953 | `FUN_801F7624` | 0: `0x801F78FC`; 1: `0x801F794C`; 2: three of `0x801F7D70`, `0x801F7DB4`, `0x801F7E08`, each triple turned `0x80 - rand() % 257` off the caller's yaw with `+0x3E = 16 - rand() % 33` |
/// | 0955 | `FUN_801F9370` | 0: `0x801F93EC` (White Shield's shield, called by arm 0's `0x801F93D4`); 1: `0x801F9744` (Power Charge's aura, called by arm 0's `0x801F972C`); both on the caller's `+0x14` / `+0x24` at its `+0x72` |
/// | 0956 | `FUN_801F7EC4` | 0: `0x801F80B4`; 1: `0x801F8290`, `0x801F82DC`, `0x801F8330`, `0x801F84C4`, at the caller's scale; 2: `0x801F850C`, the same |
/// | 0964 | `FUN_801F8BF8` | 0: `0x801F8EDC` / `8F38` / `8F94` by the last roll; 1: four `0x801F9008`, each on six draws; 2: `0x801F9568` (all at the caller's scale) |
///
/// PROT 0941's arm 1 also spawns the 2D puff `FUN_801DFDF0(3, ..)`
/// ([`super::stager_effect`]). Not ported: its arm 0, `sh zero,0x42` on the
/// caller.
///
/// PORT: overlay_cast_earthquake_0935_801f7ff0, overlay_cast_steal_0941_801f7db0, overlay_cast_power_up_0942_801f8118, overlay_cast_chaos_flare_0951_801f81dc, overlay_cast_element_change_0964_801f8bf8
/// PORT: FUN_801F769C (PROT 0943's stager)
/// PORT: overlay_cast_hyper_crush_0936_801f7bd0 (PROT 0936's stager)
/// PORT: overlay_cast_hyper_lightning_0937_801f7850 (PROT 0937's stager)
/// PORT: FUN_801F776C (PROT 0945's stager)
/// PORT: FUN_801F7EC4 (PROT 0956's stager)
/// PORT: overlay_cast_white_shield_0955_801f9370 (PROT 0955's stager)
/// PORT: overlay_cast_terio_punch_0953_801f7624 (PROT 0953's stager)
pub fn enemy_cast_stager_spawns(
    entry: u32,
    arm: u8,
    caller: StagerCaller,
    roll: u8,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let at = |record: u32, scale: u16| on_caller(record, caller, scale);
    let own = caller.scale;
    match (entry, arm) {
        (935, 0) => vec![at(0x801F_81EC, 0x1000)],
        (941, 1) => vec![at(0x801F_7F98, 0x1000)],
        (941, 2) => vec![at(0x801F_8184, 0x1000)],
        (941, 3) => vec![at(0x801F_81C8, 0x1000)],
        (941, 4) => vec![at(0x801F_8238, 0x1000)],
        (942, 0) => vec![at(0x801F_81D0, own)],
        (942, 1) => vec![at(0x801F_8480, own)],
        (942, 2) => vec![at(0x801F_84F8, own)],
        (943, 0) => vec![at(0x801F_7788, own)],
        (943, 1) => vec![at(0x801F_784C, own), at(0x801F_78A8, own)],
        (943, 2) => vec![at(0x801F_791C, own)],
        (945, 0 | 1) => vec![at(0x801F_7878, own)],
        (945, 2) => vec![at(0x801F_7CF0, 0x1000)],
        (945, 3) => vec![at(0x801F_7DF0, 0x1000)],
        (956, 0) => vec![at(0x801F_80B4, 0x1000)],
        (956, 1) => vec![
            at(0x801F_8290, own),
            at(0x801F_82DC, own),
            at(0x801F_8330, own),
            at(0x801F_84C4, own),
        ],
        (956, 2) => vec![at(0x801F_850C, own)],
        (953, 0) => vec![at(0x801F_78FC, 0x1000)],
        (953, 1) => vec![at(0x801F_794C, 0x1000)],
        // Three triples on the caller, each turned off its yaw by
        // `0x80 - rand() % 257` and carrying `+0x3E = 16 - rand() % 33`
        // (the first draw), stored into all three (`0x801F76AC..0x801F77A0`).
        (953, 2) => (0..3)
            .flat_map(|_| {
                let lift = (0x10 - (rand() % 33) as i32) as u16;
                let turn = 0x80 - (rand() % 0x101) as i32;
                let mut rot = caller.rot;
                rot[1] = (i32::from(rot[1]) + turn) as i16;
                [0x801F_7D70, 0x801F_7DB4, 0x801F_7E08].map(|r| {
                    let mut s = at(r, 0x1000);
                    s.rot = rot;
                    s.pokes = vec![(0x3E, lift)];
                    s
                })
            })
            .collect(),
        (955, 0) => vec![at(0x801F_93EC, own)],
        (955, 1) => vec![at(0x801F_9744, own)],
        (951, 0) => vec![at(0x801F_82EC, 0x1000)],
        (951, 1) => vec![at(0x801F_82EC, 0x1000), at(0x801F_83B0, 0x1000)],
        (951, 2) => vec![at(0x801F_84EC, own)],
        (951, 3) => vec![at(0x801F_87DC, own), at(0x801F_8908, own)],
        (964, 0) => match roll {
            0 => vec![at(0x801F_8EDC, own)],
            1 => vec![at(0x801F_8F38, own)],
            2 => vec![at(0x801F_8F94, own)],
            _ => Vec::new(),
        },
        (964, 1) => (0..4u16)
            .map(|i| {
                let mut s = at(0x801F_9008, own);
                let a = (rand() % 12) as u16;
                let b = ((rand() % 33) as i16 - 0x10) as u16;
                let c = (rand() % 64) as u16;
                let d = (rand() % 32 + 0x20) as u16;
                let e = (rand() % 128 + 0x400) as u16;
                let f = (rand() % 0x400) as u16 + (i << 10);
                s.pokes = vec![
                    (0x54, a),
                    (0x84, b),
                    (0xBA, c),
                    (0xB4, d),
                    (0xB6, e),
                    (0x28, f),
                ];
                s
            })
            .collect(),
        (964, 2) => vec![at(0x801F_9568, own)],
        // Hyper Crush's ray burst: the flash, then eight rays, each turned
        // `i * 0x200 + (rand() & 0x1FF)` about its roll `+0x28` with a spin
        // `+0x84 = (rand() & 0xFF) - 0x80` (`0x801F7C04..0x801F7C70`). Called
        // seven times, five vsyncs apart, by arm 1's `0x801F7D6C`.
        (936, 0) => ray_burst(at(0x801F_7DE4, 0x1000), 0x801F_7D84, caller, false, rand),
        (936, 1) => vec![at(0x801F_8094, 0x1000), at(0x801F_810C, 0x1000)],
        // Hyper Lightning's: arm 0 is Hyper Crush's on its own records
        // (`0x801F78A8..0x801F7910`), arm 2 the same with a third draw into
        // the ray's height `+0xB4 = (rand() & 0xF) + 0x10`
        // (`0x801F793C..0x801F79B4`).
        (937, 0) => ray_burst(at(0x801F_7AF4, 0x1000), 0x801F_7A94, caller, false, rand),
        (937, 1) => vec![at(0x801F_7D00, 0x1000)],
        (937, 2) => ray_burst(at(0x801F_7AF4, 0x1000), 0x801F_7DB0, caller, true, rand),
        _ => Vec::new(),
    }
}

/// A flash and eight rays on the caller, the ray burst PROT 0936's and
/// 0937's stagers share: each ray's spin `+0x84` and roll `+0x28` off two
/// `rand()` draws, and with `height` a third into `+0xB4`.
fn ray_burst(
    flash: StagerSpawn,
    ray: u32,
    caller: StagerCaller,
    height: bool,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let mut out = vec![flash];
    for i in 0..8u16 {
        let mut s = on_caller(ray, caller, 0x1000);
        let spin = ((rand() & 0xFF) as i16 - 0x80) as u16;
        let roll = (i << 9).wrapping_add((rand() & 0x1FF) as u16) & 0xFFF;
        s.pokes = vec![(0x84, spin), (0x28, roll)];
        if height {
            s.pokes.push((0xB4, (rand() & 0xF) as u16 + 0x10));
        }
        out.push(s);
    }
    out
}

/// PROT 0962's stager `FUN_801F813C`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 1 | `0x801F82AC` on the caller (`0x801F8174..0x801F81CC`) |
/// | 2 | `0x801F86D4`, `0x801F8720` and `0x801F88FC` on the caller (`0x801F8188..0x801F81CC`): Ultra Charge's three rings, called every fifteen frames by its caster-seated `0x801F86B8` |
///
/// Each through `FUN_80050ED4` at `0x1000`.
///
/// PORT: FUN_801F813C (PROT 0962 stager)
pub fn blade_breath_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        1 => vec![on_caller(0x801F_82AC, caller, 0x1000)],
        2 => [0x801F_86D4, 0x801F_8720, 0x801F_88FC]
            .into_iter()
            .map(|r| on_caller(r, caller, 0x1000))
            .collect(),
        _ => Vec::new(),
    }
}

/// PROT 0965's stager `FUN_801F7B74` (Doomsday), reached through
/// `gp[+0x714]` from its parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7CD4`, then `0x801F7D24` (`0x801F7BC0..0x801F7BEC`) |
/// | 1 | `0x801F7E0C` (`0x801F7BF0..0x801F7C00`): the ring arm 6's `0x801F7DF0` calls every twenty-four frames, each widening off its own `+0xB4` / `+0xB6` rates |
/// | 2 | `0x801F7EE4` (`0x801F7C04..0x801F7C0C`) |
///
/// Each through `FUN_80050ED4` on the caller's `+0x14` / `+0x24` at
/// `0x1000`.
///
/// PORT: FUN_801F7B74 (PROT 0965 stager)
pub fn doomsday_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let seat = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![seat(0x801F_7CD4), seat(0x801F_7D24)],
        1 => vec![seat(0x801F_7E0C)],
        2 => vec![seat(0x801F_7EE4)],
        _ => Vec::new(),
    }
}

/// PROT 0944's stager `FUN_801F7F2C`, reached through `gp[+0x714]` from its
/// parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7FE8` (`FUN_80021B04`, `0x801F7F80`) |
/// | 1 | `0x801F8244`, then falls into arm 2's call (`0x801F7FA0..0x801F7FB8`) |
/// | 2 | `0x801F8290` |
///
/// Every call sits on the caller's `+0x14` / `+0x24` and forwards its `+0x72`
/// as the scale. Guilty Cross's arm-3 record `0x801F8210` calls arms 1 and 2
/// in a loop.
///
/// PORT: FUN_801F7F2C (PROT 0944 stager)
pub fn guilty_cross_cell_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let scale = caller.scale;
    match arm {
        0 => vec![on_caller(0x801F_7FE8, caller, scale)],
        1 => vec![
            on_caller(0x801F_8244, caller, scale),
            on_caller(0x801F_8290, caller, scale),
        ],
        2 => vec![on_caller(0x801F_8290, caller, scale)],
        _ => Vec::new(),
    }
}

/// The records a capture-cell stager seats with words rolled off `rand()`
/// stored over the seat: PROT 0946's Call Wave rays (`0x801F7970`, arm 0 of
/// [`call_wave_stager_spawns`]), PROT 0961's shards (`0x801F7E80`, arm 2
/// of [`dead_end_crisis_stager_spawns`]) and PROT 0964's rays (`0x801F9008`,
/// arm 1 of [`enemy_cast_stager_spawns`]). Each part's wait, lean rate
/// `+0x84`, spread, length and roll are draws on the stream's history since
/// the fight began, which a replay does not re-deal; a capture reads them
/// back off the part.
pub const ROLLED_STAGER_RECORDS: [u32; 3] = [0x801F_7970, 0x801F_7E80, 0x801F_9008];
/// The rolled words of a [`ROLLED_STAGER_RECORDS`] part beyond a seated
/// part's position, banks, wait and channel block: the lean rate `+0x84`.
pub const ROLLED_STAGER_EXTRA_OFFSETS: [u16; 1] = [0x84];

/// PROT 0946, Call Wave (`0x55`) and Big Wave (`0x56`) in one body.
pub const CALL_WAVE_ENTRY: u32 = 946;
/// Big Wave's action id: the choreography PROT 0946's arm 0 runs on its
/// latch-set branch (`0x801F6B00`), which the port keys on the action.
pub const BIG_WAVE_ACTION: u8 = 0x56;

/// Whether PROT 0946 owes a fold for `action_id`. Only Big Wave hits: its
/// arm 7 is the module's one damage site (`jal 0x801DD4B0` at `0x801F73DC`).
/// Call Wave runs arms 0..3 and arm 3 ends the module once its countdown
/// drains (`s6` cleared at `0x801F6E84`, the exit at `0x801F6E88`), so it never
/// reaches arm 7: the cast is the warning, its damage is the next turn's.
pub fn call_wave_folds(action_id: u8) -> bool {
    action_id == BIG_WAVE_ACTION
}

/// PROT 0946's wave record the stager's arm 1 seats.
pub const CALL_WAVE_WAVE_RECORD: u32 = 0x801F_7BF8;
/// The handle table `0x801F7F30` arm 1 fills: sixteen slots, and arm 1 seats
/// only into a null one (`0x801F7894..0x801F78A8`). Nothing in the body
/// clears a slot as its part dies, so a cast seats sixteen waves at most.
pub const CALL_WAVE_WAVE_SLOTS: usize = 16;

/// PROT 0946's stager `FUN_801F76C4`, reached through `gp[+0x714]` from its
/// parts' op `0x20` (`a1` the arm):
///
/// | Arm | Caller | Seats |
/// |---:|---|---|
/// | 0 | `0x801F78CC`, every eleven frames (Call Wave) | `0x801F7928`, `0x801F79C0`, `0x801F7A08` on the caller (`0x801F76F8..0x801F7744`), then eight rays `0x801F7970`, ray `k` stored after its seat `+0x54 = k + r % 3`, `+0x84 = r % 256 - 0x80`, `+0xBA = r % 128`, `+0xB6 = r % 256 + 0x1000`, `+0x28 = 0x200 k + r % 512 - 0x100`, one `rand()` each in that order (`0x801F774C..0x801F7840`) |
/// | 1 | `0x801F7C44`, every five frames (Big Wave) | while the phase `ctx[+0x279]` is below `7`, one wave `0x801F7BF8` into the first null slot of the handle table ([`CALL_WAVE_WAVE_SLOTS`]), none once it is full (`0x801F7870..0x801F78AC`) |
///
/// Every call is `FUN_80021B04(caller + 0x14, caller + 0x24, record, 0x1000)`.
/// `rand()` is `FUN_80056798`, never negative, so each `%` is the plain
/// remainder the arm's `mult` / shift sequences compute.
///
/// PORT: FUN_801F76C4 (PROT 0946 stager)
pub fn call_wave_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    phase: u8,
    waves_seated: usize,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => {
            let mut out: Vec<StagerSpawn> = [0x801F_7928, 0x801F_79C0, 0x801F_7A08]
                .into_iter()
                .map(|r| on_caller(r, caller, 0x1000))
                .collect();
            for k in 0..8u16 {
                let mut s = on_caller(0x801F_7970, caller, 0x1000);
                let wait = k + (rand() % 3) as u16;
                let lean = (rand() % 256) as i32 - 0x80;
                let ba = (rand() % 128) as u16;
                let b6 = (rand() % 256) as u16 + 0x1000;
                let roll = (i32::from(k) << 9) + (rand() % 512) as i32 - 0x100;
                s.pokes = vec![
                    (0x54, wait),
                    (0x84, lean as u16),
                    (0xBA, ba),
                    (0xB6, b6),
                    (0x28, roll as u16),
                ];
                out.push(s);
            }
            out
        }
        1 if phase < 7 && waves_seated < CALL_WAVE_WAVE_SLOTS => {
            vec![on_caller(CALL_WAVE_WAVE_RECORD, caller, 0x1000)]
        }
        _ => Vec::new(),
    }
}

/// PROT 0961's stager `FUN_801F78A4` (row `0x20 + 26` of `0x801F6734`),
/// reached through `gp[+0x714]` from its parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F7AD0`, `0x801F7B1C` (`0x801F7914`, `0x801F7A30`) |
/// | 1 | `0x801F7DB0` (`0x801F7A30`) |
/// | 2 | `0x801F7DF4`, `0x801F7E38`, then four `0x801F7E80`, part `k` stored after its seat `+0x54 = 8 k + r % 32`, `+0x84 = r % 256 - 0x80`, `+0xBA = r % 128`, `+0xB6 = r % 256 + 0x800`, `+0x28 = r % 4096`, one `rand()` each in that order (`0x801F7944..0x801F79F4`) |
/// | 3 | `0x801F8234`, `0x801F8278` (`0x801F7A18`, `0x801F7A30`) |
///
/// Every call is `FUN_80050ED4(caller + 0x14, caller + 0x24, record,
/// 0x1000)`.
///
/// PORT: FUN_801F78A4 (PROT 0961 stager)
pub fn dead_end_crisis_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let at = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![at(0x801F_7AD0), at(0x801F_7B1C)],
        1 => vec![at(0x801F_7DB0)],
        2 => {
            let mut out = vec![at(0x801F_7DF4), at(0x801F_7E38)];
            for k in 0..4u16 {
                let mut s = at(0x801F_7E80);
                let wait = (k << 3) + (rand() % 32) as u16;
                let lean = (rand() % 256) as i32 - 0x80;
                let ba = (rand() % 128) as u16;
                let b6 = (rand() % 256) as u16 + 0x800;
                let roll = (rand() % 4096) as u16;
                s.pokes = vec![
                    (0x54, wait),
                    (0x84, lean as u16),
                    (0xBA, ba),
                    (0xB6, b6),
                    (0x28, roll),
                ];
                out.push(s);
            }
            out
        }
        3 => vec![at(0x801F_8234), at(0x801F_8278)],
        _ => Vec::new(),
    }
}

/// PROT 0959's held white fade, the stager's arm 3 (`0x801F85A8..0x801F85FC`):
/// black to white over `0x20` frames after a `0x40` delay, held (`-1`), and
/// parked at `ctx[+0x102C]` for arm 2 to kill.
pub const MEGATON_PRESS_HELD_WHITE: legaia_engine_battle_vm::battle_action::SummonFadeTemplate =
    legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
        kind: 1,
        duration: 0x20,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        delay: 0x40,
        hold: -1,
    };
/// PROT 0959's white fade-out, the stager's arm 2 (`0x801F84A4..0x801F84D4`):
/// white to black over `0x40` frames, after it kills the held white.
pub const MEGATON_PRESS_WHITE_OUT: legaia_engine_battle_vm::battle_action::SummonFadeTemplate =
    legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
        kind: 1,
        duration: 0x40,
        start_rgb: [0xFF, 0xFF, 0xFF],
        end_rgb: [0, 0, 0],
        delay: 0,
        hold: 0,
    };

/// One of PROT 0959's twelve-rock rings (stager arms 1 and 4): rock `i` at
/// `sin` / `cos` of entry `i * 4096 / 12 + 0xAA` of the SCUS tables over
/// four (`lh 0x154(table + 2 idx)`, `>> 2` toward zero), each pushed
/// `rand() % 256 - 0x80` on both axes, at height `rand() % 512 + y0`, turned
/// by three draws `rand() % 4096`. The draw order is the three angles, `x`,
/// `z`, then the height.
fn megaton_press_rock_ring(
    record: u32,
    y0: i32,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    (0..12i32)
        .map(|i| {
            let rot = [
                (rand() % 4096) as i16,
                (rand() % 4096) as i16,
                (rand() % 4096) as i16,
            ];
            let idx = ((i << 12) / 12 + 0xAA) as u16;
            let (sin, cos) = legaia_engine_system::trig::trig12(idx);
            let x = i32::from(sin) / 4 + (rand() % 256) as i32 - 0x80;
            let z = i32::from(cos) / 4 + (rand() % 256) as i32 - 0x80;
            let y = (rand() % 512) as i32 + y0;
            StagerSpawn {
                record,
                pos: [x as i16, y as i16, z as i16],
                rot,
                pokes: Vec::new(),
                patches: Vec::new(),
                halve_speed: false,
                scale: 0x1000,
            }
        })
        .collect()
}

/// PROT 0959's stager `FUN_801F8250` (Megaton Press), reached through
/// `gp[+0x714]` from its parts' op `0x20` (`a1` the arm, the table at
/// `0x801F69D8`):
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F8868`, `0x801F88C4` on the caller |
/// | 1 | a ring of twelve rocks `0x801F8C20` around the origin at height `-0x900..-0x700` ([`megaton_press_rock_ring`]), then `0x801F8940`, `0x801F89D8`, `0x801F8AE4`, `0x801F8B30` on the caller (the last three through arm 5's tail, `j 0x801F87A4`) |
/// | 2 | (the host flushes the pool, kills the held white and fades white out: [`MEGATON_PRESS_WHITE_OUT`]) `0x801F8E18`, `0x801F8E60` on the caller |
/// | 3 | four `0x801F8DD4` on the caller's position, rolled `rand() % 512 + 0x400 k + 0x100` about Z, each then stored `+0x84 = rand() % 65 - 32`; (the host parks [`MEGATON_PRESS_HELD_WHITE`]) |
/// | 4 | a ring of twelve rocks `0x801F8E34` at height `-0x100..0x100` |
/// | 5 | `0x801F9478`, `0x801F8AE4`, `0x801F8B30` on the caller |
///
/// Every call goes through `FUN_80050ED4` at `0x1000`; a caller seat is its
/// `+0x14` / `+0x24`.
///
/// PORT: FUN_801F8250 (PROT 0959 stager; the fades are the host's)
pub fn megaton_press_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let at = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![at(0x801F_8868), at(0x801F_88C4)],
        1 => {
            let mut out = megaton_press_rock_ring(0x801F_8C20, -0x900, rand);
            // `0x801F89D8`'s seat is arm 5's `jal` at `0x801F87A4`, reached
            // by `j` (`0x801F8470`), so the arm runs on through arm 5's last
            // two seats.
            out.extend([
                at(0x801F_8940),
                at(0x801F_89D8),
                at(0x801F_8AE4),
                at(0x801F_8B30),
            ]);
            out
        }
        2 => vec![at(0x801F_8E18), at(0x801F_8E60)],
        3 => (0..4u16)
            .map(|k| {
                let mut s = at(0x801F_8DD4);
                s.rot = [0, 0, ((rand() % 512) as u16 + (k << 10) + 0x100) as i16];
                s.pokes = vec![(0x84, ((rand() % 65) as i32 - 32) as u16)];
                s
            })
            .collect(),
        4 => megaton_press_rock_ring(0x801F_8E34, -0x100, rand),
        5 => vec![at(0x801F_9478), at(0x801F_8AE4), at(0x801F_8B30)],
        _ => Vec::new(),
    }
}

/// PROT 0960's stager `FUN_801F86B0` (Plasma Strike), reached through
/// `gp[+0x714]` from its parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F8784`, `0x801F87E0` on the caller |
/// | 1 | `0x801F8B40` on the caller |
/// | 2 | `0x801F8C74` on the caller |
///
/// Every call goes through `FUN_80050ED4` at `0x1000`.
///
/// PORT: FUN_801F86B0 (PROT 0960 stager)
pub fn plasma_strike_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let at = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![at(0x801F_8784), at(0x801F_87E0)],
        1 => vec![at(0x801F_8B40)],
        2 => vec![at(0x801F_8C74)],
        _ => Vec::new(),
    }
}

/// PROT 0958's stager `FUN_801F8D30` (Blazing Slash), reached through
/// `gp[+0x714]` from its parts' op `0x20`:
///
/// | Arm | Seats |
/// |---:|---|
/// | 0 | `0x801F8ED4`, `0x801F8F30` on the caller |
/// | 1 | `0x801F9188` on the caller |
/// | 2 | `0x801F8F8C`, `0x801F8FF4`, `0x801F905C`, `0x801F90B4`, `0x801F910C`, `0x801F9188` on the caller |
///
/// Every call goes through `FUN_80050ED4` at `0x1000`, on the caller's
/// `+0x14` trio and `+0x24` angles. Arm 0 is the call arm 0's emitter
/// `0x801F8EB8` makes `31` times, `11` vsyncs apart: the flame cone pairs
/// that build and die under the caster through arms 1..4. Arm 2 is the
/// opening call of `0x801F9164` (arms 5 and 21), arm 1 its `31` repeats.
///
/// PORT: FUN_801F8D30 (PROT 0958 stager)
pub fn blazing_slash_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let at = |r: u32| on_caller(r, caller, 0x1000);
    match arm {
        0 => vec![at(0x801F_8ED4), at(0x801F_8F30)],
        1 => vec![at(0x801F_9188)],
        2 => [
            0x801F_8F8C,
            0x801F_8FF4,
            0x801F_905C,
            0x801F_90B4,
            0x801F_910C,
            0x801F_9188,
        ]
        .into_iter()
        .map(at)
        .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn blazing_slash_stager_seats_its_cone_pair_and_flame_rows() {
        let caller = StagerCaller {
            pos: [1, 2, 3],
            rot: [4, 5, 6],
            ..Default::default()
        };
        let recs = |arm| {
            blazing_slash_stager_spawns(arm, caller)
                .iter()
                .map(|s| s.record)
                .collect::<Vec<_>>()
        };
        assert_eq!(recs(0), [0x801F_8ED4, 0x801F_8F30]);
        assert_eq!(recs(1), [0x801F_9188]);
        assert_eq!(recs(2).len(), 6);
        assert!(recs(3).is_empty());
        let s = blazing_slash_stager_spawns(0, caller);
        assert_eq!(
            (s[0].pos, s[0].rot, s[0].scale),
            ([1, 2, 3], [4, 5, 6], 0x1000)
        );
    }

    #[test]
    fn doomsday_stager_arm_one_seats_the_ring_on_its_caller() {
        let caller = StagerCaller {
            pos: [1, 2, 3],
            rot: [4, 5, 6],
            ..Default::default()
        };
        let s = doomsday_stager_spawns(1, caller);
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].record, s[0].pos, s[0].rot, s[0].scale),
            (0x801F_7E0C, [1, 2, 3], [4, 5, 6], 0x1000)
        );
        assert_eq!(doomsday_stager_spawns(0, caller).len(), 2);
        assert!(doomsday_stager_spawns(3, caller).is_empty());
    }

    use super::*;

    #[test]
    fn megaton_press_rock_ring_circles_the_origin() {
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            0x80
        };
        let s = megaton_press_stager_spawns(1, StagerCaller::default(), &mut rand);
        assert_eq!(s.len(), 16);
        assert_eq!(n, 72, "six draws per rock");
        // `rand() % 256 - 0x80` is `0` here: each rock sits on the circle of
        // radius `0x400`, rock 0 at entry `0xAA`.
        let (sin, cos) = legaia_engine_system::trig::trig12(0xAA);
        assert_eq!(s[0].pos, [sin / 4, (0x80 - 0x900) as i16, cos / 4]);
        assert!(s[..12].iter().all(|r| r.record == 0x801F_8C20));
    }

    #[test]
    fn dead_end_crisis_arm_two_pokes_four_shards() {
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            n * 0x123
        };
        let s = dead_end_crisis_stager_spawns(2, StagerCaller::default(), &mut rand);
        assert_eq!(s.len(), 6);
        assert_eq!(n, 20, "five draws per shard");
        assert!(s[2..].iter().all(|x| x.record == 0x801F_7E80));
        // Shard 1: draws 6..=10.
        assert_eq!(s[3].pokes[0], (0x54, 8 + (6 * 0x123) % 32));
        assert_eq!(s[3].pokes[3], (0xB6, ((9 * 0x123) % 256 + 0x800) as u16));
        assert_eq!(s[3].pokes[4], (0x28, 10 * 0x123));
    }

    #[test]
    fn mystic_circle_fan_turns_by_a_sixteenth() {
        let caller = StagerCaller {
            rot: [0, 0xF00, 0],
            scale: 0x800,
            ..Default::default()
        };
        let s = chaos_breath_cell_stager_spawns(1, caller);
        assert_eq!(s.len(), 8);
        assert_eq!(s[0].rot, [0, 0x100, 0]);
        assert_eq!(s[7].rot, [0, 0xF00, 0]);
        assert!(
            s.iter()
                .all(|x| x.scale == 0x800 && x.record == 0x801F_7C60)
        );
    }

    #[test]
    fn ultra_charge_rings_are_the_three_arm_two_records() {
        let s = blade_breath_cell_stager_spawns(2, StagerCaller::default());
        let recs: Vec<u32> = s.iter().map(|x| x.record).collect();
        assert_eq!(recs, [0x801F_86D4, 0x801F_8720, 0x801F_88FC]);
        assert!(blade_breath_cell_stager_spawns(0, StagerCaller::default()).is_empty());
    }

    #[test]
    fn call_wave_rays_take_five_draws_each() {
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            n * 0x101
        };
        let s = call_wave_stager_spawns(0, StagerCaller::default(), 0, 0, &mut rand);
        assert_eq!(s.len(), 11);
        // Ray 0's draws are 0x101, 0x202, 0x303, 0x404, 0x505.
        assert_eq!(
            s[3].pokes,
            vec![
                (0x54, (0x101 % 3) as u16),
                (0x84, (0x202 % 256 - 0x80) as u16),
                (0xBA, (0x303 % 128) as u16),
                (0xB6, (0x404 % 256 + 0x1000) as u16),
                (0x28, (0x505 % 512 - 0x100) as u16),
            ]
        );
    }

    #[test]
    fn call_wave_waves_stop_at_phase_seven_or_a_full_table() {
        let mut rand = || 0;
        let c = StagerCaller::default();
        assert_eq!(call_wave_stager_spawns(1, c, 6, 15, &mut rand).len(), 1);
        assert!(call_wave_stager_spawns(1, c, 7, 0, &mut rand).is_empty());
        assert!(call_wave_stager_spawns(1, c, 0, 16, &mut rand).is_empty());
    }
}
