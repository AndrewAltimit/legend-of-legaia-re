//! Game-mode driver.
//!
//! Port of the 28-entry game-mode state table at SCUS RAM `0x8007078C`. Each
//! retail entry is 24 bytes:
//!
//! ```text
//!   +0x00  u32 name_string_ptr   ; ASCII label for debug ("CONFIG MODE", ...)
//!   +0x0A  i16 next_mode         ; mode to transition to when handler signals
//!                                ; completion (-1 = self-managed, no auto-tx)
//!   +0x10  u32 handler_fn_ptr    ; per-mode handler called every frame
//!   +0x14  u32 parameter         ; flag bits passed to the handler
//! ```
//!
//! The retail current-mode register is `gp[0x524]` (an `i16`); the dev
//! mode-transition writer is `FUN_800179C0` (gated on debug enable). Each
//! handler returns by either staying in the same mode (per-frame loop), or
//! transitioning to `next_mode` (init -> run pattern).
//!
//! In the clean-room port we map each mode to a [`GameMode`] enum variant,
//! the handler to a [`ModeHandler`] trait, and the parameter to flag bits
//! in [`ModeParam`]. The Sony function pointers are NOT used; engine
//! integrations supply Rust closures that drive the [`super::world::World`].

use crate::world::{SceneMode, World};

/// One row of the retail mode table. Engine-mapping shape: same fields as
/// the on-disc layout, minus the function pointer (replaced by an enum
/// dispatch in [`ModeDriver`]).
#[derive(Debug, Clone, Copy)]
pub struct ModeEntry {
    pub mode: GameMode,
    /// Debug name. Matches the SCUS entry's `name_string_ptr` text.
    pub name: &'static str,
    /// Mode to transition to on completion. `None` = self-managed (the
    /// retail i16 -1 sentinel).
    pub next: Option<GameMode>,
    /// Flag bits at +0x14. Most have meaningful values: 0x002, 0x00A, 0x800,
    /// 0x802, 0x80A. The 0x800 bit toggles "init handler" vs "run handler"
    /// (Mode 2 = 0x80A is INIT, Mode 3 = 0x002 is RUN). Bits 0x008/0x002 vary.
    pub param: u32,
}

/// The 28 game modes. Variant ordering matches the retail table index.
/// `from_index`/`as_index` round-trip them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// Mode 0 - boot config init (sound test / debug menu entry).
    ConfigInit,
    /// Mode 1 - per-frame config menu.
    ConfigMode,
    /// Mode 2 - main init (boot to title screen).
    MainInit,
    /// Mode 3 - title-screen per-frame.
    MainMode,
    /// Mode 4 - monster test init (debug).
    MonsterTest,
    /// Mode 5 - monster test per-frame.
    MonsterMode,
    /// Mode 6 - TMD test init (debug).
    TmdTest,
    /// Mode 7 - TMD test per-frame.
    TmdMode,
    /// Mode 8 - effect-pool test init.
    EfectTest,
    /// Mode 9 - effect-pool test per-frame.
    EfectMode,
    /// Mode 10 - generic test init.
    TestTest,
    /// Mode 11 - generic test per-frame.
    TestMode,
    /// Mode 12 - field/town init (MAPDISP MODE INIT).
    MapdispInit,
    /// Mode 13 - field/town per-frame (MAPDISP MODE).
    MapdispMode,
    /// Mode 14 - map test init (debug).
    MapTest,
    /// Mode 15 - map test per-frame.
    MapMode,
    /// Mode 16 - "READ" init (string-test mode).
    ReadInit,
    /// Mode 17 - READ per-frame.
    ReadMode,
    /// Mode 18 - game-over init.
    GameOverInit,
    /// Mode 19 - game-over per-frame.
    GameOverMode,
    /// Mode 20 - battle init.
    BattleInit,
    /// Mode 21 - battle per-frame.
    BattleMode,
    /// Mode 22 - card-game init.
    CardInit,
    /// Mode 23 - card-game per-frame.
    CardMode,
    /// Mode 24 - other init.
    OtherInit,
    /// Mode 25 - other per-frame.
    OtherMode,
    /// Mode 26 - cutscene/STR init.
    StrInit,
    /// Mode 27 - cutscene/STR per-frame.
    StrMode,
}

impl GameMode {
    pub fn as_index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Some(match i {
            0 => GameMode::ConfigInit,
            1 => GameMode::ConfigMode,
            2 => GameMode::MainInit,
            3 => GameMode::MainMode,
            4 => GameMode::MonsterTest,
            5 => GameMode::MonsterMode,
            6 => GameMode::TmdTest,
            7 => GameMode::TmdMode,
            8 => GameMode::EfectTest,
            9 => GameMode::EfectMode,
            10 => GameMode::TestTest,
            11 => GameMode::TestMode,
            12 => GameMode::MapdispInit,
            13 => GameMode::MapdispMode,
            14 => GameMode::MapTest,
            15 => GameMode::MapMode,
            16 => GameMode::ReadInit,
            17 => GameMode::ReadMode,
            18 => GameMode::GameOverInit,
            19 => GameMode::GameOverMode,
            20 => GameMode::BattleInit,
            21 => GameMode::BattleMode,
            22 => GameMode::CardInit,
            23 => GameMode::CardMode,
            24 => GameMode::OtherInit,
            25 => GameMode::OtherMode,
            26 => GameMode::StrInit,
            27 => GameMode::StrMode,
            _ => return None,
        })
    }

    /// Map a game mode to the [`SceneMode`] the World should run in. Init
    /// modes hold their successor's scene mode (init code prepares assets
    /// for the per-frame mode).
    pub fn scene_mode(self) -> SceneMode {
        match self {
            GameMode::MapdispInit | GameMode::MapdispMode => SceneMode::Field,
            GameMode::BattleInit | GameMode::BattleMode => SceneMode::Battle,
            GameMode::StrInit | GameMode::StrMode => SceneMode::Cutscene,
            // Title / config / debug-test modes don't drive a Field/Battle
            // scene tick. The actor VM and effect pool still run via the
            // World; the top-level dispatch just no-ops.
            _ => SceneMode::Title,
        }
    }
}

/// The 28-entry retail mode table, transcribed from SCUS `0x8007078C`. Use
/// [`GameMode::as_index`] to look up an entry.
pub const TABLE: [ModeEntry; 28] = [
    ModeEntry {
        mode: GameMode::ConfigInit,
        name: "CONFIG",
        next: None,
        param: 0x002,
    },
    ModeEntry {
        mode: GameMode::ConfigMode,
        name: "CONFIG MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::MainInit,
        name: "MAIN",
        next: None,
        param: 0x80A,
    },
    ModeEntry {
        mode: GameMode::MainMode,
        name: "MAIN MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x002,
    },
    ModeEntry {
        mode: GameMode::MonsterTest,
        name: "MONSTER TEST",
        next: None,
        param: 0x00A,
    },
    ModeEntry {
        mode: GameMode::MonsterMode,
        name: "MONSTER MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::TmdTest,
        name: "TMD TEST",
        next: None,
        param: 0x002,
    },
    ModeEntry {
        mode: GameMode::TmdMode,
        name: "TMD MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::EfectTest,
        name: "EFECT TEST",
        next: None,
        param: 0x800,
    },
    ModeEntry {
        mode: GameMode::EfectMode,
        name: "EFECT MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::TestTest,
        name: "TEST TEST",
        next: Some(GameMode::ConfigInit),
        param: 0x002,
    },
    ModeEntry {
        mode: GameMode::TestMode,
        name: "TEST MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::MapdispInit,
        name: "MAPDSIP MODE INIT",
        next: None,
        param: 0x002,
    },
    ModeEntry {
        mode: GameMode::MapdispMode,
        name: "MAPDSIP MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::MapTest,
        name: "MAP TEST",
        next: None,
        param: 0x00A,
    },
    ModeEntry {
        mode: GameMode::MapMode,
        name: "MAP MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::ReadInit,
        name: "READ",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::ReadMode,
        name: "READ MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::GameOverInit,
        name: "GAME OVER",
        next: Some(GameMode::ConfigInit),
        param: 0x802,
    },
    ModeEntry {
        mode: GameMode::GameOverMode,
        name: "GAMEOVER MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::BattleInit,
        name: "BATTLE",
        next: None,
        param: 0x80A,
    },
    ModeEntry {
        mode: GameMode::BattleMode,
        name: "BATTLE MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::CardInit,
        name: "CARD",
        next: None,
        param: 0x802,
    },
    ModeEntry {
        mode: GameMode::CardMode,
        name: "CARD MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::OtherInit,
        name: "OTHER",
        next: None,
        param: 0x802,
    },
    ModeEntry {
        mode: GameMode::OtherMode,
        name: "OTHER MODE",
        next: None,
        param: 0x000,
    },
    ModeEntry {
        mode: GameMode::StrInit,
        name: "STR",
        next: None,
        param: 0x80A,
    },
    ModeEntry {
        mode: GameMode::StrMode,
        name: "STR MODE",
        next: Some(GameMode::ConfigInit),
        param: 0x000,
    },
];

/// What a mode handler reports back to the driver after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerResult {
    /// Stay in the current mode for another frame (the retail "loop"
    /// pattern — most "MODE" handlers do this).
    Continue,
    /// Transition to the table's `next` entry. If `next` is `None` the
    /// driver leaves the current mode unchanged (matching the retail
    /// next == -1 sentinel).
    Done,
    /// Hard transition to a specific mode. Useful for init handlers that
    /// branch (e.g. main menu chooses CARD vs MAPDISP).
    GoTo(GameMode),
}

/// Trait an engine integration implements to provide per-mode behaviour.
///
/// The default impl makes every mode a no-op `Continue`, so an integration
/// can override only the modes it cares about and let the rest stay in a
/// quiescent state. The retail dispatch is much wider (loads scene
/// assets, drives the field VM, etc.); the trait is the seam to plug
/// those in.
///
/// `ModeDriver::tick` calls these in order: it consults the current
/// mode, calls the matching handler, applies the result.
pub trait ModeHandler {
    fn run(&mut self, mode: GameMode, world: &mut World) -> HandlerResult {
        let _ = (mode, world);
        HandlerResult::Continue
    }
}

/// No-op handler. Useful for tests + integrations that just want the
/// driver to track the current mode without driving anything.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHandler;
impl ModeHandler for NoopHandler {}

/// The mode driver. Owns the current-mode register (the engine equivalent
/// of `gp[0x524]`) and a frame counter for diagnostics.
#[derive(Debug)]
pub struct ModeDriver {
    current: GameMode,
    /// Total frames the driver has ticked, across all modes.
    pub frames: u64,
    /// Frames spent in the current mode (resets on transition).
    pub frames_in_mode: u64,
}

impl ModeDriver {
    /// Boot the driver in `MainInit` (mode 2), matching the retail boot
    /// sequence which jumps to MainInit after `gp` setup completes.
    pub fn new_at_boot() -> Self {
        Self::new(GameMode::MainInit)
    }

    pub fn new(start: GameMode) -> Self {
        Self {
            current: start,
            frames: 0,
            frames_in_mode: 0,
        }
    }

    pub fn current(&self) -> GameMode {
        self.current
    }

    pub fn entry(&self) -> &ModeEntry {
        &TABLE[self.current.as_index()]
    }

    /// Force a transition to `mode`. Resets `frames_in_mode`.
    pub fn jump_to(&mut self, mode: GameMode) {
        if mode != self.current {
            self.current = mode;
            self.frames_in_mode = 0;
        }
    }

    /// Drive one frame: sync the World's [`SceneMode`] to the current
    /// game mode, call the host's [`ModeHandler::run`], apply the result.
    /// Returns the handler's result so engines that want to act on
    /// transitions can observe them.
    pub fn tick<H: ModeHandler>(&mut self, host: &mut H, world: &mut World) -> HandlerResult {
        // Keep the World's scene-mode in sync each frame. Cheap and
        // idempotent — the World's tick path keys off it.
        world.mode = self.current.scene_mode();
        let r = host.run(self.current, world);
        // Always tick the World after the handler, so a Continue runs the
        // VMs for this mode every frame. Init modes that flip to the run
        // mode via Done get one final World tick before transitioning.
        world.tick();
        self.frames += 1;
        self.frames_in_mode += 1;
        match r {
            HandlerResult::Continue => {}
            HandlerResult::Done => {
                if let Some(next) = self.entry().next {
                    self.jump_to(next);
                }
            }
            HandlerResult::GoTo(mode) => self.jump_to(mode),
        }
        r
    }
}

impl Default for ModeDriver {
    fn default() -> Self {
        Self::new_at_boot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_28_entries_in_order() {
        assert_eq!(TABLE.len(), 28);
        for (i, entry) in TABLE.iter().enumerate() {
            assert_eq!(entry.mode.as_index(), i, "entry {i} index mismatch");
        }
    }

    #[test]
    fn from_index_round_trips() {
        for i in 0..28 {
            let m = GameMode::from_index(i).unwrap();
            assert_eq!(m.as_index(), i);
        }
        assert!(GameMode::from_index(28).is_none());
    }

    #[test]
    fn scene_mode_for_field_modes_is_field() {
        assert_eq!(GameMode::MapdispInit.scene_mode(), SceneMode::Field);
        assert_eq!(GameMode::MapdispMode.scene_mode(), SceneMode::Field);
    }

    #[test]
    fn scene_mode_for_battle_modes_is_battle() {
        assert_eq!(GameMode::BattleInit.scene_mode(), SceneMode::Battle);
        assert_eq!(GameMode::BattleMode.scene_mode(), SceneMode::Battle);
    }

    #[test]
    fn scene_mode_for_str_modes_is_cutscene() {
        assert_eq!(GameMode::StrInit.scene_mode(), SceneMode::Cutscene);
        assert_eq!(GameMode::StrMode.scene_mode(), SceneMode::Cutscene);
    }

    #[test]
    fn driver_starts_in_main_init() {
        let d = ModeDriver::new_at_boot();
        assert_eq!(d.current(), GameMode::MainInit);
        assert_eq!(d.frames_in_mode, 0);
    }

    #[test]
    fn handler_continue_keeps_mode_and_increments_frames() {
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        let mut h = NoopHandler;
        let mut w = World::default();
        for _ in 0..3 {
            assert_eq!(d.tick(&mut h, &mut w), HandlerResult::Continue);
        }
        assert_eq!(d.current(), GameMode::MapdispMode);
        assert_eq!(d.frames, 3);
        assert_eq!(d.frames_in_mode, 3);
        assert_eq!(w.mode, SceneMode::Field);
    }

    #[test]
    fn handler_done_transitions_to_next_when_set() {
        struct DoneOnce {
            ticked: bool,
        }
        impl ModeHandler for DoneOnce {
            fn run(&mut self, _: GameMode, _: &mut World) -> HandlerResult {
                if self.ticked {
                    HandlerResult::Continue
                } else {
                    self.ticked = true;
                    HandlerResult::Done
                }
            }
        }
        // MainMode has next=ConfigInit — transition should land there.
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut h = DoneOnce { ticked: false };
        let mut w = World::default();
        d.tick(&mut h, &mut w);
        assert_eq!(d.current(), GameMode::ConfigInit);
        assert_eq!(d.frames_in_mode, 0);
    }

    #[test]
    fn handler_done_no_op_when_next_is_none() {
        // ConfigInit has next=None — Done should leave the mode unchanged.
        struct AlwaysDone;
        impl ModeHandler for AlwaysDone {
            fn run(&mut self, _: GameMode, _: &mut World) -> HandlerResult {
                HandlerResult::Done
            }
        }
        let mut d = ModeDriver::new(GameMode::ConfigInit);
        let mut h = AlwaysDone;
        let mut w = World::default();
        d.tick(&mut h, &mut w);
        assert_eq!(d.current(), GameMode::ConfigInit);
    }

    #[test]
    fn handler_goto_jumps_directly() {
        struct GoToBattle;
        impl ModeHandler for GoToBattle {
            fn run(&mut self, _: GameMode, _: &mut World) -> HandlerResult {
                HandlerResult::GoTo(GameMode::BattleInit)
            }
        }
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        let mut h = GoToBattle;
        let mut w = World::default();
        d.tick(&mut h, &mut w);
        assert_eq!(d.current(), GameMode::BattleInit);
    }

    #[test]
    fn jump_to_resets_frame_counter() {
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        d.frames_in_mode = 100;
        d.jump_to(GameMode::BattleInit);
        assert_eq!(d.frames_in_mode, 0);
    }

    #[test]
    fn jump_to_same_mode_is_idempotent() {
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        d.frames_in_mode = 100;
        d.jump_to(GameMode::MapdispMode);
        // Same mode -> frame counter NOT reset (a self-jump should be a no-op).
        assert_eq!(d.frames_in_mode, 100);
    }
}
