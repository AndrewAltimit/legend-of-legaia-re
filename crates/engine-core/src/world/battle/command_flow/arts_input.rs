//! The Arts menu and command-string input: the menu tick, the input views, the
//! Spirit gauge view and the input open / tick.

use super::*;

impl World {
    /// Drive the open battle Arts submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`legaia_engine_battle::battle_arts::BattleArtsInput`] per
    /// frame. A confirmed row commits its direction string
    /// ([`Self::run_battle_art`]); the queue is built and the attack band
    /// armed at the caster's dispatch. Backing out reopens the command menu.
    pub(in crate::world) fn tick_battle_arts_menu(&mut self) {
        use legaia_engine_battle::battle_arts::{ArtsResolution, BattleArtsInput};
        use legaia_engine_system::input::PadButton;

        let Some(mut menu) = self.battle.arts_menu.take() else {
            return;
        };

        // Same validator-backed target rows as the command menu.
        let (party, monsters) = self.battle_target_rows();

        let ev = BattleArtsInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, party, monsters);

        match menu.resolved() {
            Some(ArtsResolution::Confirmed {
                art_index,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                // A saved-chain row is its directional string: the same
                // arrows the command input would have produced, run through
                // the same queue-builder.
                let sequence = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| a.sequence.clone())
                    .unwrap_or_default();
                self.run_battle_art(caster, &sequence, target_row, target_slot);
            }
            Some(ArtsResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle.arts_menu = Some(menu);
            }
        }
    }

    /// Open the retail-model **Arts command input** for `actor`: seed the
    /// AP pool from the acting character's AGL (retail `ctx+0x6DC` <-
    /// actor `+0x154`; [`legaia_engine_battle::arts_command_input::DEFAULT_POOL`] without
    /// stats), the four per-direction press costs (Left = the arm command
    /// `0x0C`, carrying the per-(character, weapon) `+0x74` byte from
    /// [`Self::battle_arm_costs`]; the others at the favored base), and the
    /// Triangle arts-list page count from the caster's loaded art catalog.
    ///
    /// PORT: FUN_801D0748 (state 0x50 arm)
    /// REF: FUN_801D388C
    /// `true` while a party member owns the pad in the retail-model Arts
    /// command input. Retail parks the party **status plate off-screen**
    /// for the whole session (its draws move to `y = 230`, below the
    /// 228-line display window - `docs/subsystems/minigame-muscle-dome.md`
    /// § Arts command input), so a host's battle-HUD strip reads this and
    /// emits nothing while it holds.
    pub fn arts_input_active(&self) -> bool {
        self.battle.arts_input.is_some()
    }

    /// The actor-table index of the party member entering commands, or
    /// `None` when no session is open. The party surface has two mutually
    /// exclusive forms - roster panels, and a full-width bar for the actor
    /// that owns the pad - so a host needs the *which*, not just the
    /// *whether*.
    pub fn arts_input_actor(&self) -> Option<u8> {
        self.battle.arts_input.as_ref().map(|s| s.actor)
    }

    /// Renderer-agnostic view of the open Arts command input, or `None`
    /// when no session is up. Both hosts build the pinned chrome from
    /// this and nothing else.
    ///
    /// A Muscle Dome leg runs the same entry screen (its command flow embeds
    /// the battle's entry session, `muscle_dome::DomeMenu`), so during one
    /// this is the dome fighter's entry - one view, one chrome, both kinds of
    /// fight.
    pub fn arts_input_view(
        &self,
    ) -> Option<legaia_engine_battle::arts_command_input::ArtsInputView<'_>> {
        if self.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
            return self.minigames.muscle_dome.as_ref()?.arts_input_view();
        }
        let s = self.battle.arts_input.as_ref()?;
        Some(legaia_engine_battle::arts_command_input::ArtsInputView {
            buffer: &s.buffer,
            spent: &s.spent,
            pennants: s.bar_commands(),
            pennant_spent: s.bar_spent(),
            pool: s.pool,
            pool_max: s.pool_max,
            costs: s.costs,
            chip_icons: {
                let roster = self.party_roster_slot(s.actor as usize);
                let equip = self
                    .party
                    .roster
                    .members
                    .get(roster)
                    .map(|r| r.equipment().slots)
                    .unwrap_or_default();
                legaia_engine_battle::arts_command_input::retail_chip_icons(
                    roster as u8 + 1,
                    &equip,
                )
            },
            // The right-hand plate reads the caster's **Spirit** gauge and
            // never moves during entry - the entry budget's visible form is
            // the bar. Without a live Spirit value the pool stands in.
            plate_value: self.spirit_gauge(s.actor).min(100) as u8,
            list_page: s.list_page,
            list_pages: s.list_pages,
            list_slide: s.list_slide,
            phase: (&s.phase).into(),
            status: self.raw_status_word(s.actor),
            pennant_glides: &s.pennant_glides,
            chip_slide: s.chip_slide,
        })
    }

    /// The **Spirit turn's** AP bar + AP plate, as the same view the
    /// arts-entry chrome draws from, or `None` outside a party member's
    /// Spirit action.
    ///
    /// Retail's Spirit dispatch arm raises the arts screen's own two gauge
    /// elements - placement records `0x0F` (the AP bar) and `0x52` (the AP
    /// plate) - and the spirit band then grows the bar to the extended gauge
    /// the next turn's entry pool will have, and climbs the plate by the
    /// Spirit the turn earns (`legaia_engine_battle_vm::battle_action`'s spirit
    /// band, [`legaia_engine_battle_vm::battle_action::BattleActionCtx::spirit_bar_width`] /
    /// `spirit_plate_value`). The Done band's tail unloads both. So the view
    /// is up from the band's entry (`0x46`) until the `0x51` fade-down's
    /// once-only UI teardown runs (the retail `0x51` captures of a Spirit turn
    /// still carry both), with no pennants and no chips (the entry-only half), the bar sized by
    /// the live width (`width + 6` is the gauge value the arts bar scales
    /// off) and the plate reading the live ramp.
    ///
    /// Both hosts draw it through the arts-entry chrome builders when no
    /// entry session is open ([`Self::arts_input_view`] takes precedence).
    pub fn spirit_gauge_view(
        &self,
    ) -> Option<legaia_engine_battle::arts_command_input::ArtsInputView<'static>> {
        use legaia_engine_battle::arts_command_input::{ArtsInputScreen, ArtsInputView};
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || self.battle.arts_input.is_some()
        {
            return None;
        }
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let state = self.battle_ctx.action_state;
        let up = (S::SpiritArtsEntry.as_byte()..=S::DoneCleanup.as_byte()).contains(&state)
            || (state == S::DoneFadeDown.as_byte() && self.battle_ctx.done_ui_torn_down == 0);
        if !up {
            return None;
        }
        let slot = self.battle_ctx.active_actor;
        if usize::from(slot) >= usize::from(self.party.party_count.max(1)) {
            return None;
        }
        let actor = self.actors.get(usize::from(slot))?;
        if actor.battle.action_category
            != legaia_engine_battle_vm::battle_action::ActionCategory::Spirit.as_byte()
        {
            return None;
        }
        let width = self.battle_ctx.spirit_bar_width;
        Some(ArtsInputView {
            buffer: &[],
            spent: &[],
            pennants: &[],
            pennant_spent: &[],
            pool: 0,
            pool_max: width
                .saturating_add(legaia_engine_battle_vm::battle_action::SPIRIT_BAR_WIDTH_BIAS)
                .max(0) as u16,
            costs: [legaia_engine_battle::arts_command_input::FAVORED_COST; 4],
            chip_icons: [0x0F, 0x0E, 0x11, 0x10],
            plate_value: self.battle_ctx.spirit_plate_value.clamp(0, 100) as u8,
            list_page: None,
            list_pages: 0,
            list_slide: 0,
            phase: ArtsInputScreen::Review,
            status: 0,
            pennant_glides: &[],
            chip_slide: None,
        })
    }

    /// Stand the open arts entry's Triangle list on `page` with its window's
    /// slide counter at `slide` - a retail capture's list as its RAM holds
    /// it (`_DAT_8007B458 / 5`, `0x801F4E0A`). A no-op without an entry, or
    /// for a page the character's learned count does not reach.
    pub fn seat_arts_list(&mut self, page: u8, slide: u8) {
        use legaia_engine_battle::arts_command_input::ARTS_LIST_SLIDE_MAX;
        let Some(s) = self.battle.arts_input.as_mut() else {
            return;
        };
        if page >= s.list_pages {
            return;
        }
        s.list_page = Some(page);
        s.list_row_page = page;
        s.list_slide = slide.min(ARTS_LIST_SLIDE_MAX);
    }

    /// The Triangle arts list's window while it is on screen: `(actor, row
    /// page, slide)` off the open entry, else off the window an ended entry
    /// left sliding out ([`legaia_engine_battle_loop::battle_state::BattleState::arts_list_window`]).
    /// `None` at slide `0`. On a dome leg it is the dome entry's window
    /// (actor `0`, the player's fighter), and only once the host has named
    /// whose learned list it pages over
    /// (`MuscleDomeSession::set_arts_list_owner`).
    pub fn arts_list_window(&self) -> Option<(u8, u8, u8)> {
        self.arts_list_source()
            .map(|(_, actor, page, slide)| (actor, page, slide))
    }

    /// [`Self::arts_list_window`] with the roster slot whose record the rows
    /// read: `(char slot, actor, row page, slide)`.
    fn arts_list_source(&self) -> Option<(usize, u8, u8, u8)> {
        if self.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
            let dome = self.minigames.muscle_dome.as_ref()?;
            let owner = dome.arts_list_owner()?;
            let (page, slide) = dome.arts_list_window()?;
            return Some((usize::from(owner), 0, page, slide));
        }
        let (actor, page, slide) = match self.battle.arts_input.as_ref() {
            Some(s) => s.list_window().map(|(page, slide)| (s.actor, page, slide)),
            None => self
                .battle
                .arts_list_window
                .map(|w| (w.actor, w.row_page, w.slide)),
        }?;
        Some((
            self.party_roster_slot(usize::from(actor)),
            actor,
            page,
            slide,
        ))
    }

    /// Step the window an ended entry left on screen one command-flow frame
    /// ([`legaia_engine_battle::arts_command_input::ArtsListWindow::step`]).
    pub(in crate::world) fn step_arts_list_window(&mut self) {
        let step = self.battle.anim_pass_vsyncs;
        if let Some(w) = self.battle.arts_list_window.as_mut()
            && !w.step(step)
        {
            self.battle.arts_list_window = None;
        }
    }

    /// The rows the Triangle arts list's window shows, in row order, or
    /// `None` while the window is off-screen ([`Self::arts_list_window`]) or
    /// the arts-name table is not installed ([`Self::install_menu_text`]).
    ///
    /// The list renderer `FUN_80034358` walks the acting character's learned
    /// ids (record `+0x186..`, count `+0x185`) from the pager's row offset
    /// (`_DAT_8007B458`, five rows a page), and for each scans the arts-name
    /// table for `[character, id]`. An id the table does not carry leaves its
    /// row empty (`None`): the row pitch still advances (`0x8003474C`).
    /// `0xC1 XX` substitutions in a name are spliced with party member
    /// `XX`'s name, as the text kernel's expander `FUN_80036514` does.
    ///
    /// PORT: FUN_80034358 (row model)
    pub fn arts_list_rows(
        &self,
    ) -> Option<Vec<Option<legaia_engine_battle::arts_command_input::ArtsListRow>>> {
        use legaia_engine_battle::arts_command_input::{ARTS_LIST_ROWS_PER_PAGE, ArtsListRow};
        let (char_slot, _, page, _) = self.arts_list_source()?;
        let table = self.menu.text.as_ref()?.arts.as_ref()?;
        let record = self.party.roster.members.get(char_slot)?;
        let skills = record.displayed_skills();
        let count = usize::from(skills.count).min(skills.ids.len());
        let halved = record.ability_bits()[5] & 0x08 != 0;
        let character = self.caster_character(char_slot as u8);
        let first = usize::from(page) * ARTS_LIST_ROWS_PER_PAGE;
        let rows = (first..count.min(first + ARTS_LIST_ROWS_PER_PAGE))
            .map(|i| {
                let id = skills.ids[i];
                let e = table
                    .iter()
                    .find(|e| e.character == character && e.index == id)?;
                let mut name = Vec::with_capacity(e.name_bytes.len());
                let mut b = e.name_bytes.iter().copied();
                while let Some(c) = b.next() {
                    if c == 0xC1 {
                        let who = b.next().unwrap_or(0);
                        name.extend_from_slice(self.party_name(usize::from(who)).as_bytes());
                    } else {
                        name.push(c);
                    }
                }
                Some(ArtsListRow {
                    name,
                    ap: if halved { e.ap >> 1 } else { e.ap },
                    glyphs: e.glyphs.clone(),
                })
            })
            .collect();
        Some(rows)
    }

    /// Where the Spirit turn's AP bar and AP plate sit relative to their rest
    /// seats while their raise glide runs ([`legaia_engine_battle_loop::battle_state::BattleState::spirit_gauge_glide`]):
    /// `(bar dy, plate dx)` in stage pixels, `(0, 0)` once landed. The bar
    /// rises from `y = 234` onto `192` and the plate slides from `x = 328`
    /// onto `208` (placement records `0x0F` / `0x52`, walked by
    /// `FUN_801D9BBC`).
    pub fn spirit_gauge_glide_offsets(&self) -> (i32, i32) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        let g = self.battle.spirit_gauge_glide.as_ref();
        (
            LogLaunch::seat_dy(g, 234, 192),
            LogLaunch::seat_dy(g, 328, 208),
        )
    }

    /// The `+0x16E` status word of the member the command ring is open for,
    /// `None` without a command session. The ring's Rot / Curse marks read
    /// it (`legaia_engine_battle_hud::battle_ring_marks`), as its refusals do
    /// ([`ring_arm_refused`]).
    pub fn battle_command_status_word(&self) -> Option<u16> {
        let cmd = self.battle.command.as_ref()?;
        Some(self.raw_status_word(cmd.actor))
    }

    pub(in crate::world) fn open_arts_command_input(&mut self, actor: u8) {
        use legaia_engine_battle::arts_command_input::{
            ARTS_LIST_ROWS_PER_PAGE, ArtsCommandInputSession, DEFAULT_POOL, FAVORED_COST,
        };
        let char_slot = self.party_roster_slot(actor as usize) as u8;
        // The pool is the battle actor's **live** gauge `+0x154`
        // (`801d4e10 lhu v0,0x154(v0)`), not the record's AGL: after a Spirit
        // turn the round boundary has extended it, and that longer gauge is
        // the whole point of the command. A battle actor the setup never
        // seeded (a synthetic fight) falls back to the record.
        let pool = self
            .actors
            .get(actor as usize)
            .map(|a| a.battle.agl)
            .filter(|&a| a > 0)
            .or_else(|| {
                self.party
                    .roster
                    .members
                    .get(char_slot as usize)
                    .map(|r| r.live_stats().agl)
                    .filter(|&a| a > 0)
            })
            .unwrap_or(DEFAULT_POOL);
        // Cost order = Command byte order (Left, Right, Down, Up) = the
        // runtime action slots `0xC..=0xF` the disc bytes are keyed by.
        let costs = self
            .battle
            .swing_costs
            .get(char_slot as usize)
            .copied()
            .unwrap_or([FAVORED_COST; 4]);
        let character = self.caster_character(char_slot);
        let n_arts = self
            .tables
            .art_records
            .iter()
            .filter(|((ch, _), rec)| *ch == character && !rec.commands.is_empty())
            .count();
        let pages = n_arts.div_ceil(ARTS_LIST_ROWS_PER_PAGE) as u8;
        // Retail preseeds the queue window from the character's auto command
        // string as the entry opens (`jal FUN_801DA34C` at `0x801D1734`, the
        // `sb 0x50` phase store in its delay slot); a bare confirm replays it.
        let preseed = self.preseed_arts_entry_string(actor);
        let loaded = preseed.len();
        // The Triangle list pages over the character's learned arts
        // (`record[+0x185]`), the count the retail pager reads.
        let learned = self
            .party
            .roster
            .members
            .get(char_slot as usize)
            .map(|r| r.displayed_skills().count);
        let mut session =
            ArtsCommandInputSession::new(actor, actor, pool, costs, pages).with_preseed(preseed);
        if let Some(n) = learned {
            session = session.with_list_rows(n);
        }
        // The gauge build cuts the window at the first command one full pool
        // cannot pay, zeroing that one byte (`sb zero,0x1df(a1)` at
        // `0x801D4DC4`).
        let kept = session.preseed.len();
        if kept < loaded
            && let Some(b) = self
                .actors
                .get_mut(usize::from(actor))
                .and_then(|a| a.battle.params.get_mut(kept))
        {
            *b = 0;
        }
        // The entry's window registration (`FUN_801DBB8C`) zeroes the slide
        // counter: an older window still sliding out is gone.
        self.battle.arts_list_window = None;
        self.battle.arts_input = Some(session);
    }

    /// Load `actor`'s auto command string into its `+0x1DF` window
    /// ([`Self::preseed_auto_command_string`]) and return it as the entry's
    /// command bytes (`Command::as_byte()`), for the arts input to replay.
    /// The saved window holds the entered arrows as the gauge wrote them -
    /// the swing bytes `0x0C..=0x0F` - so the string ends at its first byte
    /// outside that range.
    pub(super) fn preseed_arts_entry_string(&mut self, actor: u8) -> Vec<u8> {
        let n = self.preseed_auto_command_string(actor);
        let Some(a) = self.actors.get(usize::from(actor)) else {
            return Vec::new();
        };
        a.battle.params[..n]
            .iter()
            .map_while(|&b| swing_byte_command(b))
            .map(legaia_art::Command::as_byte)
            .collect()
    }

    /// Drive the open Arts command input one frame from [`World::input`].
    ///
    /// On a confirmed Begin the entered sequence is committed
    /// ([`Self::run_battle_art`]); retail's queue-builder runs over it at
    /// the caster's dispatch and the attack band stages the result. Backing
    /// out (empty buffer + Circle, or no valid target) reopens the command
    /// menu.
    pub(in crate::world) fn tick_battle_arts_input(&mut self) {
        use legaia_engine_battle::arts_command_input::{ArtsCommandPad, ArtsInputResolution};
        use legaia_engine_system::input::PadButton;

        let Some(mut session) = self.battle.arts_input.take() else {
            return;
        };
        let (party, monsters) = self.battle_target_rows();
        let mut ev = ArtsCommandPad {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
            triangle: self.input.just_pressed(PadButton::Triangle),
        };
        // A rotted limb's direction is refused during entry, with the buzz
        // cue (`legaia_engine_battle::arts_command_input::rot_blocks`).
        if matches!(
            session.phase,
            legaia_engine_battle::arts_command_input::ArtsInputPhase::Entering
        ) {
            let status = self.raw_status_word(session.actor);
            let mut refused = false;
            for (pressed, cmd) in [
                (&mut ev.left, legaia_art::Command::Left),
                (&mut ev.up, legaia_art::Command::Up),
                (&mut ev.down, legaia_art::Command::Down),
                (&mut ev.right, legaia_art::Command::Right),
            ] {
                if *pressed && legaia_engine_battle::arts_command_input::rot_blocks(status, cmd) {
                    *pressed = false;
                    refused = true;
                }
            }
            if refused {
                // `FUN_8004FCC8(ROT_REFUSED_CUE)` (`0x801D1EA0..0x801D1F78`):
                // the dispatcher stores `id - 1` on the ring.
                self.push_battle_ui_cue(super::ui_cues::BattleUiCue::Back);
            }
        }
        let was_entering = matches!(
            session.phase,
            legaia_engine_battle::arts_command_input::ArtsInputPhase::Entering
        );
        let was_targeting = matches!(
            session.phase,
            legaia_engine_battle::arts_command_input::ArtsInputPhase::Targeting { .. }
        );
        let phase_before = session.phase.clone();
        session.input(ev, party, monsters);
        // `FUN_801D0748` runs the arts-list driver `FUN_801D3444` after the
        // frame's input: the Triangle list window slides by the frame step.
        session.step_list_slide(self.battle.anim_pass_vsyncs);
        // Whatever ends the entry this frame leaves its window on screen.
        let left = legaia_engine_battle::arts_command_input::ArtsListWindow::left_by(&session);
        if let Some(cue) = super::ui_cues::arts_phase_cue(&phase_before, &session.phase, ev.cross) {
            self.push_battle_ui_cue(cue);
        }
        // The `0x5A` cursor the entry's confirm opened starts on the
        // member's current target ([`World::seat_attack_cursor`]).
        if !was_targeting
            && let legaia_engine_battle::arts_command_input::ArtsInputPhase::Targeting { picker } =
                &mut session.phase
        {
            self.seat_attack_cursor(session.actor, picker);
        }

        // Sparring tutorial: the entry's own two flow states. Leaving the
        // entry is retail's `0x50 -> 0x5A` (the confirm, or the exhausting
        // press), whose hook checks the hyper-arts drill against the swing
        // bytes the gauge wrote to `actor[+0x1DF..]`; the review's cancel is
        // `0x5A -> 0x50` and re-raises the entry hook. The commit is the
        // `110` validator with category `3` (an art is an Attack). Either
        // hook's rewind discards the entry and reopens the command menu.
        if self.battle.tutorial.is_some() {
            use legaia_engine_battle::arts_command_input::ArtsInputPhase as P;
            use legaia_engine_battle_menus::battle_flow::BattleFlowState as Flow;
            let rejected = match &session.phase {
                P::Entering if !was_entering => self.set_battle_flow(Flow::ArtsCommandEntry),
                P::Review | P::Targeting { .. } | P::Confirmed { .. } if was_entering => {
                    let mut buf = [0u8; 5];
                    for (dst, &b) in buf.iter_mut().zip(session.committed_string()) {
                        *dst = legaia_art::Command::from_byte(b)
                            .map_or(0, |c| c.as_action().as_byte());
                    }
                    if let Some(tut) = self.battle.tutorial.as_mut() {
                        tut.inputs.command_buffer = buf;
                        // The `Command` chip cleared the Auto flag.
                        tut.inputs.auto_attack = false;
                    }
                    self.set_battle_flow(Flow::TargetSelect)
                }
                _ => false,
            };
            let rejected = rejected
                || (matches!(session.phase, P::Confirmed { .. }) && self.battle_tutorial_commit(3));
            if rejected {
                let actor = session.actor;
                self.battle.arts_list_window = left;
                self.open_battle_command(actor);
                return;
            }
        }

        match session.resolved() {
            Some(ArtsInputResolution::Confirmed {
                target_row,
                target_slot,
            }) => {
                let caster = session.actor;
                // A replayed preseed commits exactly as a typed string does
                // - retail's bare confirm enters the same `0x5A` state.
                let sequence = session.committed_string().to_vec();
                self.battle.arts_list_window = left;
                self.run_battle_art(caster, &sequence, target_row, target_slot);
            }
            Some(ArtsInputResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.battle.arts_list_window = left;
                self.open_battle_command(actor);
            }
            None => {
                self.battle.arts_input = Some(session);
            }
        }
    }
}
