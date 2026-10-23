//! [`RetailBattle::from_ram`]: reading a battle save state's encounter out
//! of main RAM.

use super::*;

/// `ctx[+0x274]` - the next actor the initiative pick `FUN_801DABA4` chose.
pub(super) const NEXT_ACTOR: u32 = 0x274;

pub(super) fn in_ram(p: u32) -> bool {
    (0x8000_0000..0x8020_0000).contains(&p)
}

pub(super) fn combatant(ram: &[u8], slot: u32) -> Option<Combatant> {
    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
    if !in_ram(p) {
        return None;
    }
    Some(Combatant {
        hp: game_anchors::u16_at(ram, p + 0x14C),
        hp_max: game_anchors::u16_at(ram, p + 0x14E),
        mp: game_anchors::u16_at(ram, p + 0x150),
        mp_max: game_anchors::u16_at(ram, p + 0x152),
    })
}

/// The slot-B module image's span: a cast module links at `0x801F69D8` and
/// its records sit inside it (`docs/subsystems/cast-module.md`).
const SLOT_B: std::ops::Range<u32> = 0x801F_69D8..0x8020_0000;

/// `FUN_80021DF4`, the part tick a move-VM node carries at `+0x0C`.
const PART_TICK: u32 = 0x8002_1DF4;

/// Every live part of a record a slot-B cast module seats on every pass
/// ([`per_pass_spawn_records`](legaia_engine_cast::cast_module_camera::per_pass_spawn_records)),
/// in actor-list order, as a
/// [`CastPartSeed`](legaia_engine_effects::summon::CastPartSeed): its record
/// `+0x48` and the halfwords at
/// [`CAST_PART_SEED_OFFSETS`](legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS).
/// A node whose kill bit (`+0x10 & 8`) is up is left out.
///
/// Those parts are the passes' history: their count and ages follow the
/// frame driver's cadence and PROT 0962's rays each carry a `rand()` roll.
/// A part seated once is left to the replay, whose own state is aligned to
/// the displayed frame - the RAM runs one frame-pair ahead of it, and a
/// growing channel seated from the RAM overshoots the frame on screen.
///
/// Ultra Charge's camera-locked shot discs
/// ([`ULTRA_CHARGE_SHOT_DISCS`](legaia_engine_cast::cast_module_camera::ULTRA_CHARGE_SHOT_DISCS))
/// are seated too: their growth channel is a clock of the frames since arm
/// 0, and taken back over the two passes' steps it lands on the frame on
/// screen (`cort_evolved_ultra_charge_mid_cast` `.901` to `.918`; Ultra
/// Charge's rings, seated the same way, measured no better and are left to
/// the replay).
///
/// The seeds are taken back to the displayed frame the same way
/// ([`displayed_part_halfword`]): each integrated word less the vsyncs the
/// part tick ran on it since - the two passes' own steps ([`pass_steps`]),
/// not twice the history's [`frame_step`]. A history of clamped entries
/// (`cort_evolved_ultra_charge_mid_cast` holds sixteen `0x2BC`s) reads step
/// `3` off the clamp where every pass ran step `4`, and the rays stood two
/// vsyncs' turn off the frame on screen.
///
/// The parts a capture-cell stager seats with `rand()` rolls
/// ([`ROLLED_STAGER_RECORDS`](legaia_engine_cast::cast_module_ticks::ROLLED_STAGER_RECORDS))
/// are seated the same way, their lean rate `+0x84` with them: Call Wave's
/// rays fan at angles the replay's own stream does not deal.
pub(super) fn retail_cast_parts(ram: &[u8]) -> Vec<legaia_engine_effects::summon::CastPartSeed> {
    use legaia_engine_cast::cast_module_camera::ULTRA_CHARGE_SHOT_DISCS;
    use legaia_engine_cast::cast_module_ticks::{
        ROLLED_STAGER_EXTRA_OFFSETS, ROLLED_STAGER_RECORDS,
    };
    let per_pass: Vec<u32> =
        legaia_engine_cast::cast_module_camera::per_pass_spawn_records().collect();
    let lag = pass_steps(ram).iter().map(|&s| i32::from(s)).sum::<i32>();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            let rec = game_anchors::u32_at(ram, n + 0x48);
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::u32_at(ram, n + 0x10) & 0x8 == 0
                && SLOT_B.contains(&rec)
                && (per_pass.contains(&rec)
                    || ROLLED_STAGER_RECORDS.contains(&rec)
                    || ULTRA_CHARGE_SHOT_DISCS.contains(&rec))
        })
        .map(|n| {
            let tag = game_anchors::u32_at(ram, n + 0x48);
            let extra: &[u16] = if ROLLED_STAGER_RECORDS.contains(&tag) {
                &ROLLED_STAGER_EXTRA_OFFSETS
            } else {
                &[]
            };
            legaia_engine_effects::summon::CastPartSeed {
                tag,
                pokes: legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS
                    .iter()
                    .chain(extra)
                    .map(|&o| (o, displayed_part_halfword(ram, n, o, lag)))
                    .collect(),
            }
        })
        .collect()
}

/// The render banks an effect-script table part's seed carries: its
/// rotation trio `+0x24..+0x28`.
const ACTION_FX_SEED_OFFSETS: [u16; 3] = [0x24, 0x26, 0x28];

/// What a growing default-arm node's seed carries on top of
/// [`ACTION_FX_SEED_OFFSETS`]: the radii `+0xB4 / +0xB6`, the heights
/// `+0xB8 / +0xBA` and the length `+0xC8` the emitter `FUN_80028158` reads
/// (`src = node + 0x9C`, so `src[+0x18..+0x1E]`; `+0xC8 >> 3` is the
/// phase). The part tick grows them off `+0xC0..+0xC6` / `+0xCA` every
/// pass in render modes `2` and `6`, so like the banks they are a clock of
/// the node's age: Tail Fire's floor ring (record `0x801F5EB0`) on
/// `battle_gimard_tail_fire_a` holds radii `224 / 320` at rates
/// `224 / 320` in RAM, and the displayed frame drew it a lag smaller -
/// mostly hidden behind Vahn's boots, where the replay's ring, seated on
/// its own clock, stood a white band around them.
const DEFAULT_ARM_CHANNEL_OFFSETS: [u16; 5] = [0xB4, 0xB6, 0xB8, 0xBA, 0xC8];

/// Whether part `node` is a draw-kind-4 node on the default arm
/// (`+0x56 == 4`, `+0x9E & 0x6000 == 0`, `0x8001B128..0x8001B134`) in a
/// render mode whose channel block grows (`+0x5A` `2` or `6`,
/// `0x80021E78..0x80021FA0`).
fn is_growing_default_arm(ram: &[u8], node: u32) -> bool {
    let h = |o: u32| game_anchors::u16_at(ram, node + o);
    h(0x56) == 4 && h(0x9E) & 0x6000 == 0 && matches!(h(0x5A), 2 | 6)
}

/// What a burst child's seed carries on top of [`CAST_PART_SEED_OFFSETS`]
/// (position, banks, wait, channel block): the three words the burst
/// `FUN_801F30C4` rolls off `rand()` for it - spread `+0x3E`, tail `+0x98`
/// and, through the seater's `rot[1] & 0xFFF`, heading `+0x96` - and the
/// scale `+0x72` it writes beside them.
///
/// [`CAST_PART_SEED_OFFSETS`]: legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS
const BURST_CHILD_EXTRA_OFFSETS: [u16; 4] = [0x3E, 0x72, 0x96, 0x98];

/// What a growing morph part's seed carries on top of its rotation trio:
/// its depth-cue level `+0x78` and its first morph lane's weight `+0xA0`.
/// Both are clocks of the part's age - the melee ray burst grows its lane
/// `0x155` a vsync and ramps its level `0x100` a vsync - and the replay
/// seats its burst on its own pass, not on retail's
/// (`rim_elm_gimard_victory`: lane `0xFFC` and level `0x400` in the RAM,
/// `2728` and `0` on screen, where the replay's had grown `2387`).
const MORPH_SEED_OFFSETS: [u16; 2] = [0x78, 0xA0];

/// The melee ray bursts: the effect-script table arm's codes `0` and `9`
/// (`0x801F6324[0]` / `[9]`, pool meshes `8` / `9`).
const RAY_BURST_RECORDS: [u32; 2] = [0x801F_5484, 0x801F_54B4];

/// Only the ray bursts are seeded this way ([`is_ray_burst`]). Other
/// morphing parts are left to the replay: seeded the same way, Ultra
/// Charge's (`cort_evolved_ultra_charge_mid_cast`) lost half its frame.
///
/// A ray burst's level `+0x78` or lane weight `+0xA0` as the displayed
/// frame held it: the level gains `(+0x90 * 8) >> 6` a vsync
/// (`part_motion::level_block`), the lane its up rate `+0xB8` a vsync
/// (`FUN_80020740`), and both clamp to `0..=0x1000`, so taken back they
/// stay inside it.
pub(super) fn displayed_morph_halfword(ram: &[u8], node: u32, off: u16, lag: i32) -> u16 {
    let h = |o: u32| i32::from(game_anchors::u16_at(ram, node + o) as i16);
    let per_vsync = if off == 0x78 { h(0x90) >> 3 } else { h(0xB8) };
    (h(u32::from(off)) - per_vsync * lag).clamp(0, 0x1000) as u16
}

/// Whether part `node` is a melee ray burst under its envelope gate (`+0x10`
/// bit `0x1000`, op `0x0A`), lane clamped or not.
fn is_ray_burst(ram: &[u8], node: u32) -> bool {
    RAY_BURST_RECORDS.contains(&game_anchors::u32_at(ram, node + 0x48))
        && game_anchors::u32_at(ram, node + 0x10) & 0x1000 != 0
}

/// The ray burst's position `+0x14..+0x18`. The burst stands where its
/// program put it (no velocity, no speed), so once that program has run
/// its level ramp longer than the display lag the RAM's position is the
/// displayed frame's ([`ray_burst_stood_through_lag`]). The replay's strike
/// point is its own: `battle_melee_hit_spark`'s burst (level `0x900`, nine
/// vsyncs into its ramp) stands at `(-5, -330, 368)` in the RAM and
/// `(-4, -330, 378)` in the replay, its banks at `4132` against `4144`
/// (`image` `.847` to `.876`).
const RAY_BURST_POSITION_OFFSETS: [u16; 3] = [0x14, 0x16, 0x18];

/// Whether the burst's level ramp `+0x78` (`(+0x90 * 8) >> 6` a vsync)
/// began before the displayed frame, `lag` vsyncs back: the words its
/// program wrote by then are the screen's. A ramp no older than the lag
/// started inside it (its displayed level reads `0`), with whatever the
/// program rewrote alongside: `rim_elm_gimard_victory`'s level reads
/// `0x400` (four vsyncs) against a four-vsync lag, and seated on the RAM's
/// position its frame fell `.931` to `.920`.
pub(super) fn ray_burst_stood_through_lag(ram: &[u8], node: u32, lag: i32) -> bool {
    let h = |o: u32| i32::from(game_anchors::u16_at(ram, node + o) as i16);
    let per_vsync = h(0x90) >> 3;
    let level = h(0x78);
    per_vsync > 0 && level < 0x1000 && level > per_vsync * lag
}

/// [`MORPH_SEED_OFFSETS`] a ray burst can still be seeded on: a level or
/// lane on its `0x1000` clamp names no age and is left to the replay.
fn ray_burst_morph_offsets(ram: &[u8], node: u32) -> Vec<u16> {
    MORPH_SEED_OFFSETS
        .iter()
        .copied()
        .filter(|&o| game_anchors::u16_at(ram, node + u32::from(o)) < 0x1000)
        .collect()
}

/// Every live effect-script table part (`FUN_80021DF4` nodes whose record
/// `+0x48` lies in the resident battle overlay below the slot-B image -
/// the `0x801F6324` prototypes `FUN_801DEA50`'s table arm seats) that is
/// turning (a non-zero rate in `+0x80..+0x84`), in actor-list order, as a
/// [`CastPartSeed`](legaia_engine_effects::summon::CastPartSeed) of its
/// rotation trio as the displayed frame held it ([`displayed_part_halfword`]).
///
/// A turning part's banks are a clock of its age in vsyncs: the part tick
/// adds the rate times the frame step every pass, so a part seated on a
/// step-`2` pass has turned two vsyncs' worth by its first draw, where the
/// replay's per-vsync seat has turned one. The summon invoke clip's ray fan
/// (record `0x801F5A04`, `-56` a vsync about Y) reads that way: one vsync
/// ahead of the displayed frame in the replay (`freed_summon_mid_cast`
/// `+0x26` `812` against the displayed `868`), every ray standing a few
/// degrees off retail's. Only the banks are seated - the part's program
/// words (wait, PC, level ramp) may have been rewritten inside the display
/// lag and are not recovered - plus, on a growing default-arm node (seated
/// whether or not it turns), its channel block
/// ([`DEFAULT_ARM_CHANNEL_OFFSETS`]), the same clock.
pub(super) fn retail_action_fx_parts(
    ram: &[u8],
) -> Vec<legaia_engine_effects::summon::CastPartSeed> {
    let lag = i32::from(display_lag_vsyncs(ram));
    let overlay = legaia_game_tables::move_power::BATTLE_OVERLAY_BASE..SLOT_B.start;
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            let rec = game_anchors::u32_at(ram, n + 0x48);
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::u32_at(ram, n + 0x10) & 0x8 == 0
                && overlay.contains(&rec)
                && (is_burst_child(rec)
                    || is_growing_default_arm(ram, n)
                    || is_ray_burst(ram, n)
                    || (0x80..=0x84)
                        .step_by(2)
                        .any(|o| game_anchors::u16_at(ram, n + o) != 0))
        })
        .map(|n| {
            let tag = game_anchors::u32_at(ram, n + 0x48);
            let offsets: Vec<u16> = if is_burst_child(tag) {
                legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS
                    .iter()
                    .chain(&BURST_CHILD_EXTRA_OFFSETS)
                    .copied()
                    .collect()
            } else if is_growing_default_arm(ram, n) {
                ACTION_FX_SEED_OFFSETS
                    .iter()
                    .chain(&DEFAULT_ARM_CHANNEL_OFFSETS)
                    .copied()
                    .collect()
            } else if is_ray_burst(ram, n) {
                return legaia_engine_effects::summon::CastPartSeed {
                    tag,
                    pokes: RAY_BURST_POSITION_OFFSETS
                        .iter()
                        .filter(|_| ray_burst_stood_through_lag(ram, n, lag))
                        .chain(&ACTION_FX_SEED_OFFSETS)
                        .map(|&o| (o, displayed_part_halfword(ram, n, o, lag)))
                        .chain(
                            ray_burst_morph_offsets(ram, n)
                                .into_iter()
                                .map(|o| (o, displayed_morph_halfword(ram, n, o, lag))),
                        )
                        .collect(),
                };
            } else {
                ACTION_FX_SEED_OFFSETS.to_vec()
            };
            legaia_engine_effects::summon::CastPartSeed {
                tag,
                pokes: offsets
                    .iter()
                    .map(|&o| (o, displayed_part_halfword(ram, n, o, lag)))
                    .collect(),
            }
        })
        .collect()
}

/// A battle actor's ring-id history `+0x1FB`: one byte a battle frame,
/// slot 0 the live frame (`FUN_80049348` reads `+0x1FB + s0`).
const RING_ID: u32 = 0x1FB;
/// Its anim-cursor history `+0x17A`: one halfword a battle frame.
const RING_CURSOR: u32 = 0x17A;
/// The ring's depth (`0x1F` shifts and slot 0).
const RING_SLOTS: u32 = 32;
/// The lowest ring id the ghost walk draws (`sltiu v0,v0,0x11` at
/// `0x80049460`).
const GHOST_RING_ID_MIN: u8 = 0x11;

/// Every after-image ghost the **displayed** frame drew, as the history
/// ring holds it ([`legaia_engine_capture::ghost_ring`]).
///
/// The walk `FUN_80049348` reads ring slots `step` and `2 * step` back from
/// the frame it draws, `step = 8 / +0x21D` (`1` for a zero rate), doubled on
/// a monster seat (`0x800493F0..0x80049428`), and skips a slot whose ring id
/// is `0x10` or below. The displayed frame was drawn
/// [`DISPLAYED_FRAMES_BACK`] slots before the RAM's, so each ghost on
/// screen is that many slots further down. Only a ghost of the clip the
/// displayed frame shows (the same ring id) is seated: the engine re-poses
/// it from the body's playing clip. A depth the gate closed is seeded as
/// undrawn, so a replay whose art committed earlier on its own clock draws
/// no ghost there either (`battle_vahn_tri_somersault_super`: the art's
/// first slot is 5, so the walk's depth 4 from the displayed slot 2 reads a
/// ring id of `0` and retail draws one ghost, the replay two).
///
/// Party seats only. A party seat's ring id is its committed dynamic slot
/// `+0x1D9` (`0x80047FCC`), which names the clip the engine must be
/// playing; a monster's is `record[+0x77] + 0x10` (`0x80048044..0x80048060`),
/// which does not, and the Tail Fire captures' Gimard ghosts read worse
/// seated on a replay standing elsewhere (`battle_gimard_tail_fire_a`
/// `.925` to `.920`).
pub(super) fn retail_ghost_ring(
    ram: &[u8],
    party_count: u8,
) -> Vec<legaia_engine_capture::ghost_ring::GhostRingSeed> {
    let mut out = Vec::new();
    for pool in 0..u32::from(party_count.clamp(1, 3)) {
        let slot = pool;
        let p = game_anchors::u32_at(ram, ACTOR_TABLE + pool * 4);
        if !in_ram(p) {
            continue;
        }
        let rate = u32::from(game_anchors::u8_at(ram, p + 0x21D));
        let step = (8u32.checked_div(rate).unwrap_or(0)).max(1);
        let shown = game_anchors::u8_at(ram, p + RING_ID + DISPLAYED_FRAMES_BACK);
        for depth in [step, 2 * step] {
            let at = DISPLAYED_FRAMES_BACK + depth;
            if at >= RING_SLOTS {
                continue;
            }
            let id = game_anchors::u8_at(ram, p + RING_ID + at);
            let (Ok(slot), Ok(depth)) = (u8::try_from(slot), u8::try_from(depth)) else {
                continue;
            };
            // The walk drew nothing here: the replay must not either.
            if id < GHOST_RING_ID_MIN {
                out.push(legaia_engine_capture::ghost_ring::GhostRingSeed {
                    slot,
                    clip: id,
                    depth,
                    drawn: false,
                    cursor: 0,
                    x: 0,
                    z: 0,
                });
                continue;
            }
            if id != shown {
                continue;
            }
            out.push(legaia_engine_capture::ghost_ring::GhostRingSeed {
                slot,
                clip: id,
                depth,
                drawn: true,
                cursor: game_anchors::u16_at(ram, p + RING_CURSOR + at * 2),
                x: i32::from(game_anchors::u32_at(ram, p + POSITION_RING + at * 8) as i16),
                z: i32::from(game_anchors::u32_at(ram, p + POSITION_RING + at * 8 + 4) as i16),
            });
        }
    }
    out
}

/// Whether `record` is one of the two stager records the battle burst
/// `FUN_801F30C4` seats its twelve children on (the wide arm's
/// `0x801F5DA4`, the narrow arm's `0x801F5D0C`). Each child's heading,
/// spread and tail are three `rand()` draws, so where a burst's rays and
/// puffs point is the stream's history since the fight began, which no
/// replay re-deals: `battle_melee_hit_spark`'s Somersault holds twelve
/// narrow-arm rays fanned at headings no seed reproduces.
fn is_burst_child(record: u32) -> bool {
    [
        legaia_engine_vm::battle_burst::BurstMode::Wide,
        legaia_engine_vm::battle_burst::BurstMode::Narrow,
    ]
    .iter()
    .any(|m| m.record_addr() == record)
}

/// Part `node`'s halfword `+off` as the displayed frame held it, `lag`
/// vsyncs before the RAM: the part tick `FUN_80021DF4` integrates, a frame
/// at a time and scaled by the frame step times the rate byte `8` (`>> 6`),
/// so one vsync's worth is the rate `>> 3`.
///
/// - the render banks `+0x24..+0x28` by `+0x80..+0x84` (`0x80022918..`);
/// - the position `+0x14..+0x18` by the velocity `+0x3C..+0x40` and, on X
///   / Z, the speed `+0x98` along the heading `+0x96` (`0x800229B4..`);
///   both skipped for render modes `3` and `5` (`0x800228A0..0x800228B0`);
/// - the channels `+0xB4..+0xBA` and `+0xC8` by `+0xC0..+0xC6` / `+0xCA` in
///   render modes `2` and `6` (`0x80021E78..0x80021FA0`);
/// - the wait `+0x54`, drained by the same product (`8` a vsync).
///
/// A word the part's program rewrote inside the lag is not recovered.
pub(super) fn displayed_part_halfword(ram: &[u8], node: u32, off: u16, lag: i32) -> u16 {
    let h = |o: u32| i32::from(game_anchors::u16_at(ram, node + o) as i16);
    let now = h(u32::from(off));
    let mode = h(0x5A);
    let moves = mode != 3 && mode != 5;
    let per_vsync = match off {
        0x24 | 0x26 | 0x28 if moves => h(u32::from(off) + 0x5C) >> 3,
        0x14 | 0x16 | 0x18 if moves => {
            let heading = (h(0x96) & 0xFFF) as u16;
            let (sin, cos) = legaia_engine_system::trig::trig12(heading);
            let speed = h(0x98);
            let along = match off {
                0x14 => i32::from(sin) * speed,
                0x18 => i32::from(cos) * speed,
                _ => 0,
            };
            (along + (h(u32::from(off) + 0x28) << 12)) >> 15
        }
        0xB4 | 0xB6 | 0xB8 | 0xBA if mode == 2 || mode == 6 => h(u32::from(off) + 0x0C) >> 3,
        0xC8 if mode == 2 || mode == 6 => h(0xCA) >> 3,
        0x54 => -8,
        _ => 0,
    };
    let then = now - per_vsync * lag;
    // The length channel `+0xC8` grows from zero off op `0x1C`'s rate: one
    // that rewinds past zero was (re)started inside the lag and stood at
    // zero on screen. Call Wave's spent volley reads `0` at rate `96`; taken
    // back eight vsyncs it would read `-96`, a ray drawn backwards.
    if off == 0xC8 && now >= 0 && per_vsync > 0 && then < 0 {
        return 0;
    }
    then as u16
}

/// `ctx[+0x273]` - the pushes the damage-number ring took this action.
const RING_PUSHES: u32 = 0x273;
/// `ctx[+0x83C + i * 4]` - ring entry `i`'s amount.
const RING_VALUE: u32 = 0x83C;
/// `ctx[+0x318 + i * 2]` - ring entry `i`'s pool slot.
const RING_SEAT: u32 = 0x318;
/// The ring's entry count (`FUN_801F44A0` folds its cursor `mod 8`).
const RING_LEN: u8 = 8;
/// The string's running total `DAT_8007BD14` (`0x801EEA64..0x801EEA78`).
const COMBO_TOTAL: u32 = 0x8007_BD14;

/// The hits the action in flight has landed on each pool slot's HP, off the
/// damage-number ring `FUN_801F44A0` pushes (inlined at `0x801E1898..0x801E1918`
/// in the monster cast tick, at `0x801EDDE4` in the melee kernel):
/// `(pool slot, amount)`.
///
/// The action seed `0x0C` zeroes the push count `ctx[+0x273]` and the cursor
/// `ctx[+0x262]` at the head of every action (`0x801E2C3C` / `0x801E2C40`),
/// so inside an action entries `0..count` are exactly its own pushes. Ahead
/// of that pass (the round states and `0x0C` itself) the ring still holds
/// the previous action's, which no replay re-deals, and a count past the
/// ring's length has overwritten its first hits; both read as none.
///
/// A push is not always on the bar yet. The monster cast tick stores the HP
/// beside its push (`0x801E1960`), but the melee kernel banks a swing's
/// damage in the victim's word `+0x00` (`0x801EDB40`, the amount it pushes)
/// and takes the whole bank off `+0x14C`
/// later (`0x801EEA3C`, zeroing the bank at `0x801EEA74`): a capture
/// mid-combo shows the hits on the ring and the victim's HP untouched
/// (`battle_vahn_tri_somersault_super`: `242` pushed, `442` banked, full
/// HP). What the bank still holds is taken off the slot's sum.
/// The ring's pushes in order, `(pool slot, amount)` - the hits the action
/// in flight resolved, each as the melee kernel landed it, `0` where the
/// number has left the screen and the ring no longer holds it. Empty where
/// [`action_hits`] is (outside an action, or a ring that has wrapped).
pub(super) fn action_hit_sequence(ram: &[u8], ctx: u32) -> Vec<(u8, u16)> {
    let state = game_anchors::u8_at(ram, ctx + 7);
    let count = game_anchors::u8_at(ram, ctx + RING_PUSHES);
    if game_anchors::u8_at(ram, ctx + 6) != 0xFF
        || PRE_SEED_STATES.contains(&state)
        || state == 0x0C
        || count > RING_LEN
    {
        return Vec::new();
    }
    (0..u32::from(count))
        .filter_map(|i| {
            let value = game_anchors::u16_at(ram, ctx + RING_VALUE + i * 4) as i16;
            let seat = game_anchors::u16_at(ram, ctx + RING_SEAT + i * 2);
            (seat < 8).then_some((seat as u8, value.max(0) as u16))
        })
        .collect()
}

pub(super) fn action_hits(ram: &[u8], ctx: u32) -> Vec<(u8, u16)> {
    let state = game_anchors::u8_at(ram, ctx + 7);
    let count = game_anchors::u8_at(ram, ctx + RING_PUSHES);
    if game_anchors::u8_at(ram, ctx + 6) != 0xFF
        || PRE_SEED_STATES.contains(&state)
        || state == 0x0C
        || count > RING_LEN
    {
        return Vec::new();
    }
    let mut pushed = [0u32; 8];
    for i in 0..u32::from(count) {
        let value = game_anchors::u16_at(ram, ctx + RING_VALUE + i * 4) as i16;
        let seat = game_anchors::u16_at(ram, ctx + RING_SEAT + i * 2);
        if value > 0 && seat < 8 {
            pushed[seat as usize] += value as u32;
        }
    }
    (0..8u8)
        .filter_map(|slot| {
            let p = game_anchors::u32_at(ram, ACTOR_TABLE + u32::from(slot) * 4);
            let banked = if in_ram(p) {
                game_anchors::u32_at(ram, p)
            } else {
                0
            };
            let landed = pushed[slot as usize].saturating_sub(banked);
            (landed > 0).then_some((slot, landed.min(u32::from(u16::MAX)) as u16))
        })
        .collect()
}

impl RetailBattle {
    /// Read the battle observables; `Err` names why the state is not a
    /// seedable battle.
    pub fn from_ram(ram: &[u8]) -> std::result::Result<Self, String> {
        let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
        if !in_ram(ctx) {
            return Err(format!(
                "battle context pointer 0x{ctx:08X} not resident (battle still loading)"
            ));
        }
        let party_count = game_anchors::u8_at(ram, ctx);
        let monster_count = game_anchors::u8_at(ram, ctx + 1);
        if !(1..=3).contains(&party_count) || !(1..=5).contains(&monster_count) {
            return Err(format!(
                "battle context counts out of range (party {party_count}, monsters {monster_count})"
            ));
        }
        let monster_ids: Vec<u8> = (0..u32::from(monster_count).min(4))
            .map(|m| game_anchors::u8_at(ram, FORMATION_CELL + m))
            .collect();
        if monster_ids.iter().all(|&id| id == 0) {
            return Err("formation cell empty".into());
        }
        let action_state = game_anchors::u8_at(ram, ctx + 7);
        let active_actor = if game_anchors::u8_at(ram, ctx + 6) == 0xFF
            && PRE_SEED_STATES.contains(&action_state)
        {
            game_anchors::u8_at(ram, ctx + NEXT_ACTOR)
        } else {
            game_anchors::u8_at(ram, ctx + 0x13)
        };
        let active = Some(game_anchors::u32_at(
            ram,
            ACTOR_TABLE + u32::from(active_actor.min(7)) * 4,
        ))
        .filter(|&p| in_ram(p));
        Ok(Self {
            party_count,
            monster_count,
            flow: game_anchors::u8_at(ram, ctx + 6),
            action_state,
            run_state: game_anchors::u8_at(ram, RUN_STATE),
            stage_id: game_anchors::u8_at(ram, STAGE_ID),
            scripted: game_anchors::u32_at(ram, PER_BATTLE_FLAGS) & 0x80 != 0,
            stage_variant: game_anchors::u8_at(ram, PER_BATTLE_FLAGS) & 0x1F,
            keep_backdrop_object_1: game_anchors::u8_at(ram, KEEP_BACKDROP_OBJECT_1) != 0,
            mystic_shield: game_anchors::u32_at(ram, MYSTIC_SHIELD_HANDLE) != 0,
            cast_parts: retail_cast_parts(ram),
            action_fx_parts: retail_action_fx_parts(ram),
            effect_children: retail_effect_children(ram),
            ghost_ring: retail_ghost_ring(ram, party_count),
            backdrop_slot_1_yaw: game_anchors::u16_at(ram, BACKDROP_SLOT_1_YAW),
            monster_ids,
            seat_chars: (0..u32::from(party_count))
                .map(|s| game_anchors::u8_at(ram, SEAT_CHARS + s))
                .collect(),
            party_categories: (0..u32::from(party_count))
                .map(|s| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + s * 4);
                    if in_ram(p) {
                        game_anchors::u8_at(ram, p + 0x1DE)
                    } else {
                        0
                    }
                })
                .collect(),
            party: (0..u32::from(party_count))
                .map(|s| combatant(ram, s))
                .collect(),
            monsters: (0..u32::from(monster_count))
                .map(|m| combatant(ram, 3 + m))
                .collect(),
            active_actor,
            queued_action: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF)),
            target_code: active.map_or(0, |p| seed_target_code(ram, ctx, p)),
            queued_category: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DE)),
            summon_fade: summon_fade(ram),
            display_lag: display_lag_vsyncs(ram),
            display_pass_lag: display_pass_steps(ram).iter().map(|&s| u16::from(s)).sum(),
            frame_step: frame_step(ram),
            cam_accum: game_anchors::u32_at(ram, ctx + 0x87C),
            caster_clip: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1D9)),
            walk_yaw_base: game_anchors::u16_at(ram, ctx + 0x6DA),
            track_coin: game_anchors::u8_at(ram, ctx + 0x26D) & 1,
            gimard_countdown: game_anchors::u32_at(
                ram,
                legaia_engine_cast::cast_module_camera::GIMARD_COUNTDOWN_VA,
            ) as i32,
            cam_style: game_anchors::u8_at(ram, ctx + 0xD),
            cam_tween: retail_cam_tween(ram, ctx),
            displayed_cam_live: {
                let passes = tween_passes_back(
                    action_state,
                    game_anchors::u16_at(ram, ctx + 0x6DA),
                    DISPLAY_LAG_PASSES,
                );
                // Each pass on its own step where the older ordering table's
                // floor confirms the newest one; the history's largest entry
                // for every pass otherwise.
                let steps = display_pass_steps(ram);
                let confirmed = steps != [frame_step(ram); 2];
                let steps: Vec<u8> = (0..usize::from(passes))
                    .map(|n| steps.get(n).copied().unwrap_or(frame_step(ram)))
                    .collect();
                displayed_cam_live_steps(
                    ram,
                    ctx,
                    &steps,
                    confirmed,
                    summon_route_prior_end(ram, ctx, action_state),
                )
            },
            displayed_module_cam_live: displayed_module_cam_live(
                ram,
                ctx,
                &display_pass_steps(ram)[..usize::from(DISPLAY_LAG_PASSES).min(2)],
            ),
            cam_landed: camera_landed(ram, ctx),
            camera_option: game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION),
            battle_id: game_anchors::u8_at(ram, BATTLE_ID_WORD),
            hud_glides_landed: (0..HUD_GLIDE_SLOTS).all(|s| {
                game_anchors::u8_at(ram, ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE) == 0
            }),
            hud_glides: {
                // Taken back over the two passes' own steps, as the
                // round-prompt and landed seats below are: a glide's
                // `elapsed` advances by each pass's step.
                // `zeto_call_wave_mid_cast`'s message bar reads `11` in the
                // RAM, `7` in the older ordering table (a step-`4` pass) and
                // `3` on screen, where twice the history's step-`3` word
                // would put it at `5`.
                let lag: u16 = pass_steps(ram).iter().map(|&s| u16::from(s)).sum();
                let mut seats = hud_glide_seats(ram, ctx, lag);
                // A round prompt whose open glide the table has already
                // cleared can still be unfolding on screen.
                if game_anchors::u8_at(ram, ctx + 6) == 0x1E
                    && seats.is_empty()
                    && let Some(seat) = round_prompt_pool_seat(ram, display_pass_steps(ram)[1])
                {
                    seats.push(seat);
                }
                // Retired records that landed on the frozen pass, still
                // mid-glide on screen.
                // The combo cluster, retired in the RAM but gliding on screen.
                if let Some(seat) = combo_cluster_timer_seat(ram, ctx, display_lag_vsyncs(ram))
                    && !seats.iter().any(|s| s.target == seat.target)
                {
                    seats.push(seat);
                }
                for seat in landed_glide_pool_seats(ram, ctx, display_pass_steps(ram)) {
                    if !seats.iter().any(|s| s.target == seat.target) {
                        seats.push(seat);
                    }
                }
                seats
            },
            popup_cells: displayed_popup_cells(ram),
            popup_age: popup_age_seat(ram, ctx, display_lag_vsyncs(ram), frame_step(ram)),
            spirit_gauge: retail_spirit_gauge(ram, ctx, active, display_lag_vsyncs(ram)),
            hud_glides_unseen: {
                let lag = display_lag_vsyncs(ram);
                let live: Vec<u8> = (0..HUD_GLIDE_SLOTS)
                    .map(|s| ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE)
                    .filter(|&r| game_anchors::u8_at(ram, r) != 0)
                    .map(|r| game_anchors::u8_at(ram, r + 1))
                    .collect();
                !live.is_empty() && live.iter().all(|&e| u16::from(e) < lag)
            },
            select_attack: game_anchors::u8_at(ram, SELECT_ATTACK_OPTION),
            entry_counter: game_anchors::u8_at(ram, ENTRY_COUNTER),
            absorbed_seru: game_anchors::u8_at(ram, ctx + 0x269),
            magic_level_up: game_anchors::u8_at(ram, ctx + 0x26) == MAGIC_LEVEL_BANNER,
            strike_cursor: game_anchors::u8_at(ram, ctx + 0x15),
            committed_queue: std::array::from_fn(|i| {
                active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF + i as u32))
            }),
            arts_queue: active.is_some_and(|p| {
                (0..0x10).any(|i| matches!(game_anchors::u8_at(ram, p + 0x1DF + i), 0x19 | 0x1A))
            }),
            acting_gauge: active.map(|p| {
                (
                    game_anchors::u16_at(ram, p + 0x154),
                    game_anchors::u16_at(ram, p + 0x156),
                )
            }),
            span_gate: span_gate(ram, ctx),
            win_pose: span_gate(ram, ctx)
                .is_end()
                .then(|| active.map(|p| game_anchors::u8_at(ram, p + 0x1DB)))
                .flatten(),
            module_phase: game_anchors::u8_at(ram, ctx + 0x279),
            action_hits: action_hits(ram, ctx),
            action_hit_sequence: action_hit_sequence(ram, ctx),
            combo_total: u32::from(game_anchors::u16_at(ram, COMBO_TOTAL)),
            entry_latches: Vec::new(),
            resident_shiny_seru: false,
            inflight_idle: [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
            clip_cursors: (0..8u32)
                .filter_map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    if !in_ram(p) {
                        return None;
                    }
                    let node = game_anchors::u32_at(ram, p + 0x22C);
                    if !in_ram(node) {
                        return None;
                    }
                    let clip = game_anchors::u8_at(ram, p + 0x1D9);
                    let cursor = i32::from(game_anchors::u16_at(ram, node + 0x68));
                    // The frame on screen is the one built two game frames
                    // before the RAM's ([`display_lag_vsyncs`]), and
                    // each frame advances the cursor by `(frame step * rate
                    // +0x21D * the entry's rate byte +0x78) >> 1` - `>> 2` for
                    // a Slowed (`+0x16E & 0x1000`) member's idle
                    // (`FUN_80047430`, `0x800476D8..0x80047764`).
                    let entry = game_anchors::u32_at(ram, node + 0x4C);
                    let entry_rate = if in_ram(entry) {
                        i32::from(game_anchors::u8_at(ram, entry + 0x78))
                    } else {
                        0
                    };
                    let status = game_anchors::u16_at(ram, p + 0x16E);
                    let slowed = status & 0x1000 != 0 && clip == 0;
                    // A Stone or Numb body on its idle takes no advance at
                    // all (`andi v0,a0,0x404` at `0x80047640`), so the frame
                    // on screen shows the RAM's cursor.
                    let frozen = status
                        & legaia_engine_battle_vm::battle_anim_rate::FROZEN_IDLE_STATUS_BITS
                        != 0
                        && clip == 0;
                    // Each of the lag's passes on its own step
                    // (`display_pass_steps`).
                    let back: i32 = display_pass_steps(ram)
                        .iter()
                        .map(|&step| {
                            let prod = i32::from(step)
                                * i32::from(game_anchors::u8_at(ram, p + 0x21D))
                                * entry_rate;
                            if frozen {
                                0
                            } else if slowed {
                                prod >> 2
                            } else {
                                prod >> 1
                            }
                        })
                        .sum();
                    Some((slot as u8, clip, cursor - back))
                })
                .collect(),
            captured: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    let h = |o: u32| game_anchors::u16_at(ram, p + o) as i16;
                    in_ram(p).then(|| {
                        (
                            [h(0x34), h(0x38)],
                            [h(0x3C), h(0x40)],
                            game_anchors::u8_at(ram, p + 0x1D9),
                            if slot >= 3 {
                                let rec = game_anchors::u32_at(ram, 0x801C_9348 + (slot - 3) * 4);
                                if in_ram(rec) {
                                    game_anchors::u8_at(ram, rec + 0x1F)
                                } else {
                                    0
                                }
                            } else {
                                0
                            },
                        )
                    })
                })
                .collect(),
            displayed_ground: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    let at = p + POSITION_RING + DISPLAYED_FRAMES_BACK * 8;
                    in_ram(p).then(|| {
                        [
                            game_anchors::u32_at(ram, at) as i32 as i16,
                            game_anchors::u32_at(ram, at + 4) as i32 as i16,
                        ]
                    })
                })
                .collect(),
            ground: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| {
                        [
                            game_anchors::u16_at(ram, p + 0x34) as i16,
                            game_anchors::u16_at(ram, p + 0x38) as i16,
                        ]
                    })
                })
                .collect(),
            facing: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| game_anchors::u16_at(ram, p + 0x46) & 0xFFF)
                })
                .collect(),
            turn_owner: game_anchors::u8_at(ram, ctx + NEXT_ACTOR),
            init_keys: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| game_anchors::u16_at(ram, p + 0x16C))
                })
                .collect(),
            defeat_lanes: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    (in_ram(p)
                        && game_anchors::u8_at(ram, p + 0x21C)
                            == legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE)
                        .then(|| game_anchors::u32_at(ram, p + 4))
                })
                .collect(),
            status: (0..8u32)
                .map(|slot| {
                    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
                    in_ram(p).then(|| game_anchors::u16_at(ram, p + 0x16E))
                })
                .collect(),
            timed_message: {
                let hold = game_anchors::u32_at(ram, TIMED_MESSAGE_HOLD) as i32;
                let va = game_anchors::u32_at(ram, TIMED_MESSAGE_WORD);
                (hold > 0 && va != 0).then_some((va, hold.min(i32::from(i16::MAX)) as i16))
            },
            target_plate_cleared: game_anchors::u32_at(ram, TARGET_PLATE_WORD) == 0,
            spoils_caption: game_anchors::u8_at(ram, ctx + 0x18)
                == legaia_engine_battle::battle_steal::STEAL_CAPTION_ELEMENT,
            target_fade_lane: active.and_then(|p| {
                let t = game_anchors::u8_at(ram, p + 0x1DD);
                let tp = game_anchors::u32_at(ram, ACTOR_TABLE + u32::from(t.min(7)) * 4);
                (t < 8 && in_ram(tp) && game_anchors::u8_at(ram, tp + 0x21C) == DEFEAT_FADE_STATE)
                    .then(|| (game_anchors::u32_at(ram, tp + 4) & 0x3FF) as u16)
            }),
        })
    }
}

/// The timed message's hold `0x801F6964` (`FUN_80046A20` counts it down).
pub(super) const TIMED_MESSAGE_HOLD: u32 = 0x801F_6964;
/// Record `0x66`'s content word, `0x80076C10 + 0x66 * 0x18 + 0x14`.
pub(super) const TIMED_MESSAGE_WORD: u32 = 0x8007_75B4;
/// Record `0x51`'s content word, `0x80076C10 + 0x51 * 0x18 + 0x14`.
pub(super) const TARGET_PLATE_WORD: u32 = 0x8007_73BC;

/// The static spell table's records (`DAT_800754C8`, 12-byte stride); the
/// target shape is the record's `+2` byte (`docs/formats/spell-table.md`).
const SPELL_TABLE: u32 = 0x8007_54C8;
/// Spell target shape bits: ally side, all on that side.
const SHAPE_ALL_ALLIES: u8 = 0x22;

/// The acting seat's target byte as its action **seed** read it.
///
/// The magic band rewrites a group code in place once a lone combatant is
/// left on that side (`0x801E4298..0x801E4334`): `8` becomes
/// `ctx[+0x24A] - 1` (`sb t2,0x1dd(s3)` at `0x801E431C`) and `9` becomes
/// `ctx[+0x24B]` (`0x801E42D0`), the census's sole-survivor latches. A
/// capture past that point holds the slot, while the replay starts at the
/// seed, which tests the byte once: a party-wide cast seeded on the slot
/// opens the target's readout bar, which retail's seed on `8` never does
/// (`evolved_0x91_midcast`, Holy Eyes from a lone Vahn). So a Magic action
/// whose spell is all-allies and whose byte is the lone member's slot is
/// handed back as `8`. The `9` arm is left as read: no capture shows what
/// an all-enemies replay on the code would change.
pub(super) fn seed_target_code(ram: &[u8], ctx: u32, actor: u32) -> u8 {
    let raw = game_anchors::u8_at(ram, actor + 0x1DD);
    let magic = game_anchors::u8_at(ram, actor + 0x1DE)
        == legaia_engine_battle_vm::battle_action::ActionCategory::Magic.as_byte();
    if !magic {
        return raw;
    }
    let spell = u32::from(game_anchors::u8_at(ram, actor + 0x1DF));
    let shape = game_anchors::u8_at(ram, SPELL_TABLE + spell * 12 + 2);
    let lone_party = game_anchors::u8_at(ram, ctx + 0x24A);
    if shape & SHAPE_ALL_ALLIES == SHAPE_ALL_ALLIES && lone_party != 0 && raw == lone_party - 1 {
        legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE
    } else {
        raw
    }
}
