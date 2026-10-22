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
/// The seeds are taken back to the displayed frame the same way
/// ([`displayed_part_halfword`]): each integrated word less the
/// [`display_lag_vsyncs`] the part tick ran on it since.
pub(super) fn retail_cast_parts(ram: &[u8]) -> Vec<legaia_engine_effects::summon::CastPartSeed> {
    let per_pass: Vec<u32> =
        legaia_engine_cast::cast_module_camera::per_pass_spawn_records().collect();
    let lag = i32::from(display_lag_vsyncs(ram));
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            let rec = game_anchors::u32_at(ram, n + 0x48);
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::u32_at(ram, n + 0x10) & 0x8 == 0
                && SLOT_B.contains(&rec)
                && per_pass.contains(&rec)
        })
        .map(|n| legaia_engine_effects::summon::CastPartSeed {
            tag: game_anchors::u32_at(ram, n + 0x48),
            pokes: legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS
                .iter()
                .map(|&o| (o, displayed_part_halfword(ram, n, o, lag)))
                .collect(),
        })
        .collect()
}

/// The render banks an effect-script table part's seed carries: its
/// rotation trio `+0x24..+0x28`.
const ACTION_FX_SEED_OFFSETS: [u16; 3] = [0x24, 0x26, 0x28];

/// What a burst child's seed carries on top of [`CAST_PART_SEED_OFFSETS`]
/// (position, banks, wait, channel block): the three words the burst
/// `FUN_801F30C4` rolls off `rand()` for it - spread `+0x3E`, tail `+0x98`
/// and, through the seater's `rot[1] & 0xFFF`, heading `+0x96` - and the
/// scale `+0x72` it writes beside them.
///
/// [`CAST_PART_SEED_OFFSETS`]: legaia_engine_effects::summon::CAST_PART_SEED_OFFSETS
const BURST_CHILD_EXTRA_OFFSETS: [u16; 4] = [0x3E, 0x72, 0x96, 0x98];

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
/// lag and are not recovered.
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

/// `_DAT_8007BD30` - the effect pool's base pointer (`FUN_801E0080`).
const EFFECT_POOL_PTR: u32 = 0x8007_BD30;
/// The pool's child slots: `0x80` of `0x20` bytes from `+0x10`.
const EFFECT_CHILD_SLOTS: u32 = 0x80;
/// Most pack0 batches the pointer table at head `+0x08` is searched over.
const PACK0_SEARCH: u32 = 0x100;

/// Every live effect-pool child of the state, as the displayed frame held
/// it ([`rewind_effect_child`]), in slot order. A child's anim cursor
/// `+0x1C` points into its pack0 batch (`entry + 2 + 6 * frame_cursor`);
/// the batch index the engine keys on is the entry's place in the pointer
/// table at the pool head's `+0x08`. A child whose batch is not found is
/// left out.
pub(super) fn retail_effect_children(
    ram: &[u8],
) -> Vec<legaia_engine_core::world::EffectChildSeed> {
    let pool = game_anchors::u32_at(ram, EFFECT_POOL_PTR);
    if !in_ram(pool) {
        return Vec::new();
    }
    let table = game_anchors::u32_at(ram, pool + 8);
    if !in_ram(table) {
        return Vec::new();
    }
    let motion_scale = i32::from(game_anchors::i16_at(ram, pool));
    let lag = display_lag_vsyncs(ram);
    let mut out = Vec::new();
    for slot in 0..EFFECT_CHILD_SLOTS {
        let c = pool + 0x10 + slot * 0x20;
        let frame_count = game_anchors::u8_at(ram, c);
        if frame_count == 0 {
            continue;
        }
        let frame_cursor = game_anchors::u8_at(ram, c + 2);
        let entry =
            game_anchors::u32_at(ram, c + 0x1C).wrapping_sub(2 + 6 * u32::from(frame_cursor));
        let Some(anim_id) =
            (0..PACK0_SEARCH).find(|&i| game_anchors::u32_at(ram, table + 4 * i) == entry)
        else {
            continue;
        };
        let pos = [0x0C, 0x10, 0x14].map(|o| game_anchors::u32_at(ram, c + o) as i32);
        let velocity = [4, 6, 8].map(|o| game_anchors::i16_at(ram, c + o));
        let seed = legaia_engine_core::world::EffectChildSeed {
            anim_id: anim_id as u16,
            frame_count,
            frame_cursor,
            wait: game_anchors::u8_at(ram, c + 3),
            mirror: game_anchors::u8_at(ram, c + 1),
            pos,
            velocity,
        };
        let frame = |k: u8, b: u32| game_anchors::u8_at(ram, entry + 2 + 6 * u32::from(k) + b);
        if let Some(seed) = rewind_effect_child(seed, lag, motion_scale, frame) {
            out.push(seed);
        }
    }
    out
}

/// Child `seed` taken back `sweeps` walker sweeps - the display lag, since
/// `FUN_801E0080` sweeps once a vsync. `frame(k, b)` reads byte `b` of the
/// batch's frame `k` (`+1` hold delay, `+2` motion speed). `None` for a
/// child the walker seeded inside the window, which the displayed frame
/// does not hold yet.
///
/// The inverse of one sweep (`0x801E0088` pass 1): a child whose wait still
/// holds its frame's whole delay entered that frame on the sweep, so it
/// stood on the previous frame at wait `0`; any other took `8` off its wait.
/// Either way it took one motion step, `vel * speed * scale * 8 >> 15`, on
/// the frame it ended the sweep on. A child on frame `0` within `8` of its
/// first delay was seeded on the sweep (the seed's batch is walked in the
/// same sweep, so it starts one decrement down).
pub(super) fn rewind_effect_child(
    mut seed: legaia_engine_core::world::EffectChildSeed,
    sweeps: u16,
    motion_scale: i32,
    frame: impl Fn(u8, u32) -> u8,
) -> Option<legaia_engine_core::world::EffectChildSeed> {
    for _ in 0..sweeps {
        let speed = i32::from(frame(seed.frame_cursor, 2));
        for (p, v) in seed.pos.iter_mut().zip(seed.velocity) {
            let step = i32::from(v)
                .wrapping_mul(speed)
                .wrapping_mul(motion_scale)
                .wrapping_shl(3)
                >> 15;
            *p = p.wrapping_sub(step);
        }
        let held = (u32::from(frame(seed.frame_cursor, 1)) << 3) as u8;
        if seed.frame_cursor == 0 {
            if seed.wait >= held.saturating_sub(8) {
                return None;
            }
            seed.wait += 8;
        } else if seed.wait == held {
            seed.frame_cursor -= 1;
            seed.wait = 0;
        } else {
            seed.wait = seed.wait.saturating_add(8);
        }
    }
    Some(seed)
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
    (now - per_vsync * lag) as u16
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
            displayed_cam_live: displayed_cam_live(
                ram,
                ctx,
                tween_passes_back(
                    action_state,
                    game_anchors::u16_at(ram, ctx + 0x6DA),
                    display_lag_vsyncs(ram) / u16::from(frame_step(ram).max(1)),
                ),
                frame_step(ram),
                summon_route_prior_end(ram, ctx, action_state),
            ),
            camera_option: game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION),
            battle_id: game_anchors::u8_at(ram, BATTLE_ID_WORD),
            hud_glides_landed: (0..HUD_GLIDE_SLOTS).all(|s| {
                game_anchors::u8_at(ram, ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE) == 0
            }),
            hud_glides: {
                let mut seats = hud_glide_seats(ram, ctx, display_lag_vsyncs(ram));
                // A round prompt whose open glide the table has already
                // cleared can still be unfolding on screen.
                if game_anchors::u8_at(ram, ctx + 6) == 0x1E
                    && seats.is_empty()
                    && let Some(seat) = round_prompt_pool_seat(ram, frame_step(ram))
                {
                    seats.push(seat);
                }
                // Retired records that landed on the frozen pass, still
                // mid-glide on screen.
                for seat in landed_glide_pool_seats(ram, ctx, frame_step(ram)) {
                    if !seats.iter().any(|s| s.target == seat.target) {
                        seats.push(seat);
                    }
                }
                seats
            },
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
                    let prod = i32::from(frame_step(ram))
                        * i32::from(game_anchors::u8_at(ram, p + 0x21D))
                        * entry_rate;
                    let advance = if frozen {
                        0
                    } else if slowed {
                        prod >> 2
                    } else {
                        prod >> 1
                    };
                    let frames_back =
                        i32::from(display_lag_vsyncs(ram) / u16::from(frame_step(ram).max(1)));
                    Some((slot as u8, clip, cursor - frames_back * advance))
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
