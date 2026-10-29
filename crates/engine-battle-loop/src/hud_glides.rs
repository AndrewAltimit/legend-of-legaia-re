//! The battle HUD's glide and sweep state the loop steps: the commit-log
//! launch, the action / target plaques, the round prompt and ring-open
//! glides, the command pad hold, the intro-names sweep and the Arts list
//! window.

use crate::battle_state::BattleState;

/// What one engine tick adds to a battle-pass counter that retail advances
/// by the frame step: a tracked widget glide's `elapsed` byte, the intro
/// names' hold, the timed message's hold.
///
/// `FUN_801D9BBC` adds the frame step `*(0x1F800393)` per battle pass
/// (`lbu v1,0x393(v1)` / `addu v0,a0,v1` at `0x801D9C18..0x801D9C28`), and a
/// pass spans that many vsyncs, so a glide's `total` counts vsyncs: the
/// sixteen-frame raise lasts sixteen vsyncs at any cadence. The holds drain
/// the same way (`ctx[+0x6D6] -= 0x1F800393`, `0x801D0E3C..0x801D0E58`;
/// the `0x801F6964` hold in `FUN_80046A20`). The world ticks once per vsync
/// (the action SM's own timers drain by `1` a tick), so the per-tick step is
/// `1`. Stepping by the frame step on every tick ran each of them at twice
/// retail's speed under the battle's step of `2`
/// (`nivora_duel_mid_blazing_slash`: retail's plaque and bar are ten of
/// sixteen into their raise, the engine's had landed).
pub const BATTLE_PASS_STEP_PER_TICK: u8 = 1;

impl BattleState {
    /// The flag byte a glide record carries: `ctx[+0x1B]`, `1` from the
    /// round reset (`li v0,0x1` / `sb v0,0x1b(a0)` at
    /// `0x801D88FC..0x801D8900`). The arts entry's own spawns clear it
    /// around their `FUN_801D8DE8` call (`0x801D3D40`) and restore it.
    pub const GLIDE_BLOCKING_FLAG: u8 = 1;

    /// Sweep the intro labels on a round's hand-off to the action SM.
    /// Retail reaches the execute arm `0xFE` only through `0x0B`'s expiry,
    /// which sweeps every text actor (`0x801D0E58..0x801D0EB8`), so an
    /// action never runs under the names; the port opens its round prompt
    /// under them and sweeps them here instead.
    ///
    /// REF: FUN_801D0748 (flow state `0x0B`), FUN_800355F0
    pub fn sweep_battle_intro_names(&mut self) {
        self.intro_names_frames = 0;
    }

    /// Whether the battle camera's entry sweep still owns the opening
    /// (`BattleCamera::start_entry_sweep`). Retail's frame driver
    /// `FUN_80046A20` calls the battle tick `FUN_801D0748` - whose flow
    /// states `0x0A` / `0x0B` compose and drain these labels - only once its
    /// entry counter `gp+0x330` reads `0xFF` (`beq a0,v0,0x80047014` at
    /// `0x80046EF8`, `jal 0x801D0748` at `0x80047014`), so the names come up
    /// after the sweep, never under it.
    pub fn battle_entry_sweeping(&self) -> bool {
        self.camera
            .as_ref()
            .is_some_and(|c| c.entry_sweep_counter().is_some())
    }

    /// One tick of the intro timer: `ctx[+0x6D6] -= 0x1F800393` per battle
    /// pass, expiring at or below zero (`0x801D0E3C..0x801D0E58`) - one a
    /// vsync tick ([`crate::hud_glides::BATTLE_PASS_STEP_PER_TICK`]).
    /// The timer holds while the entry sweep runs
    /// ([`Self::battle_entry_sweeping`]).
    pub fn step_battle_intro_names(&mut self) {
        if self.battle_entry_sweeping() {
            return;
        }
        let step = u16::from(crate::hud_glides::BATTLE_PASS_STEP_PER_TICK);
        self.intro_names_frames = self.intro_names_frames.saturating_sub(step);
    }

    /// Raise a launch in `inbound`'s direction.
    pub fn launch_commit_log(&mut self, inbound: bool) {
        self.commit_log_launch = Some(legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(
            inbound,
        ));
    }

    /// One tick of the launch glide ([`BATTLE_PASS_STEP_PER_TICK`]). A
    /// settled inbound launch retires - the log is back at rest; a settled
    /// outbound one stays, which keeps the log hidden until the member
    /// returns to the ring or commits.
    pub fn step_commit_log_launch(&mut self) {
        let step = BATTLE_PASS_STEP_PER_TICK;
        if let Some(l) = self.commit_log_launch.as_mut() {
            l.step(step);
            if l.inbound && l.settled() {
                self.commit_log_launch = None;
            }
        }
    }

    /// A cast module raising the roster panels on mode `0`: PROT 0911's arm
    /// 9 pass, `FUN_801D8DE8(6, 0)` and `(0x4E + i, 0)` for `i` below
    /// `ctx[+0] - 1`, then `ctx[+0x18] = 6` (`0x801F7E64..0x801F7ED4`). Each
    /// record spawns at seat A and glides to seat B
    /// (`legaia_engine_battle_loop::battle_state::BattleState::module_panels_glide`).
    pub fn raise_module_roster_panels(&mut self) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        self.module_panels_glide = Some(LogLaunch::new(false));
    }

    /// Start the round prompt's open glide: sub-draw step 0 spawning its
    /// five records at seat A (`legaia_engine_battle_loop::battle_state::BattleState::round_prompt_glide`).
    pub fn start_round_prompt_glide(&mut self) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        self.round_prompt_glide = Some(LogLaunch::new(false));
    }

    /// Start the ring's open glide: sub-draw step 1 spawning its twelve
    /// records (`legaia_engine_battle_loop::battle_state::BattleState::ring_open_glide`).
    pub fn start_ring_open_glide(&mut self) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        self.ring_open_glide = Some(LogLaunch::new(false));
    }

    /// Land every HUD widget glide in flight on its rest seat, as
    /// `FUN_801D9BBC` does once a glide's `elapsed` reaches its `total`. The
    /// retail-compare capture seats this when the retail state's glides had
    /// all landed; no game path calls it.
    pub fn land_battle_hud_glides(&mut self) {
        for g in [
            self.action_plaque_glide.as_mut(),
            self.target_plaque_glide.as_mut(),
            self.readout_bar_glide.as_mut(),
            self.message_bar_glide.as_mut(),
            self.spirit_gauge_glide.as_mut(),
            self.module_panels_glide.as_mut(),
            self.result_windows_glide.as_mut(),
            self.round_prompt_glide.as_mut(),
            self.ring_open_glide.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            g.elapsed = g.total;
        }
        self.widget_glide_return = 0;
        self.widget_glide_latch = 0;
    }

    /// Land the command surfaces' own glides - the round prompt's and the
    /// ring's - and open the menu SM's gate, leaving the action plates
    /// where they are. A capture drive calls it every tick it steers: it
    /// walks the command flow to the phase a retail state froze, and the
    /// ticks a player spends waiting out each glide are not part of that
    /// state (the battle camera's prompt orbit runs on them). No game path
    /// calls it.
    pub fn release_command_pad_hold(&mut self) {
        for g in [
            self.round_prompt_glide.as_mut(),
            self.ring_open_glide.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            g.elapsed = g.total;
        }
        self.widget_glide_latch = 0;
    }

    /// One tick of the tracked widget glides, on the step the commit log's
    /// launch takes ([`BATTLE_PASS_STEP_PER_TICK`]; `FUN_801D9BBC` walks
    /// every tracked widget in one pass), and the pass's return value.
    ///
    /// The stepper returns `a3`: seeded from the first live record's flag
    /// byte `+3` (`0x801D9C00..0x801D9C08`), then rewritten to
    /// `total - elapsed + 1` by every record still in flight after its step
    /// (`0x801D9D04..0x801D9D18`). A record that lands this pass leaves it
    /// alone and retires (`sb zero,0(a1)` at `0x801D9D1C`), so the landing
    /// pass still returns the flag and the pass after it returns `0`. The
    /// flag is `ctx[+0x1B]` at spawn (`lbu a2,0xa(s1)` at `0x801D93C0`),
    /// which the round reset seeds to [`Self::GLIDE_BLOCKING_FLAG`].
    ///
    /// PORT: FUN_801D9BBC (the return value)
    pub fn step_action_plate_glides(&mut self) {
        let step = BATTLE_PASS_STEP_PER_TICK;
        let mut ret = 0u8;
        for g in [
            self.action_plaque_glide.as_mut(),
            self.target_plaque_glide.as_mut(),
            self.readout_bar_glide.as_mut(),
            self.message_bar_glide.as_mut(),
            self.spirit_gauge_glide.as_mut(),
            self.module_panels_glide.as_mut(),
            self.round_prompt_glide.as_mut(),
            self.ring_open_glide.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            // A landed record is retired: the stepper skips it.
            if g.settled() {
                continue;
            }
            if ret == 0 {
                ret = Self::GLIDE_BLOCKING_FLAG;
            }
            g.step(step);
            if !g.settled() {
                ret = g.total - g.elapsed + 1;
            }
        }
        self.widget_glide_return = ret;
    }

    /// Whether the next battle tick's menu pass will leave without reading
    /// the pad (`World::battle_command_glide_gate`): a tracked glide is
    /// still in flight, or a staged battle owes its latch pass. What a pad
    /// script waits out before it presses on a freshly opened prompt.
    pub fn battle_command_pad_held(&self) -> bool {
        // No flow state runs under the battle-entry sweep: the frame driver
        // calls the battle tick only once its entry counter parks
        // (`FUN_80046A20`, `0x80046EF8`), so no press reaches a command
        // surface there.
        // REF: FUN_80046A20
        if self.battle_entry_sweeping() {
            return true;
        }
        [
            self.action_plaque_glide.as_ref(),
            self.target_plaque_glide.as_ref(),
            self.readout_bar_glide.as_ref(),
            self.message_bar_glide.as_ref(),
            self.spirit_gauge_glide.as_ref(),
            self.module_panels_glide.as_ref(),
            self.round_prompt_glide.as_ref(),
            self.ring_open_glide.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|g| !g.settled())
            || (self.stage_id != 0
                && (self.widget_glide_return != 0 || self.widget_glide_latch != 0))
    }

    /// Backstop for a session that was already open when the flow byte moved
    /// onto the round-open `Begin | Run` prompt.
    ///
    /// Retail's `0x14` arm sets `ctx[+0x06] = 0x1E` before any member picks,
    /// and the ring (`0x28`) is only reached through it - so the prompt is a
    /// property of the **round**, not of the turn. The port reads that off
    /// [`legaia_engine_battle_menus::battle_flow::BattleFlowState::TurnPrompt`]: the round boundary
    /// parks the flow there and battle entry leaves it at `Idle`, and
    /// `World::open_battle_command` now builds the session **already on the
    /// prompt** in both cases. What is left for this pass is the one ordering
    /// it cannot cover - a session opened while the flow was elsewhere and
    /// still open when the boundary parks it here. A session reopened
    /// mid-round (a submenu backed out of) finds the flow on a window state
    /// and is left alone, which is where retail's own cancel arms land.
    ///
    /// An **ambushed** party never reaches here on its lost round: the
    /// `ctx[+0x290]` side lockout (`World::reseed_initiative`) zeroes every
    /// party key, so no party turn opens - retail's `0x0B -> 0xFE` jump in the
    /// port's own seating.
    ///
    /// REF: FUN_801D0748 (states 0x14 / 0x1E)
    pub fn arm_round_open_prompt(&mut self) {
        use legaia_engine_battle_menus::battle_flow::BattleFlowState;
        use legaia_engine_battle_menus::battle_input::CommandPhase;
        if self.flow != BattleFlowState::TurnPrompt {
            return;
        }
        // State `0x14`'s `0x1E` write is the battle tick's, which the frame
        // driver does not call under the battle-entry sweep (`0x80046EF8`):
        // the prompt - and its unfold glide - opens once the sweep is over.
        if self.battle_entry_sweeping() {
            return;
        }
        let no_escape = self.no_escape;
        if let Some(session) = self.command.as_mut()
            && matches!(session.phase, CommandPhase::Menu { .. })
        {
            session.no_escape = no_escape;
            session.phase = CommandPhase::RoundPrompt { cursor: 0 };
            self.start_round_prompt_glide();
        }
    }

    /// Open the retail-model **Arts command input** for `actor`: seed the
    /// AP pool from the acting character's AGL (retail `ctx+0x6DC` <-
    /// actor `+0x154`; [`legaia_engine_battle::arts_command_input::DEFAULT_POOL`] without
    /// stats), the four per-direction press costs (Left = the arm command
    /// `0x0C`, carrying the per-(character, weapon) `+0x74` byte from
    /// `World::battle_arm_costs`; the others at the favored base), and the
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
        self.arts_input.is_some()
    }

    /// The actor-table index of the party member entering commands, or
    /// `None` when no session is open. The party surface has two mutually
    /// exclusive forms - roster panels, and a full-width bar for the actor
    /// that owns the pad - so a host needs the *which*, not just the
    /// *whether*.
    pub fn arts_input_actor(&self) -> Option<u8> {
        self.arts_input.as_ref().map(|s| s.actor)
    }

    /// Stand the open arts entry's Triangle list on `page` with its window's
    /// slide counter at `slide` - a retail capture's list as its RAM holds
    /// it (`_DAT_8007B458 / 5`, `0x801F4E0A`). A no-op without an entry, or
    /// for a page the character's learned count does not reach.
    pub fn seat_arts_list(&mut self, page: u8, slide: u8) {
        use legaia_engine_battle::arts_command_input::ARTS_LIST_SLIDE_MAX;
        let Some(s) = self.arts_input.as_mut() else {
            return;
        };
        if page >= s.list_pages {
            return;
        }
        s.list_page = Some(page);
        s.list_row_page = page;
        s.list_slide = slide.min(ARTS_LIST_SLIDE_MAX);
    }

    /// Step the window an ended entry left on screen one command-flow frame
    /// ([`legaia_engine_battle::arts_command_input::ArtsListWindow::step`]).
    pub fn step_arts_list_window(&mut self) {
        let step = self.anim_pass_vsyncs;
        if let Some(w) = self.arts_list_window.as_mut()
            && !w.step(step)
        {
            self.arts_list_window = None;
        }
    }

    /// Where the Spirit turn's AP bar and AP plate sit relative to their rest
    /// seats while their raise glide runs (`legaia_engine_battle_loop::battle_state::BattleState::spirit_gauge_glide`):
    /// `(bar dy, plate dx)` in stage pixels, `(0, 0)` once landed. The bar
    /// rises from `y = 234` onto `192` and the plate slides from `x = 328`
    /// onto `208` (placement records `0x0F` / `0x52`, walked by
    /// `FUN_801D9BBC`).
    pub fn spirit_gauge_glide_offsets(&self) -> (i32, i32) {
        use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
        let g = self.spirit_gauge_glide.as_ref();
        (
            LogLaunch::seat_dy(g, 234, 192),
            LogLaunch::seat_dy(g, 328, 208),
        )
    }
}
