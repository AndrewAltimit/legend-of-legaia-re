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
pub(super) fn retail_cast_parts(ram: &[u8]) -> Vec<legaia_engine_effects::summon::CastPartSeed> {
    let per_pass: Vec<u32> =
        legaia_engine_cast::cast_module_camera::per_pass_spawn_records().collect();
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
                .map(|&o| (o, game_anchors::u16_at(ram, n + u32::from(o))))
                .collect(),
        })
        .collect()
}

/// `ctx[+0x273]` - the pushes the damage-number ring took this action.
const RING_PUSHES: u32 = 0x273;
/// `ctx[+0x83C + i * 4]` - ring entry `i`'s amount.
const RING_VALUE: u32 = 0x83C;
/// `ctx[+0x318 + i * 2]` - ring entry `i`'s pool slot.
const RING_SEAT: u32 = 0x318;
/// The ring's entry count (`FUN_801F44A0` folds its cursor `mod 8`).
const RING_LEN: u8 = 8;

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
                seats
            },
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
            entry_latches: Vec::new(),
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
