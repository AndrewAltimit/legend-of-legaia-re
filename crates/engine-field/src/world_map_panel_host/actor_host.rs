//! `PanelActorHost`: the panel actor pool, its per-frame step and the flag / travel environment it reads.

use super::*;

/// Which panel actor is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelActorKind {
    /// `FUN_801ED308` - screen brightness fade / flash.
    FadeFlash,
    /// `FUN_801ED590` - the sub-list open / close picker.
    SubList,
    /// `FUN_801EDF00` - the return-to-title soft reset.
    SoftReset,
    /// `FUN_801EE5D4` - the screen-fill fade transition.
    FillFade,
    /// `FUN_801EE90C` - the yes/no text-box dispatcher.
    TextBox,
    /// `FUN_801EF014` - the story-flag window picker.
    FlagWindow,
    /// `FUN_801EE094` / `FUN_801EE328` - a travel art.
    TravelArt(TravelArt),
    /// `FUN_801F2134` - the close tick, slot `0` of `PTR_FUN_801F33B4`.
    ///
    /// Never installed by a chord: it is what an exit that hands back to
    /// handler id `0` runs next (the fill fade's case 4 at `0x801EE8EC`), so
    /// [`PanelActorHost::retire`] follows that one id instead of dropping
    /// the actor.
    CloseTick,
    /// `FUN_801F20B0` - the panel-install draw tick, slot `0x1A`. The
    /// sub-list, text-box and flag-window exits hand back to it: its state
    /// `0` runs the panel script at `0x801F2A88`
    /// ([`crate::submode_hub_actors::PANEL_DRAW_TICK`]), then it pumps and
    /// clears the completion gate like the close tick.
    DrawTick,
}

impl PanelActorKind {
    /// The phase [`PanelActorHost::install`] seeds this actor at.
    ///
    /// Everything starts at `0` except the text box. `FUN_801EE90C`'s phase 0
    /// is not its prompt - it jumps straight to the fill-fade block at phase
    /// 10, which walks 11..13 and parks at **14**, an arm that only clears
    /// `scene[+0x3E]` and has no exit. That is retail: phase 0 is the arrival
    /// path for a caller that installs the actor *mid*-transition, and the
    /// release comes from the scene manager watching `scene[+0x3E]`, which
    /// this engine does not model. Seeding the prompt phase directly is what
    /// makes the yes/no box the thing the actor runs.
    pub fn entry_phase(self) -> i16 {
        match self {
            PanelActorKind::TextBox => 1,
            _ => 0,
        }
    }
}

/// A world map the party has stood on, and the tile they left it at.
///
/// Retail keeps these as `0x10`-byte records in the buffer `FUN_80019788`
/// returns, keyed by the map name at `+0xC`; the travel-art resolve phase
/// scans them for the party's current map and warps to the stored tile. The
/// engine keeps the same `(map id, tile)` pair, recorded by the world-map tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitedMap {
    /// The map id the scan compares against `0x80084628`.
    pub map_id: u32,
    /// Stored tile X (`0x80084624`).
    pub tile_x: i32,
    /// Stored tile Z (`0x8008462C`).
    pub tile_z: i32,
}

/// Story-flag access the flag-window picker needs (`FUN_8003CE64` test,
/// `FUN_8003CE08` set, `FUN_8003CE34` range clear).
pub trait PanelFlagStore {
    fn flag_test(&self, id: i32) -> bool;
    fn flag_set(&mut self, id: i32);
    fn flag_clear(&mut self, id: i32);
}

/// The two world reads a travel art takes each frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TravelEnv {
    /// The player actor's live Y (`+0x16`), which Rula's lift moves.
    /// `None` leaves the lift on the kernel's own running Y (a host with no
    /// player actor).
    pub player_y: Option<i16>,
    /// `FUN_8003CE64(0x0B)`: the opener program queued at phase 0 is still
    /// running (it clears flag `0x0B` in its state 4).
    pub effect_busy: bool,
}

/// What one host frame produced, for the caller to apply and a renderer to
/// draw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelFrame {
    /// SFX ring producer calls raised this frame, in call order - the
    /// sub-list's `FUN_80035BD0(0x20)` (`0x801ED65C`) and the text box's
    /// `FUN_80035BD0(0)` + `FUN_80035B50(0x25)` (`0x801EE9AC` / `0x801EE9B4`).
    pub sfx: Vec<legaia_engine_system::sound_state::SfxRingOp>,
    /// Every [`ActorExit`] a terminal arm asked for. Recorded, not dispatched -
    /// see the module docs.
    pub exits: Vec<ActorExit>,
    /// Brightness level the fade/flash actor wants pushed to the display.
    pub brightness: Option<i32>,
    /// The records screen's slide y, on frames the soft-reset actor draws it.
    pub records_y: Option<i32>,
    /// The soft-reset actor asked for the white fade, in frames.
    pub white_fade: Option<i32>,
    /// The soft-reset actor reached its executable reload. The engine records
    /// it and does not act on it.
    pub reload_executable: bool,
    /// The text-box confirm arm asked for the party HP/MP restore.
    pub restore_party: bool,
    /// Story flags the picker set / cleared this frame.
    pub flags_set: Vec<i32>,
    /// The travel art resolved a destination; the caller warps the player.
    pub warp: Option<TravelDestination>,
    /// The travel-art scan missed and the actor parked in its diagnostic phase.
    pub travel_unfound: bool,
    /// The travel art's phase 0 asked for its opener: the caller sets flag
    /// `0x0B` and spawns scripted-scene program `n` (`FUN_801D5A24(n)`).
    pub travel_queue_program: Option<u16>,
    /// Rula's lift moved the player to this Y this frame, raising the
    /// player's `+0x10` bit 0 and scratchpad `0x1F800394` bit 24.
    pub lift_player_y: Option<i16>,
    /// Rula's lift ended: clear the post-warp pad hold `_DAT_8007B6B4`.
    pub clear_warp_hold: bool,
    /// Riremito's resolve: restore the player's render scale `+0x72` and
    /// clear its `+0x10` bit `0x200000`.
    pub restore_player: bool,
    /// The actor retired this frame.
    pub retired: bool,
    /// The sub-list took its state-3 hand-off (row 1 confirm).
    pub hand_off: bool,
    /// The fade/flash actor performed retail's `FUN_801D841C` hand-off this
    /// frame. A host that owns the memory-card UI opens it here and answers
    /// with [`PanelActorHost::release_flash_with`]; the panel host keeps the
    /// same record in [`PanelActorHost::save_screen`] meanwhile.
    pub save_screen: Option<SaveScreenHandoff>,
}

/// The panel-actor screen: one installed actor, the window system under it,
/// and the globals its phases read.
#[derive(Debug, Clone)]
pub struct PanelActorHost {
    /// The window system.
    pub windows: PanelWindowHost,
    /// The installed actor, if any.
    pub kind: Option<PanelActorKind>,
    /// `ctx[+0x54]` - the phase halfword.
    pub phase: i16,
    /// `ctx[+0x9E]` - the per-actor frame counter.
    pub timer: i16,
    /// `ctx[+0x50]` - the handler id the terminal arms save.
    pub handler_id: u16,
    /// `_DAT_8007B440` - the brightness accumulator.
    pub brightness: i32,
    /// `_DAT_8007B43C` - the flash counter.
    pub flash_counter: i32,
    /// `_DAT_8007B910` - the live **audio** level the sub-list halves and
    /// doubles. Seeded `0xD7` beside its persistent reference `_DAT_8008457C`
    /// by the cold reset `FUN_8001FFA4`, and halved into libsnd's `0..0x7F`
    /// range by every reader. Not screen brightness - that is `brightness`
    /// above.
    pub audio_level: i32,
    /// `_DAT_8007BB88` - the shared list cursor / picker selection.
    pub cursor: i32,
    /// `_DAT_8007BB9C` - the row the flag scan remembered on entry.
    pub remembered_row: i32,
    /// `_DAT_8007BB80` - the global input lock.
    pub input_locked: bool,
    /// `_DAT_801F35B8` - the records-screen slide counter.
    pub slide: i32,
    /// `_DAT_8007B450` - the op-`0x49` descriptor the flag window reads.
    pub flag_desc: FlagWindowDescriptor,
    /// `FUN_8003CF04` - true while a staged load is still running.
    pub load_pending: bool,
    /// The live tint triple `0x8007BF5D..5F` and its saved copy.
    pub tint: [u8; 3],
    /// The saved tint at `0x8007B634..636`.
    pub saved_tint: [u8; 3],
    /// `scene[+0x2E]` - the hand-back sentinel every [`ActorExit`] clears.
    /// Distinct from [`Self::scene_field_3e`]; the exit arms write this one
    /// and the `ClearSceneField3E` arms write that one.
    pub scene_field_2e: i16,
    /// `scene[+0x40]` - where an [`ActorExit`] parks the retiring actor's old
    /// handler id, for the dispatcher that would pick it back up.
    pub scene_field_40: u16,
    /// `scene[+0x3E]`, zeroed by two of the actors' own `case 5` arms.
    pub scene_field_3e: i16,
    /// `scene_obj[+0x10]`, whose bit `0x0008_0000` the fill-fade sets.
    pub scene_obj_flags: u32,
    /// How many times `FUN_80031D00` (the text-actor list tick) was asked for.
    pub text_actor_ticks: u64,
    /// The travel-art actor, when one is installed.
    pub travel: Option<TravelArtActor>,
    /// What the travel art reads off the world each frame - the caller
    /// stores it before [`Self::tick`].
    pub travel_env: TravelEnv,
    /// The visited-map table the travel art scans.
    pub visited: Vec<VisitedMap>,
    /// The party HUD's idle countdown (`_DAT_801F348C`).
    pub hud_timer: i16,
    /// The HUD's cached player position (`_DAT_801F3488` / `_DAT_801F348A`).
    pub hud_cached_pos: Option<(i16, i16)>,
    /// The last party-HUD decision, for a renderer.
    pub hud: Option<HudDecision>,
    /// The save-screen hand-off, while one is outstanding.
    ///
    /// Set by the fade/flash actor's phase-1 arm (retail's `FUN_801D841C`
    /// spawn) and cleared when the flash is released. Retail's equivalent is
    /// the live node on the pool at `_DAT_8007C34C`: it exists for exactly the
    /// window in which the field overlay is not resident, which is exactly the
    /// window this actor is parked in.
    pub save_screen: Option<SaveScreenHandoff>,
}

impl Default for PanelActorHost {
    fn default() -> Self {
        Self {
            windows: PanelWindowHost::new(),
            kind: None,
            phase: 0,
            timer: 0,
            handler_id: 0,
            brightness: 0,
            flash_counter: 0,
            audio_level: 0xD7,
            cursor: 0,
            remembered_row: 0,
            input_locked: false,
            slide: 0,
            flag_desc: FlagWindowDescriptor {
                count: 16,
                first_visible: 0,
                rows: 8,
                base_flag: 0,
            },
            load_pending: false,
            tint: [0; 3],
            saved_tint: [0; 3],
            scene_field_2e: 0,
            scene_field_40: 0,
            scene_field_3e: 0,
            scene_obj_flags: 0,
            text_actor_ticks: 0,
            travel: None,
            travel_env: TravelEnv::default(),
            visited: Vec::new(),
            hud_timer: 0,
            hud_cached_pos: None,
            hud: None,
            save_screen: None,
        }
    }
}

impl PanelActorHost {
    /// A fresh host with no actor installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an actor currently owns the frame.
    pub fn is_active(&self) -> bool {
        self.kind.is_some()
    }

    /// Install an actor at phase 0. Replaces whatever was running.
    ///
    /// `handler_id` is what the terminal arms save into `scene[+0x40]`; hosts
    /// that do not model the retail handler table can pass anything.
    pub fn install(&mut self, kind: PanelActorKind, handler_id: u16) {
        self.kind = Some(kind);
        self.phase = kind.entry_phase();
        self.timer = 0;
        self.handler_id = handler_id;
        self.cursor = 0;
        self.save_screen = None;
        if let PanelActorKind::TravelArt(art) = kind {
            self.travel = Some(TravelArtActor::new(art));
        } else {
            self.travel = None;
        }
    }

    /// Force the installed actor off the screen.
    ///
    /// The **port's** escape hatch, not retail's. Several of these machines
    /// have arms that park rather than exit - the text box's phase 14, the
    /// fade/flash's phase 3, the soft reset's phase 3 - because retail's
    /// release comes from the scene manager, which the engine does not model,
    /// or from a `PTR_FUN_801F33B4` slot the table read does not reach (see
    /// the module docs). Without this a parked actor would wedge the screen
    /// for the rest of the session.
    ///
    /// Returns whether anything was dismissed.
    pub fn dismiss(&mut self) -> bool {
        if self.kind.is_none() {
            return false;
        }
        self.kind = None;
        self.travel = None;
        self.save_screen = None;
        self.phase = 0;
        self.timer = 0;
        true
    }

    /// Release a brightness flash parked at the hold phase
    /// ([`crate::world_map_panel_actors::FADE_FLASH_HOLD_PHASE`]),
    /// the way retail's menu overlay does when its save/load UI finishes.
    ///
    /// The release is **gated on an outstanding save screen** because that is
    /// the only thing that writes `_DAT_8007B43C` in retail: the counter's five
    /// seeds and its `+3` completion step all live inside the menu overlay's
    /// save-side screens (`FUN_801DC6B4` and the two completion panels at
    /// `0x801D8A58` / `0x801D8B98`), and that overlay is only resident because
    /// [`FadeFlashEffect::SpawnSaveScreen`] put the driver actor on the pool.
    /// Nothing else in the corpus stores to the address.
    ///
    /// Returns whether a parked flash was released.
    pub fn release_flash_with(&mut self, outcome: SaveScreenOutcome) -> bool {
        if self.kind != Some(PanelActorKind::FadeFlash) || self.save_screen.is_none() {
            return false;
        }
        self.save_screen = None;
        // The menu side pins the level as well as the counter, so the field
        // side resumes at full white however long the UI took.
        self.brightness = BRIGHTNESS_MAX;
        self.flash_counter = outcome.release_counter();
        true
    }

    /// [`Self::release_flash_with`] on the save-side completion screen, which
    /// is the outcome the engine's own chord stands in for.
    pub fn release_flash(&mut self) -> bool {
        self.release_flash_with(SaveScreenOutcome::HandlerA)
    }

    /// Record that the party is standing on `map_id` at `(tile_x, tile_z)`.
    ///
    /// One record per map id - a revisit updates the stored tile, which is
    /// what makes the travel art return the party where it left.
    pub fn note_visit(&mut self, map_id: u32, tile_x: i32, tile_z: i32) {
        match self.visited.iter_mut().find(|v| v.map_id == map_id) {
            Some(v) => {
                v.tile_x = tile_x;
                v.tile_z = tile_z;
            }
            None => self.visited.push(VisitedMap {
                map_id,
                tile_x,
                tile_z,
            }),
        }
    }

    /// Retire the installed actor on a terminal arm's [`ActorExit`].
    ///
    /// The four stores are retail's and are made by [`ActorExit::apply`] - the
    /// `scene[+0x2E]` sentinel, the old handler id parked in `scene[+0x40]`,
    /// the new id into `ctx[+0x50]` and the phase reset. What is the port's is
    /// only what happens *after*: retail leaves the actor installed for the
    /// handler dispatcher to pick back up off the new id, and this host drops
    /// it instead, recording the pair in [`PanelFrame::exits`].
    ///
    /// The ids this host *does* follow are the ones whose bodies are ported:
    /// `0`, the close tick ([`PanelActorKind::CloseTick`],
    /// [`crate::submode_hub_actors::close_tick`]), which the actor stays
    /// installed for and retires when the tick clears the completion gate;
    /// and `0x29` / `0x2B`, the two travel arts the fade / flash's exits name
    /// (`PTR_FUN_801F33B4[0x29] = FUN_801EE094`, `[0x2B] = FUN_801EE328`,
    /// read off the PROT 0897 image), which continue as
    /// [`PanelActorKind::TravelArt`] the way retail's dispatcher would run
    /// them next; and `0x1A`, the draw tick `FUN_801F20B0`
    /// ([`PanelActorKind::DrawTick`]) the sub-list, text-box and flag-window
    /// exits hand back to. Every other id is dropped and recorded.
    fn retire(&mut self, frame: &mut PanelFrame, exit: ActorExit) {
        frame.exits.push(exit);
        exit.apply(
            &mut self.scene_field_2e,
            &mut self.scene_field_40,
            &mut self.handler_id,
            &mut self.phase,
        );
        self.travel = None;
        self.save_screen = None;
        self.timer = 0;
        if exit.next_handler == crate::submode_hub_actors::slot::CLOSE_TICK {
            self.kind = Some(PanelActorKind::CloseTick);
            return;
        }
        if exit.next_handler == crate::submode_hub_actors::slot::DRAW_TICK {
            self.kind = Some(PanelActorKind::DrawTick);
            return;
        }
        if let Some(art) = TravelArt::for_handler_id(exit.next_handler) {
            self.install(PanelActorKind::TravelArt(art), exit.next_handler);
            return;
        }
        frame.retired = true;
        self.kind = None;
    }

    /// One frame of the close tick (`FUN_801F2134`) under the dispatcher's
    /// retire test (`FUN_801F159C`, which re-reads `scene[+0x3E]` after the
    /// handler returns).
    ///
    /// The port does not model the completion gate being raised - nothing
    /// in this host sets `scene[+0x3E]` - so the gate is taken as up while
    /// the actor is installed, which is the only state in which retail's
    /// dispatcher would still be running it. The tick's own state `0` closes
    /// every panel (`jal 0x80035A4C`, the window-script `CloseAll`), every
    /// state ticks the text actors (`jal 0x80031D00`), and the gate is cleared
    /// only while the input lock `_DAT_8007BB80` is down
    /// (`0x801F2184..0x801F21A0`) - so a locked frame keeps the actor up.
    fn tick_close_tick(&mut self, frame: &mut PanelFrame) {
        self.tick_hub_tail(frame, crate::submode_hub_actors::close_tick);
    }

    /// One frame of the draw tick (`FUN_801F20B0`), under the same retire
    /// test as [`Self::tick_close_tick`]; its state `0` installs the panel
    /// script at `0x801F2A88` instead of closing every panel.
    fn tick_draw_tick(&mut self, frame: &mut PanelFrame) {
        self.tick_hub_tail(frame, crate::submode_hub_actors::draw_tick);
    }

    /// The close / draw tick pair's shared host: run `body` over a hub actor
    /// mirroring this one, apply its panel actions, and retire when it clears
    /// the completion gate.
    fn tick_hub_tail(
        &mut self,
        frame: &mut PanelFrame,
        body: fn(
            &mut crate::submode_hub_actors::HubActor,
            &crate::submode_hub_actors::HubEnv,
            &mut crate::submode_hub_actors::HubGrid,
        ) -> crate::submode_hub_actors::HubFrame,
    ) {
        use crate::submode_hub_actors::{HubAction, HubActor, HubEnv, HubGrid};
        let mut actor = HubActor {
            state: self.handler_id,
            sub: self.phase,
            ..HubActor::default()
        };
        let env = HubEnv {
            input_blocked: i32::from(self.input_locked),
            ..HubEnv::default()
        };
        let mut grid = HubGrid {
            done_gate: 1,
            ..HubGrid::default()
        };
        let out = body(&mut actor, &env, &mut grid);
        self.phase = actor.sub;
        for action in &out.actions {
            match action {
                HubAction::CloseAllPanels => self.windows.apply(PanelEffect::CloseAll),
                HubAction::InstallPanel(va) => {
                    self.windows.run_script(*va);
                }
                HubAction::DrawPump => self.text_actor_ticks += 1,
                _ => {}
            }
        }
        if grid.done_gate == 0 {
            self.scene_field_3e = 0;
            frame.retired = true;
            self.kind = None;
            self.phase = 0;
        }
    }

    fn cursor_pad(pad_held: u16, pad_edge: u16) -> CursorPad {
        CursorPad {
            held: u32::from(pad_held),
            pressed: u32::from(pad_edge),
            action_a_mask: PANEL_ACTION_A,
            action_b_mask: PANEL_ACTION_B,
        }
    }

    /// Advance the installed actor one frame.
    ///
    /// `pad_edge` / `pad_held` are the **packed** words ([`packed_pad`]);
    /// `frame_delta` is the adaptive frame-delta byte `_DAT_1F800393`;
    /// `flags` is the story-flag bank the picker reads and writes.
    ///
    /// Returns what the frame produced. An idle host returns the default.
    pub fn tick(
        &mut self,
        pad_edge: u16,
        pad_held: u16,
        frame_delta: u8,
        flags: &mut dyn PanelFlagStore,
    ) -> PanelFrame {
        let mut frame = PanelFrame::default();
        let Some(kind) = self.kind else {
            return frame;
        };
        let pad = Self::cursor_pad(pad_held, pad_edge);
        let delta = i32::from(frame_delta);
        match kind {
            PanelActorKind::FadeFlash => self.tick_fade_flash(delta, &mut frame),
            PanelActorKind::SubList => self.tick_sub_list(pad, &mut frame),
            PanelActorKind::SoftReset => self.tick_soft_reset(delta, pad_held, &mut frame),
            PanelActorKind::FillFade => self.tick_fill_fade(frame_delta, &mut frame),
            PanelActorKind::TextBox => self.tick_text_box(pad, pad_edge, frame_delta, &mut frame),
            PanelActorKind::FlagWindow => self.tick_flag_window(pad, flags, &mut frame),
            PanelActorKind::TravelArt(_) => self.tick_travel_art(frame_delta, &mut frame),
            PanelActorKind::CloseTick => self.tick_close_tick(&mut frame),
            PanelActorKind::DrawTick => self.tick_draw_tick(&mut frame),
        }
        frame
    }

    fn tick_fade_flash(&mut self, frame_delta: i32, frame: &mut PanelFrame) {
        let (phase, level, counter, out) = fade_flash_tick(
            self.phase,
            FadeFlashInput {
                frame_delta,
                level: self.brightness,
                flash_counter: self.flash_counter,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.brightness = level;
        self.flash_counter = counter;
        for e in out {
            match e {
                FadeFlashEffect::ApplyBrightness(v) => frame.brightness = Some(v),
                FadeFlashEffect::CaptureAndClearTint => {
                    self.saved_tint = self.tint;
                    self.tint = [0; 3];
                }
                FadeFlashEffect::SpawnSaveScreen => {
                    // `FUN_801D841C` verbatim: the descriptor / pool pair and
                    // the one field it writes on the returned actor.
                    let spawn = crate::save_screen_spawn::save_screen_spawn();
                    let handoff = SaveScreenHandoff {
                        descriptor: spawn.descriptor,
                        pool: spawn.pool,
                        is_save: spawn.field_5c != 0,
                    };
                    self.save_screen = Some(handoff);
                    frame.save_screen = Some(handoff);
                }
                FadeFlashEffect::RestoreTint => self.tint = self.saved_tint,
                FadeFlashEffect::ClearSceneField3E => self.scene_field_3e = 0,
                FadeFlashEffect::Exit(x) => self.retire(frame, x),
            }
        }
    }

    fn tick_sub_list(&mut self, pad: CursorPad, frame: &mut PanelFrame) {
        let (phase, cursor, out) = sub_list_tick(
            self.phase,
            SubListInput {
                input_locked: self.input_locked,
                cursor: self.cursor,
                pad,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.cursor = cursor;
        for e in out {
            match e {
                SubListEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                SubListEffect::ScaleAudioLevel { shift_right } => {
                    self.audio_level = if shift_right {
                        self.audio_level >> 1
                    } else {
                        self.audio_level << 1
                    };
                }
                // `jal 0x80035BD0` at `0x801ED65C`: the confirm overwrites
                // the last-written ring slot rather than pushing.
                SubListEffect::PlaySfx(s) => {
                    frame
                        .sfx
                        .push(legaia_engine_system::sound_state::SfxRingOp::ReplaceLast(
                            s as i16,
                        ))
                }
                SubListEffect::ClearWindowDescriptor => {
                    self.flag_desc = FlagWindowDescriptor::default()
                }
                SubListEffect::HandOff => frame.hand_off = true,
                SubListEffect::Exit(x) => self.retire(frame, x),
                SubListEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_soft_reset(&mut self, frame_delta: i32, pad_held: u16, frame: &mut PanelFrame) {
        use crate::world_map_panel_actors::{SoftResetEffect, SoftResetInput};
        let (phase, slide, out) = soft_reset_tick(
            self.phase,
            SoftResetInput {
                frame_delta,
                slide: self.slide,
                pad: u32::from(pad_held),
            },
        );
        self.phase = phase;
        self.slide = slide;
        for e in out {
            match e {
                SoftResetEffect::ArmReset => {}
                SoftResetEffect::DrawRecords { y } => frame.records_y = Some(y),
                SoftResetEffect::WhiteFade { frames } => frame.white_fade = Some(frames),
                SoftResetEffect::ReloadExecutable => frame.reload_executable = true,
                SoftResetEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_fill_fade(&mut self, frame_delta: u8, frame: &mut PanelFrame) {
        let (phase, timer, out) = fill_fade_tick(
            self.phase,
            FillFadeInput {
                frame_delta: i16::from(frame_delta),
                timer: self.timer,
                input_locked: self.input_locked,
                load_pending: self.load_pending,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.timer = timer;
        for e in out {
            self.apply_fill_fade(e, frame);
        }
    }

    fn apply_fill_fade(&mut self, e: FillFadeEffect, frame: &mut PanelFrame) {
        match e {
            FillFadeEffect::RunPanelScript(va) => {
                self.windows.run_script(va);
            }
            FillFadeEffect::PostFillPrim => {}
            FillFadeEffect::SpawnSubActorAndCaptureTint => {
                self.saved_tint = self.tint;
                self.tint = [0; 3];
            }
            FillFadeEffect::QueueDmaAndRestoreTint => self.tint = self.saved_tint,
            FillFadeEffect::SetSceneFlagBit => self.scene_obj_flags |= 0x0008_0000,
            FillFadeEffect::TickTextActors => self.text_actor_ticks += 1,
            FillFadeEffect::Exit(x) => self.retire(frame, x),
            FillFadeEffect::ClearSceneField3E => self.scene_field_3e = 0,
        }
    }

    fn tick_text_box(
        &mut self,
        pad: CursorPad,
        pad_edge: u16,
        frame_delta: u8,
        frame: &mut PanelFrame,
    ) {
        // Retail's dismiss test is the newly-pressed word masked with the two
        // configured action buttons; the caller pre-reduces it to a bool.
        let dismiss = u32::from(pad_edge) & (PANEL_ACTION_A | PANEL_ACTION_B) != 0;
        let (phase, cursor, timer, out) = text_box_tick(
            self.phase,
            TextBoxInput {
                input_locked: self.input_locked,
                cursor: self.cursor,
                pad,
                dismiss_pressed: dismiss,
                fade: FillFadeInput {
                    frame_delta: i16::from(frame_delta),
                    timer: self.timer,
                    input_locked: self.input_locked,
                    load_pending: self.load_pending,
                    handler_id: self.handler_id,
                },
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.cursor = cursor;
        self.timer = timer;
        for e in out {
            match e {
                // `FUN_80035BD0(0)` (`0x801EE9AC`) then `FUN_80035B50(0x25)`
                // (`0x801EE9B4`): the move cue overwrites, the restore pushes.
                TextBoxEffect::PlaySfx(s) => {
                    frame
                        .sfx
                        .push(if s == crate::world_map_panel_actors::TEXT_BOX_MOVE_SFX {
                            legaia_engine_system::sound_state::SfxRingOp::ReplaceLast(s as i16)
                        } else {
                            legaia_engine_system::sound_state::SfxRingOp::Push(s as i16)
                        })
                }
                TextBoxEffect::RestoreParty => frame.restore_party = true,
                TextBoxEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                TextBoxEffect::Exit(x) => self.retire(frame, x),
                TextBoxEffect::Fade(f) => self.apply_fill_fade(f, frame),
                TextBoxEffect::ClearSceneField3E => self.scene_field_3e = 0,
                TextBoxEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_flag_window(
        &mut self,
        pad: CursorPad,
        flags: &mut dyn PanelFlagStore,
        frame: &mut PanelFrame,
    ) {
        let input = FlagWindowInput {
            desc: self.flag_desc,
            input_locked: self.input_locked,
            selection: self.cursor,
            remembered: self.remembered_row,
            pad,
            handler_id: self.handler_id,
        };
        // The scan reads the bank; the writes below take it mutably, so the
        // immutable reborrow is scoped to the call.
        let (phase, selection, remembered, out) = {
            let read: &dyn PanelFlagStore = &*flags;
            flag_window_tick(self.phase, input, |id| read.flag_test(id))
        };
        self.phase = phase;
        self.cursor = selection;
        self.remembered_row = remembered;
        for e in out {
            match e {
                FlagWindowEffect::ClearRange { base, count } => {
                    for i in 0..i32::from(count) {
                        flags.flag_clear(base + i);
                    }
                }
                FlagWindowEffect::SizePanel { y, height } => {
                    let i = crate::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX;
                    if let Some(d) = self.windows.descriptors.get_mut(i) {
                        d.y = y as i16;
                        d.height = height as i16;
                    }
                }
                FlagWindowEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                FlagWindowEffect::SetFlag(id) => {
                    flags.flag_set(id);
                    frame.flags_set.push(id);
                }
                FlagWindowEffect::Exit(x) => self.retire(frame, x),
                FlagWindowEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_travel_art(&mut self, frame_delta: u8, frame: &mut PanelFrame) {
        let Some(mut actor) = self.travel else {
            self.kind = None;
            return;
        };
        // The current map is the last one recorded, which is where the party
        // is standing - the same `0x80084628` read the retail scan compares.
        let current = self.visited.last().map(|v| v.map_id).unwrap_or(0);
        let table: Vec<VisitedMap> = self.visited.clone();
        if let Some(y) = self.travel_env.player_y {
            actor.player_y = y;
        }
        let out = actor.tick(self.travel_env.effect_busy, i16::from(frame_delta), || {
            let idx = find_visited_map(table.len(), current, |i| table[i].map_id)?;
            let v = table[idx];
            Some(destination_for(idx, v.tile_x, v.tile_z))
        });
        self.travel = Some(actor);
        frame.travel_queue_program = out.queue_effect.map(|p| p as u16);
        frame.lift_player_y = out.lift_player_y;
        frame.clear_warp_hold = out.clear_warp_hold;
        frame.restore_player = out.restore_player;
        if out.spawn_flash {
            frame.brightness = Some(crate::world_map_panel_actors::BRIGHTNESS_MAX);
        }
        if out.unfound {
            frame.travel_unfound = true;
            self.kind = None;
            self.travel = None;
        }
        if let Some(dest) = out.destination {
            frame.warp = Some(dest);
            frame.retired = true;
            self.kind = None;
            self.travel = None;
        }
    }

    /// One frame of the field party HUD (`FUN_801D0D38`).
    ///
    /// Retail installs this in the field band's per-frame panel builder; the
    /// engine drives it from the overworld tick, where the party panel and the
    /// player marker both live. `pad_held` is the **packed** held word - the
    /// suppress mask is the packed D-pad, so a raw word suppresses nothing.
    ///
    /// `projected_y` is `None` when the host has no screen projection for the
    /// player, which is retail's own staged-load path and forces the low panel.
    pub fn tick_party_hud(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
    ) -> HudDecision {
        let rearm = self.hud_cached_pos.is_none();
        let stationary = match (self.hud_cached_pos, player_pos) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        let (timer, decision) = field_hud_tick(HudInput {
            hud_disabled,
            view_mode,
            pad: u32::from(pad_held),
            scratch_suppress: false,
            rearm,
            short_idle: false,
            timer: self.hud_timer,
            timer_delta,
            player_stationary: stationary,
            projected_y,
        });
        self.hud_timer = timer;
        self.hud_cached_pos = player_pos;
        self.hud = Some(decision);
        decision
    }
}
