//! The world-map panel host's `World` seam.
//!
//! The overworld panel-actor host, the field party HUD's state machine and
//! the pad packing live in [`legaia_engine_field::world_map_panel_host`] and
//! are re-exported here at their old paths. What stays is the set of HUD
//! queries that read the world - the suppress gate, the view mode, the
//! rearm term and the present-party projection - which both hosts ask once
//! per frame.

pub use legaia_engine_field::world_map_panel_host::*;

/// Retail's `_DAT_8007B868` field-HUD suppress global, as the port can see
/// it - the one answer both hosts ask for, rather than one enumeration each.
///
/// `FUN_801D0D38` reads the global at its first instruction and, when it is
/// non-zero, clears `_DAT_8007B5F4` and jumps straight to its epilogue at
/// `0x801D1314`. The port has no single flag to mirror it, so this is the
/// enumeration of the states in which something else owns the frame.
///
/// **It gates the badge column too.** The passive-ability badges are
/// `FUN_801d095c`, and its only caller is `FUN_801D0D38`'s own
/// `jal 0x801d095c` at `0x801D130C` - eight bytes above the epilogue the
/// suppress arm jumps to, so a suppressed frame never reaches it. Every
/// surface that draws the badges therefore answers this question first,
/// exactly as the party readout does (see
/// `ghidra/scripts/funcs/overlay_0897_801d0d38.txt`).
///
/// `host_panel_owns_frame` is the one term the world cannot answer: a
/// host-side full-screen panel with no `World` state behind it (the native
/// window's boot UI, its in-window FMV player and its own typed dialog panel;
/// either host's menu runtime). Every other term is world state and is asked
/// here, so the two hosts cannot drift apart on it again.
///
/// The field party HUD's mode word `_DAT_800845C4`, which is the options
/// screen's **Field HP Display** row - the menu overlay's option descriptor
/// for row 6 names `0x800845C4` (PROT 0899 file `0x15CC0`). `0` Immediate
/// (`0x28`-frame idle), `1` Gradual (`0xA0`), `2` Display Off (no HUD).
/// Retail holds it at the player's choice everywhere, the overworld
/// included: every library save state, `map01..03` walking states among
/// them, reads `0`. Both hosts ask this one function.
///
/// PORT: FUN_801d0d38 (`lw a2,0x45c4(v0)` at `0x801D0D74`)
pub fn field_hud_view_mode(world: &crate::world::World) -> i32 {
    match world.toggles.field_hp_display {
        crate::options::HpDisplayOpt::Immediate => 0,
        crate::options::HpDisplayOpt::Gradual => 1,
        crate::options::HpDisplayOpt::DisplayOff => 2,
    }
}

/// PORT: FUN_801d0d38 (`0x801D0D38..0x801D0DBC` suppress gate)
pub fn field_hud_suppressed(world: &crate::world::World, host_panel_owns_frame: bool) -> bool {
    use crate::world::SceneMode;
    !matches!(world.mode, SceneMode::Field | SceneMode::WorldMap)
        || host_panel_owns_frame
        || world.dialog.current.is_some()
        || world.dialog.inline.is_some()
        || world.cutscene.text_balloon.is_some()
        || world.cutscene_timeline_active()
        // The naming prompt is modal on both hosts and neither had it in its
        // copy, so the readout sat behind the name-entry overlay wherever the
        // idle countdown had already expired. Which retail state raises the
        // global for it is not read off a dump - the term is here because the
        // overlay owns the frame, and because a term present on one host only
        // is the drift this kernel exists to end.
        || world.name_entry_active()
        // The field-to-battle transition. Its overlay, PROT 0979
        // `field_battle_intro`, loads into slot A at `0x801CE818` - the slot
        // the field overlay (0897) and so `FUN_801D0D38` itself live in - so
        // no readout is drawn on any transition frame; the intro's own
        // packets own the screen. The native window happened to paint the
        // readout under the intro's backdrop and the play page above it on
        // its separate overlay canvas, so the page showed it over the whole
        // shatter.
        || field_battle_transition_active(world)
}

/// `true` while the encounter session sits in its field-to-battle
/// `Transition` phase - the frames PROT 0979 owns slot A.
pub fn field_battle_transition_active(world: &crate::world::World) -> bool {
    matches!(
        world.encounters.session.as_ref().map(|s| s.phase()),
        Some(crate::encounter::EncounterPhase::Transition { .. })
    )
}

/// The player-engaged term of the field party HUD's **rearm** arm: while it
/// holds, the idle countdown restarts every frame, so the readout never
/// comes up.
///
/// `FUN_801D0D38` loads the player object out of `_DAT_8007C364`
/// (`lw a1,0x1c(s0)` off `s0 = 0x8007C348`, `0x801D0DC4`) and takes the rearm
/// arm when its `+0x10 & 0x80000` is set (`0x801D0DCC..0x801D0DD8`), ahead of
/// the scratchpad and staged-load terms. That bit is the engaged bit the
/// script runner `FUN_80039B7C` raises on every frame it steps a spawned
/// context and the touch post raises for a conversation, so a scene whose
/// script holds the player for its whole run - every ending scene, whose
/// entry script spawns the credits record and never releases it - shows no
/// readout at all. The engine answers the same question with
/// [`crate::world::World::script_context_engages_player`] and
/// [`crate::world::World::dialogue_owns_input`], plus the bit itself where
/// an engine path sets it on the player's `move_state`.
///
/// Hosts call [`FieldPartyHud::rearm`] on a frame this returns `true`, before
/// the tick.
///
/// PORT: FUN_801d0d38 (`0x801D0DC0..0x801D0DD8`, the player-bit rearm term)
pub fn field_hud_rearm_held(world: &crate::world::World) -> bool {
    let player_bit = world
        .player_actor_slot
        .and_then(|s| world.actors.get(usize::from(s)))
        .is_some_and(|a| a.move_state.flags & 0x0008_0000 != 0);
    player_bit || world.script_context_engages_player() || world.dialogue_owns_input()
}

/// Project the **present party** onto the field HUD's rows.
///
/// Retail's draw loop walks the present-party list at `0x80084598` for
/// `0x80084594` entries and indexes the character records with it; the engine
/// mirror is [`crate::world::World::party_roster_slot`] over
/// [`crate::world::PartyState::party_count`]. Every number comes from the *record*,
/// not from a battle actor: outside a fight the battle mirrors are stale (or
/// zero for a party that has never fought), and a field readout that only
/// works after the first battle is worse than none.
pub fn field_party_hud_members(world: &crate::world::World) -> Vec<FieldHudMemberData> {
    let count = (world.party.party_count as usize).min(3);
    let fallback = crate::field_menu_dispatch::roster_names(world);
    (0..count)
        .filter_map(|ordinal| {
            let slot = world.party_roster_slot(ordinal);
            let rec = world.party.roster.members.get(slot)?;
            let hms = rec.hp_mp_sp();
            // The record's own `+0x2A7` display name is what retail draws
            // (it carries the name the player typed for Vahn); the canonical
            // table stands in for a roster seeded without one.
            let name = {
                let n = rec.name();
                if n.trim().is_empty() {
                    fallback.get(slot).cloned().unwrap_or_default()
                } else {
                    n
                }
            };
            Some(FieldHudMemberData {
                name,
                level: rec.magic_rank(),
                hp: hms.hp_cur,
                hp_max: hms.hp_max,
                mp: hms.mp_cur,
                mp_max: hms.mp_max,
                alive: hms.hp_cur > 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::travel_art_actor::TravelArt;
    use legaia_engine_vm::world_map_panel_actors::{FlagWindowDescriptor, HudDecision};

    /// The Field HP Display option is the HUD's mode word, on the overworld
    /// as in a town: Immediate waits `0x28` frames, Gradual `0xA0`, Display
    /// Off never draws - and the scene kind changes none of it.
    #[test]
    fn the_hp_display_option_drives_the_hud_idle_on_every_scene_kind() {
        use crate::options::{HpDisplayOpt, OptionsState};
        use crate::world::{SceneMode, World};
        for (opt, mode, first_draw) in [
            (HpDisplayOpt::Immediate, 0, Some(0x28u32)),
            (HpDisplayOpt::Gradual, 1, Some(0xA0)),
            (HpDisplayOpt::DisplayOff, 2, None),
        ] {
            for scene in [SceneMode::Field, SceneMode::WorldMap] {
                let mut w = World::new();
                w.mode = scene;
                let o = OptionsState {
                    field_hp_display: opt,
                    ..OptionsState::default()
                };
                o.apply_to_world(&mut w);
                assert_eq!(field_hud_view_mode(&w), mode, "{opt:?} on {scene:?}");
                let mut hud = FieldPartyHud::new();
                let drew = (0..=0x100u32).find(|_| {
                    matches!(
                        hud.tick(false, mode, 0, Some((1, 2)), 1, None),
                        HudDecision::Draw { .. }
                    )
                });
                assert_eq!(drew, first_draw, "{opt:?} on {scene:?}");
            }
        }
    }

    /// The player's engaged bit holds the HUD in its rearm arm: a host that
    /// rearms on every held frame never reaches `Draw`, and once the bit
    /// drops the readout waits out a full idle countdown before it returns.
    #[test]
    fn an_engaged_player_keeps_the_hud_rearming() {
        let mut w = crate::world::World {
            player_actor_slot: Some(0),
            ..Default::default()
        };
        assert!(!field_hud_rearm_held(&w));
        w.actors[0].move_state.flags |= 0x0008_0000;
        assert!(field_hud_rearm_held(&w));

        let mut hud = FieldPartyHud::new();
        for _ in 0..200 {
            if field_hud_rearm_held(&w) {
                hud.rearm();
            }
            let d = hud.tick(false, 0, 0, Some((100, 200)), 1, None);
            assert!(!matches!(d, HudDecision::Draw { .. }), "held: {d:?}");
        }
        w.actors[0].move_state.flags &= !0x0008_0000;
        let mut first_draw = None;
        for frame in 0..200 {
            if field_hud_rearm_held(&w) {
                hud.rearm();
            }
            if let HudDecision::Draw { .. } = hud.tick(false, 0, 0, Some((100, 200)), 1, None) {
                first_draw = Some(frame);
                break;
            }
        }
        // The last held frame armed the full countdown; the released frames
        // spend it, and the one that takes it to zero draws.
        let idle = i32::from(legaia_engine_vm::world_map_panel_actors::hud_idle_frames(
            0, false,
        ));
        assert_eq!(
            first_draw,
            Some(idle - 1),
            "released: the near-camera idle countdown runs in full first"
        );
    }

    // ---------------------------------------------------------------------
    // The host chain: World::tick -> tick_world_map -> tick_world_map_panels
    // ---------------------------------------------------------------------

    fn world_on_the_overworld() -> crate::world::World {
        let mut w = crate::world::World::default();
        w.enter_world_map();
        w.world_map.ctrl.as_mut().unwrap().debug_enabled = true;
        w
    }

    /// The screen has to be reachable from the world tick, not just from a
    /// direct `PanelActorHost::install`. A raw Square edge must install the
    /// sub-list actor and open its window.
    #[test]
    fn the_world_tick_installs_the_sub_list_from_a_square_press() {
        let mut w = world_on_the_overworld();
        w.set_pad(0);
        let _ = w.tick();
        assert!(!w.world_map.ctrl.as_ref().unwrap().panels.is_active());
        w.set_pad(crate::input::PadButton::Square.mask());
        let _ = w.tick();
        let panels = &w.world_map.ctrl.as_ref().unwrap().panels;
        assert!(panels.is_active(), "Square installs the sub-list");
        assert!(
            panels.windows.is_open(SUBLIST_PANEL_INDEX),
            "and its open script spawned a window"
        );
    }

    /// The screen is a debug tool: without `debug_enabled` no chord installs
    /// anything, so a default overworld is unchanged.
    #[test]
    fn the_screen_stays_shut_without_the_debug_gate() {
        let mut w = crate::world::World::default();
        w.enter_world_map();
        w.set_pad(crate::input::PadButton::Square.mask());
        let _ = w.tick();
        assert!(!w.world_map.ctrl.as_ref().unwrap().panels.is_active());
    }

    /// The text box's confirm arm has to reach the *records*, not just the
    /// frame flag: a party on 1 HP must come back on full HP and MP.
    #[test]
    fn the_text_box_confirm_restores_the_partys_hp_and_mp() {
        let mut w = world_on_the_overworld();
        w.party.roster = legaia_save::Party::zeroed(3);
        for m in w.party.roster.members.iter_mut() {
            m.raw[0x104..0x106].copy_from_slice(&300u16.to_le_bytes()); // hp max
            m.raw[0x106..0x108].copy_from_slice(&1u16.to_le_bytes()); // hp cur
            m.raw[0x108..0x10A].copy_from_slice(&80u16.to_le_bytes()); // mp max
            m.raw[0x10A..0x10C].copy_from_slice(&0u16.to_le_bytes()); // mp cur
        }
        // R2 installs the text box; phase 0 hands to the fade block, so seat
        // the prompt phase directly and confirm.
        w.set_pad(crate::input::PadButton::R2.mask());
        let _ = w.tick();
        {
            let p = &mut w.world_map.ctrl.as_mut().unwrap().panels;
            assert_eq!(p.kind, Some(PanelActorKind::TextBox));
            assert_eq!(p.phase, 1, "installed straight at the prompt");
        }
        w.set_pad(crate::input::PadButton::Cross.mask());
        let _ = w.tick();
        for m in w.party.roster.members.iter() {
            assert_eq!(u16::from_le_bytes([m.raw[0x106], m.raw[0x107]]), 300);
            assert_eq!(u16::from_le_bytes([m.raw[0x10A], m.raw[0x10B]]), 80);
        }
    }

    /// The travel art has to move the *player actor*, and it has to move it
    /// back to where the screen was opened - not to where the player is when
    /// the dwell ends.
    #[test]
    fn the_travel_art_warps_the_player_actor_back_to_the_frozen_tile() {
        let mut w = world_on_the_overworld();
        w.spawn_actor(0).active = true;
        w.player_actor_slot = Some(0);
        w.seat_player_at_tile(20, 30);
        w.set_pad(0);
        let _ = w.tick();
        // Freeze the return point, then teleport the player somewhere else and
        // let the art run.
        {
            let p = &mut w.world_map.ctrl.as_mut().unwrap().panels;
            assert_eq!(p.visited.len(), 1, "the idle tick recorded the tile");
            p.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x1A);
        }
        w.seat_player_at_tile(200, 5);
        for _ in 0..400 {
            w.set_pad(0);
            let _ = w.tick();
            if !w.world_map.ctrl.as_ref().unwrap().panels.is_active() {
                break;
            }
        }
        let slot = w.player_actor_slot.expect("player installed") as usize;
        let a = &w.actors[slot];
        assert_eq!(a.move_state.world_x, ((20 << 7) + 0x40) as i16);
        assert_eq!(a.move_state.world_z, ((30 << 7) + 0x40) as i16);
    }

    /// The flag picker has to commit into the world's own system flag bank,
    /// which is what the field VM reads - not into a private copy.
    #[test]
    fn the_flag_window_commits_into_the_worlds_system_flag_bank() {
        let mut w = world_on_the_overworld();
        w.system_flag_set(0x0003);
        {
            let p = &mut w.world_map.ctrl.as_mut().unwrap().panels;
            p.flag_desc = FlagWindowDescriptor {
                count: 8,
                first_visible: 0,
                rows: 4,
                base_flag: 0,
            };
        }
        w.set_pad(crate::input::PadButton::R1.mask());
        let _ = w.tick();
        assert!(
            !w.system_flag_test(0x0003),
            "phase 0's range clear reached the world bank"
        );
        w.set_pad(crate::input::PadButton::Down.mask());
        let _ = w.tick();
        w.set_pad(crate::input::PadButton::Cross.mask());
        let _ = w.tick();
        let picked = w.world_map.ctrl.as_ref().unwrap().panels.cursor;
        assert!(
            w.system_flag_test(picked as u16),
            "the confirm set flag {picked} in the world bank"
        );
    }

    /// The field-to-battle transition suppresses the readout on every host:
    /// PROT 0979 loads into slot A over the field overlay, so the HUD code
    /// is not resident while the intro plays.
    #[test]
    fn the_battle_transition_suppresses_the_field_hud() {
        use crate::world::{SceneMode, World};
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_encounter_bracket();
        assert!(!field_hud_suppressed(&world, false), "idle field draws");
        let session = world
            .encounters
            .session
            .as_mut()
            .expect("bracket installed");
        assert!(session.trigger_with(crate::encounter::EncounterRoll {
            formation_id: 1,
            row_index: 0,
            roll_q8: 0,
        }));
        assert!(field_battle_transition_active(&world));
        assert!(field_hud_suppressed(&world, false), "the intro owns slot A");
    }
}
