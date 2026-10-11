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
            backdrop_slot_1_yaw: game_anchors::u16_at(ram, BACKDROP_SLOT_1_YAW),
            monster_ids,
            seat_chars: (0..u32::from(party_count))
                .map(|s| game_anchors::u8_at(ram, SEAT_CHARS + s))
                .collect(),
            party: (0..u32::from(party_count))
                .map(|s| combatant(ram, s))
                .collect(),
            monsters: (0..u32::from(monster_count))
                .map(|m| combatant(ram, 3 + m))
                .collect(),
            active_actor,
            queued_action: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF)),
            target_code: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DD)),
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
                legaia_engine_battle_vm::cast_module_camera::GIMARD_COUNTDOWN_VA,
            ) as i32,
            cam_style: game_anchors::u8_at(ram, ctx + 0xD),
            cam_tween: retail_cam_tween(ram, ctx),
            camera_option: game_anchors::u8_at(ram, BATTLE_CAMERA_OPTION),
            hud_glides_landed: (0..HUD_GLIDE_SLOTS).all(|s| {
                game_anchors::u8_at(ram, ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE) == 0
            }),
            hud_glides: hud_glide_seats(ram, ctx, display_lag_vsyncs(ram)),
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
            entry_latches: Vec::new(),
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
