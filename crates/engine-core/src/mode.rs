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
//! the handler to a [`ModeHandler`] trait, and the parameter to the
//! [`ModeEntry::param`] flag bits. The Sony function pointers are NOT used;
//! engine integrations supply Rust closures that drive the
//! [`super::world::World`]. The table's name/param/next fields are
//! reconciled against the disc-recovered map (`legaia_asset::mode_table`)
//! by the disc-gated `mode_table_reconcile` test.

use crate::input::InputState;
use crate::world::{SceneMode, World};
use legaia_engine_vm::Position as ActorVmPosition;

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
    /// Mode 0 - "CONFIG INIT" (dev label - misleading): the retail handler
    /// `FUN_80025C68` runs the sound detach + [`CORE_STATE_RESET`], then
    /// loads PROT 971, the dev DEBUG-MENU overlay (`FUN_8003EBE4(0x4C)`) -
    /// see the corrected `functions.md` row (the earlier "PROT 973
    /// slot-machine debug" reading was loader-math off-by-2). Not a
    /// game-config init.
    ConfigInit,
    /// Mode 1 - "CONFIG MODE" per-frame handler for the debug-menu mode.
    /// Uses the default per-frame dispatcher `FUN_80025EEC`.
    ConfigMode,
    /// Mode 2 - "MAIN INIT": the field/town gameplay INIT mode. The retail
    /// handler `FUN_80025B64` loads the field overlay (`FUN_8003EBE4(2)`)
    /// and calls the per-scene initializer `FUN_801D6704`, which loads the
    /// map + MAN + camera + fog + BGM, allocates the game-mode work buffer,
    /// then hands off to mode 3 (field per-frame) by writing
    /// `_DAT_8007B83C = 3`. The title screen's NEW GAME path launches this
    /// mode (`_DAT_8007B83C = 2` at `0x801DFC00`). The dev label "MAIN" and
    /// older "options menu" notes are misleading: this is the field entry,
    /// not the options screen (options is reached through the in-game menu).
    MainInit,
    /// Mode 3 - "MAIN MODE": the field/town per-frame gameplay handler
    /// (`game_mode 0x03`, the on-field / in-town loop). Mode 2 (init) hands
    /// off here once the map is resident.
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
    /// Mode 12 - world-map display init (MAPDSIP MODE INIT, disc-misspelled).
    /// NOT field/town: field/town is `MainMode` (2/3, `game_mode 0x03`). The
    /// MAPDISP per-frame handler routes the world-map render tick.
    MapdispInit,
    /// Mode 13 - world-map display per-frame (MAPDSIP MODE). See `MapdispInit`.
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
    /// Mode 22 - "CARD" init: the menu / memory-card overlay mode pair's
    /// init handler (`FUN_8002574C`).
    CardInit,
    /// Mode 23 - "CARD MODE" per-frame: one of only two per-frame modes with
    /// its own handler (`0x80025F74`). Hosts the memory-card UI AND the
    /// in-field pause menu: every menu-open capture in the save library
    /// (equipment / status / options, field and town) holds
    /// `_DAT_8007B83C = 0x17` (23) - the pause menu runs under this mode,
    /// not field mode 3.
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
            // game_mode 0x03 is the in-town / on-field gameplay mode. Two
            // independent retail captures confirm this empirically: the
            // `v0_1_pre_battle_tetsu` save (Vahn walking in Rim Elm / town01)
            // and the runtime-pinned free-movement controller on `map03`,
            // both at game_mode 0x03 (see docs/subsystems/field-locomotion.md).
            // The disc-recovered handler map (legaia_asset::mode_table)
            // confirms it structurally: mode 2's init handler FUN_80025B64
            // loads the field overlay + per-scene initializer and hands off
            // to mode 3. MainInit holds Field like the other init modes
            // below hold their successors'.
            GameMode::MainInit | GameMode::MainMode => SceneMode::Field,
            // MAPDISP (12/13) is the world-map DISPLAY mode, not the field -
            // pinned by the disc mode table (legaia_asset::mode_table): its
            // per-frame handler 0x80025F2C routes the world-map render tick
            // (docs/subsystems/world-map.md). Field/town is MainMode above.
            GameMode::MapdispInit | GameMode::MapdispMode => SceneMode::WorldMap,
            GameMode::BattleInit | GameMode::BattleMode => SceneMode::Battle,
            // CARD (22/23) hosts the memory-card UI AND the in-field pause
            // menu: every menu-open capture in the save library holds
            // `_DAT_8007B83C = 0x17` (see [`GameMode::CardMode`]). The world
            // suspends field dispatch while the menu owns the frame; the
            // hosting session restores the suspended mode on close.
            GameMode::CardInit | GameMode::CardMode => SceneMode::Menu,
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
    /// pattern - most "MODE" handlers do this).
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
    fn run(&mut self, mode: GameMode, world: &mut World, input: &InputState) -> HandlerResult {
        let _ = (mode, world, input);
        HandlerResult::Continue
    }
}

/// No-op handler. Useful for tests + integrations that just want the
/// driver to track the current mode without driving anything.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHandler;
impl ModeHandler for NoopHandler {}

/// Reference [`ModeHandler`] that drives the field-entry mode pair
/// (`MainInit` → `MainMode`, the retail field/town init + per-frame
/// handlers) end-to-end without any GPU / scene-asset dependencies. Useful
/// as a smoke test for the World + ModeDriver wiring and as an example for
/// engines integrating real scene loaders.
///
/// Behaviour:
///
/// - `MainInit`: spawn `actor_count` actors in the world via the actor VM
///   `SpawnAt` opcode, with positions arranged on a horizontal line. Returns
///   `Done` so the driver advances to the table's next mode - mirroring the
///   retail mode-2 handler's "load the scene, hand off to mode 3" shape.
/// - `MainMode`: ticks the world (positions advance via the move VM). When
///   the host signals `Cross` (just-pressed), returns `GoTo(MapdispInit)` -
///   the field → world-map exit transition. Otherwise `Continue`.
/// - Other modes: no-op `Continue`.
///
/// This is the smallest concrete demonstration that the World + ModeDriver
/// stack ticks per-frame, advances actor state, and reacts to input.
#[derive(Debug, Clone, Copy)]
pub struct FieldDemoHandler {
    pub actor_count: u8,
    initialised: bool,
}

impl FieldDemoHandler {
    pub fn new(actor_count: u8) -> Self {
        Self {
            actor_count,
            initialised: false,
        }
    }
}

impl ModeHandler for FieldDemoHandler {
    fn run(&mut self, mode: GameMode, world: &mut World, input: &InputState) -> HandlerResult {
        use crate::input::PadButton;
        match mode {
            GameMode::MainInit => {
                if !self.initialised {
                    // Set per-actor default positions before spawning so
                    // the actor VM SpawnDefault path lands them on a row.
                    for i in 0..self.actor_count {
                        let slot = i as usize;
                        if slot >= world.actors.len() {
                            break;
                        }
                        world.actors[slot].default_pos =
                            ActorVmPosition::new(32 + (slot as i16) * 24, 64);
                    }
                    // Synthesize bytecode: SpawnDefault for each actor, then End.
                    let mut bytecode = Vec::with_capacity((self.actor_count as usize + 1) * 4);
                    for i in 0..self.actor_count {
                        // 4-byte instruction: opcode=0x01 (SpawnDefault), operand_b=actor_id, w=0
                        bytecode.extend_from_slice(&[0x01, i, 0x00, 0x00]);
                    }
                    bytecode.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // End
                    let _ = world.run_actor_bytecode(&bytecode);
                    self.initialised = true;
                }
                HandlerResult::Done
            }
            GameMode::MainMode => {
                if input.just_pressed(PadButton::Cross) {
                    HandlerResult::GoTo(GameMode::MapdispInit)
                } else {
                    HandlerResult::Continue
                }
            }
            _ => HandlerResult::Continue,
        }
    }
}

/// The mode driver. Owns the current-mode register (the engine equivalent
/// of `gp[0x524]`) and a frame counter for diagnostics.
#[derive(Debug)]
pub struct ModeDriver {
    current: GameMode,
    /// Total frames the driver has ticked, across all modes.
    pub frames: u64,
    /// Frames spent in the current mode (resets on transition).
    pub frames_in_mode: u64,
    /// The [`PerFrameStage`] resolved on the last [`Self::tick`], or `None`
    /// when the current mode is an INIT mode. Hosts read it to dispatch the
    /// mode's overlay hook (mode 13's `FUN_801CE850`) and to know which
    /// mid-frame driver retail would have run.
    last_stage: Option<PerFrameStage>,
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
            last_stage: None,
        }
    }

    pub fn current(&self) -> GameMode {
        self.current
    }

    /// The per-frame staging plan resolved on the last [`Self::tick`].
    pub fn last_stage(&self) -> Option<PerFrameStage> {
        self.last_stage
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
    pub fn tick<H: ModeHandler>(
        &mut self,
        host: &mut H,
        world: &mut World,
        input: &InputState,
    ) -> HandlerResult {
        // Keep the World's scene-mode in sync each frame. Cheap and
        // idempotent - the World's tick path keys off it.
        world.mode = self.current.scene_mode();
        let r = host.run(self.current, world, input);
        // Retail's per-frame handlers ([`per_frame_stage`]) early-out when the
        // frame-begin pass `FUN_8001698C` returns non-zero: that frame gets a
        // pad poll and a `VSync(0)` and nothing else - no mid-frame driver and
        // no frame-end pass. Honour the same skip here. Only the per-frame
        // (odd-indexed) modes have that shape; INIT modes tick unconditionally.
        let skipped = per_frame_stage(self.current).is_some() && world.take_frame_begin_skip();
        self.last_stage = per_frame_stage(self.current);
        // Mode 0 CONFIG INIT runs the sound detach (`FUN_8002689C`) ahead of
        // its staging - the same call the `runs_core_reset` flag records for
        // `FUN_80025CB4`. The latch makes repeat frames in the mode free.
        if matches!(
            mode_init_stage(self.current),
            Some(ModeInitStage {
                runs_core_reset: true,
                ..
            })
        ) {
            world.detach_sound();
        }
        if !skipped {
            // Tick the World after the handler, so a Continue runs the VMs
            // for this mode every frame. Init modes that flip to the run
            // mode via Done get one final World tick before transitioning.
            world.tick();
        }
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

/// The shared mode-INIT core state reset (`FUN_80025CB4`).
///
/// PORT: FUN_80025cb4
///
/// Called by the CONFIG INIT handler (`FUN_80025C68`) after the scene-name
/// sync `FUN_8001D7F8`. Every store below is read off the instruction
/// stream (`see ghidra/scripts/funcs/80025cb4.txt`, corroborated by the
/// static-recomp rendering of `func_80025CB4` - which also shows the
/// `_DAT_8007B8C8 = 0` store issued twice, a benign duplicate the Ghidra C
/// folds away). Field order below = retail store order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreStateReset {
    /// `DAT_8007B718` (u16): display-brightness register, reset to `0x80`.
    pub brightness: u16,
    /// `DAT_8007B6F4` (u16): camera zoom / GTE `H` projection word, `0xA0`.
    pub gte_h_zoom: u16,
    /// `_DAT_8007B8B8` (u32): the field warm-entry flag - zero forces the
    /// next field entry down the cold path (see `field-locomotion.md`).
    pub field_warm_entry: u32,
    /// `DAT_8007B648` (u8) cleared.
    pub b648: u8,
    /// `_DAT_8007B83C` (u16): the master game-mode word, advanced to `1`
    /// (CONFIG MODE - the mode-0 INIT hands off to its RUN sibling).
    pub game_mode: u16,
    /// `_DAT_8007B874` (u32): the newly-pressed pad-edge word, cleared.
    pub pad_pressed_edge: u32,
    /// `_DAT_8007B830` + `_DAT_8007B8C8` (u32): cleared (B8C8 twice).
    pub b830_b8c8: u32,
    /// `DAT_8007B768` (u16): the DATA_FIELD bundle index, `0xFFFF` = none
    /// (the sentinel `FUN_80020118` tests with `bgez`).
    pub data_field_index: u16,
    /// `DAT_8007B6FC` + `DAT_8007B6C8` (the `FUN_80025358` sub-overlay
    /// stage counter) + `_DAT_8007B9C4`: cleared.
    pub counters_cleared: u32,
    /// Retail leg (`_DAT_8007B98C == 0` - debug word clear):
    /// `_DAT_8007BA36 = 1` and `DAT_8007B71C = 1`.
    pub retail_ba36_b71c: u16,
    /// `_DAT_8007B900` (u32): set to `0xFFFFFFFF` unconditionally.
    pub b900: u32,
}

/// The retail store values of [`CoreStateReset`]. The scratchpad mirrors
/// (`0x1F80037D/91/93` reloaded from `DAT_8007B7BE/E6` / `DAT_8007B8EC`)
/// are carried by the host's scratch model, not this struct.
pub const CORE_STATE_RESET: CoreStateReset = CoreStateReset {
    brightness: 0x80,
    gte_h_zoom: 0xA0,
    field_warm_entry: 0,
    b648: 0,
    game_mode: 1,
    pad_pressed_edge: 0,
    b830_b8c8: 0,
    data_field_index: 0xFFFF,
    counters_cleared: 0,
    retail_ba36_b71c: 1,
    b900: 0xFFFF_FFFF,
};

/// One mode-table INIT handler's staging plan: which slot-A overlay it
/// loads and which loaded-overlay entry point it hands off to.
///
/// The retail INIT handlers are thin wrappers with one shared shape -
/// optional state reset, `FUN_8003DE7C(0)` blocking read-wait, slot-A
/// overlay load (`FUN_8003EBE4(param, 0)` =
/// [`crate::overlay_loader::load_overlay_a`]), wait again, then a `jal`
/// into the freshly loaded overlay:
///
/// | Mode | Handler | Overlay A param | Overlay entry |
/// |---|---|---|---|
/// | 0 CONFIG INIT | `FUN_80025C68` | `0x4C` | `FUN_801CE8EC` |
/// | 2 MAIN INIT | `FUN_80025B64` | `2` | `FUN_801D6704` |
/// | 18 GAME OVER INIT | `FUN_80025B30` | `7` | `FUN_801CE844` |
///
/// CONFIG INIT additionally runs the sound detach `FUN_8002689C` and the
/// [`CORE_STATE_RESET`] first; GAME OVER INIT skips the leading wait.
/// The engine's [`ModeDriver`] + scene host replace the overlay `jal` with
/// native scene entry, so the plan is data, not control flow, here.
// PORT: FUN_80025c68 (mode-0 CONFIG INIT stage plan)
// PORT: FUN_80025b64 (mode-2 MAIN INIT stage plan)
// PORT: FUN_80025b30 (mode-18 GAME OVER INIT stage plan; retail-unreachable,
//                     dev harness only - no static writer of mode 18 exists)
// REF: FUN_8003EBE4 (the slot-A loader the params feed)
// REF: FUN_8002689C (the sound detach CONFIG INIT runs before staging;
//                    ported at engine-core::sound_state::SoundDetachLatch)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeInitStage {
    /// `FUN_8003EBE4` first argument (extraction PROT entry `param + 0x37F`).
    pub overlay_a_param: i32,
    /// VA of the loaded overlay's entry the handler `jal`s after the wait.
    pub overlay_entry: u32,
    /// Whether the handler runs [`CORE_STATE_RESET`] before staging.
    pub runs_core_reset: bool,
}

/// The mode-24 OTHER/warp INIT dispatcher's per-sub-id staging plan.
///
/// PORT: FUN_80025980
///
/// `FUN_80025980` (mode 24 "OTHER INIT") stages one of the seven
/// "other game" overlays by the warp sub-id `_DAT_8007BA34`: overlay-A
/// param = `0x4D + sel` with `sel += 2` first when `sel > 5` (recomp:
/// `slti 6` + `addiu 2` at `0x80025A1C..0x80025A28`) - so sub 0..5 map to
/// extraction PROT 972..977 and sub 6 skips to PROT 980. Confirmed pins:
/// sub 0 = fishing (PROT 972), sub 3 = casino slot machine (PROT 975),
/// sub 6 = dance (PROT 980). After the load it `jalr`s the per-sub-id
/// overlay init from the SCUS table at `0x80010AE4` and hands the mode
/// word to 0x19 (OTHER MODE).
///
/// Before staging it resets the warp-shared state: scene-name snapshot
/// (`0x8007BAE8` <- `0x80084548`, 8 bytes), DATA_FIELD staged index
/// `DAT_8007B768 = 0xFFFF`, the `B9C4`/`B6C8`/`B6A8` counters, entity
/// words `_DAT_8007BC3C`/`BC4C = -1`, kingdom-base snapshot
/// (`gp+0x7AC` <- `_DAT_80084540`), and a `FUN_80058104(0)` teardown call.
// REF: FUN_80058104
pub fn other_warp_init_stage(sub_id: i16) -> Option<ModeInitStage> {
    /// Per-sub-id overlay init entries (jump table at `0x80010AE4`).
    const OTHER_WARP_ENTRIES: [u32; 7] = [
        0x801C_F070,
        0x801C_E8A0,
        0x801C_EE80,
        0x801C_EC94,
        0x801C_F00C,
        0x801C_EA6C,
        0x801C_EF54,
    ];
    if !(0..7).contains(&i32::from(sub_id)) {
        // Retail's `sltiu 7` bound skips only the entry dispatch (the
        // overlay request still fires with the biased param); no retail
        // caller passes an out-of-range sub-id, so the engine returns no
        // stage at all.
        return None;
    }
    let sel = i32::from(sub_id);
    let biased = if sel < 6 { sel } else { sel + 2 };
    Some(ModeInitStage {
        overlay_a_param: biased + 0x4D,
        overlay_entry: OTHER_WARP_ENTRIES[sel as usize],
        runs_core_reset: false,
    })
}

/// Staging plan for the three thin INIT handlers (see [`ModeInitStage`]).
/// Returns `None` for modes whose INIT is not this wrapper shape.
pub fn mode_init_stage(mode: GameMode) -> Option<ModeInitStage> {
    match mode {
        GameMode::ConfigInit => Some(ModeInitStage {
            overlay_a_param: 0x4C,
            overlay_entry: 0x801C_E8EC,
            runs_core_reset: true,
        }),
        GameMode::MainInit => Some(ModeInitStage {
            overlay_a_param: 2,
            overlay_entry: 0x801D_6704,
            runs_core_reset: false,
        }),
        GameMode::GameOverInit => Some(ModeInitStage {
            overlay_a_param: 7,
            overlay_entry: 0x801C_E844,
            runs_core_reset: false,
        }),
        _ => None,
    }
}

/// The mid-frame driver a per-frame mode handler calls between the
/// frame-begin pass and the frame-end pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBody {
    /// `FUN_80016444(param)` - the master frame driver (five actor tick
    /// passes, five render passes, the display flip). The `param` really does
    /// vary by mode: the default handler passes `1`, MAPDISP passes `0`.
    Master { param: i32 },
    /// `FUN_80017978` - the CARD-mode substitute. Mode 23 replaces the master
    /// driver outright rather than parameterising it. See [`CARD_FRAME_BODY`]
    /// for what it does instead.
    CardDriver,
}

/// What mode 23 CARD runs in place of the master frame driver.
///
/// PORT: FUN_80017978
/// REF: FUN_800179C0, FUN_800188C8, FUN_80020DE0
///
/// The whole body is three calls and a `move v0, zero`
/// (`0x80017978..0x800179BC`):
///
/// 1. `FUN_800179C0` - the debug mode-advance chord. Its first two
///    instructions load `_DAT_8007B98C` and branch straight to `jr ra` when it
///    is zero, which is the retail value, so on a shipped disc this leg does
///    nothing. See [`DEBUG_MODE_ADVANCE`] for the law it encodes.
/// 2. `(*_DAT_8007B8E0)[+0x0C]()` - an indirect call through the CARD actor's
///    tick handler. `_DAT_8007B8E0` is not a mode-table row: it is the actor
///    the mode-entry path spawns from descriptor `0x800706D4` via
///    `FUN_80020DE0` (`sw v0,-0x4720(at)` at `0x800257AC`), and `+0x0C` is the
///    handler slot that spawner copies out of the descriptor's `+0x8`.
/// 3. `FUN_800188C8(_DAT_1F800393)` - the dev pad-driven readout HUD, itself
///    gated on `_DAT_8007B98C` and already out of scope.
///
/// So the load-bearing content is step 2 alone, and two things follow that the
/// declarative [`PerFrameStage`] shape does not otherwise say:
///
/// - **CARD never calls `FUN_80016444`.** Mode 23 runs no actor tick passes,
///   no render passes and no display flip through the master driver; whatever
///   the card actor's handler draws is the entire frame.
/// - **The abort branch is dead for CARD.** `FUN_80017978` ends `move v0,zero`
///   and has no other return path, so the `body_can_abort` test in the mode-23
///   handler `FUN_80025F74` can never fire, and the frame-end pass
///   `FUN_80016B6C` always runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardFrameBody {
    /// Whether the body runs the master frame driver `FUN_80016444`.
    pub runs_master_driver: bool,
    /// Whether it dispatches the CARD actor's `+0x0C` tick handler.
    pub ticks_card_actor: bool,
    /// The value the body always returns. Zero, so the caller's abort test
    /// never fires.
    pub returns: i32,
}

/// The retail shape of [`FrameBody::CardDriver`].
pub const CARD_FRAME_BODY: CardFrameBody = CardFrameBody {
    runs_master_driver: false,
    ticks_card_actor: true,
    returns: 0,
};

/// The debug mode-advance chord `FUN_800179C0` reads, recorded because it is
/// the only place in `SCUS_942.54` that writes the game-mode global
/// `_DAT_8007B83C` from a mode table row's `next` field - the field
/// [`ModeEntry::next`] models.
///
/// REF: FUN_800179C0
///
/// The body is inert in retail (`_DAT_8007B98C == 0` gates it at
/// `0x800179CC`), so this is a description, not a tick path. The law, read off
/// `0x800179C0..0x80017AA8`:
///
/// - A hold-repeat countdown at `_DAT_8007B890` decrements once per call and
///   suppresses the rest of the body until it reaches zero.
/// - The chord tested against the packed pad word `_DAT_8007B850` is `0x900`
///   when `_DAT_8007B868` is zero and `0x100` otherwise; in the `0x100` case a
///   low-nibble-all-set (`pad & 0xF == 0xF`) alternative also triggers.
/// - On a trigger it reads `mode_table[current].next` - the `i16` at `+0xA` of
///   the 24-byte row, table base `0x8007078C` - and a negative value means "no
///   transition", the same `-1` sentinel [`ModeEntry::next`] maps to `None`.
/// - One special case ahead of the table read: from mode 3 (`MainMode`) with
///   `_DAT_8007B8C8` non-zero it jumps to mode `0x0E` instead.
///
/// The `next` read happens twice in the disassembly, once per branch of the
/// mode-3 test, and the second copy works only because the delay slot at
/// `0x80017A60` reloads the table base (`lui v1,0x8007`). Reading the second
/// `addiu v1,v1,0x78c` as an offset *from the first address* would put the
/// table at `0x80070F18`; it does not.
pub const DEBUG_MODE_ADVANCE_TABLE_BASE: u32 = 0x8007_078C;

/// Stride of a row in the `0x8007078C` mode table, in bytes.
pub const DEBUG_MODE_ADVANCE_ROW_STRIDE: u32 = 24;

/// Byte offset of the `next mode` `i16` inside a mode-table row.
pub const DEBUG_MODE_ADVANCE_NEXT_OFFSET: u32 = 0x0A;

/// The per-frame handler shape shared by every odd-indexed (per-frame) mode.
///
/// PORT: FUN_80025eec (the default handler - 12 of the 14 per-frame modes)
/// PORT: FUN_80025f2c (mode 13 MAPDISP)
/// PORT: FUN_80025f74 (mode 23 CARD)
/// REF: FUN_8001698C, FUN_80016444, FUN_80016B6C, FUN_80017978, FUN_801CE850
///
/// All three are the same eight-instruction skeleton, and the differences
/// between them are exactly the three fields below. Read off the disassembly
/// (`see ghidra/scripts/funcs/80025eec.txt`, `80025f2c.txt`, `80025f74.txt`)
/// and confirmed against the static-recomp renderings.
///
/// ```text
///   if (FUN_8001698C() != 0) return;      // frame-begin; non-zero = skipped
///   [overlay_hook()]                      // MAPDISP only
///   if (<body>() != 0) return;
///   FUN_80016B6C();                       // frame-end
/// ```
///
/// The **early-out is the load-bearing part**. `FUN_8001698C` returns `1`
/// when it took its frame-skip branch (`gp+0x3D8` set and neither
/// `_DAT_8007B938` nor `gp+0x55C` carrying bit `0x800`), in which case it has
/// already done the pad poll and a `VSync(0)` and the frame ends there - no
/// render, and crucially **no `FUN_80016B6C`**, so the SFX cue ring is neither
/// drained nor re-aged that frame. Modelling the handler as an unconditional
/// three-call sequence loses that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerFrameStage {
    /// Overlay hook called between the begin pass and the body. Only mode 13
    /// has one (`FUN_801CE850`, the world-map render tick in the slot-A
    /// overlay); `None` everywhere else.
    pub overlay_hook: Option<u32>,
    /// Which mid-frame driver runs.
    pub body: FrameBody,
    /// Whether a non-zero body return aborts before the frame-end pass.
    /// True for all three - kept explicit because it is the branch that
    /// makes the shape a state machine rather than a call list.
    pub body_can_abort: bool,
}

/// Per-frame staging plan for a mode. `None` for the INIT (even-indexed)
/// modes, which use [`mode_init_stage`] instead.
pub fn per_frame_stage(mode: GameMode) -> Option<PerFrameStage> {
    let stage = match mode {
        // Mode 13 MAPDISP - the only handler with an overlay hook, and the
        // only one that passes 0 to the master driver (`jal 0x80016444;
        // _clear a0` at 0x80025F4C).
        GameMode::MapdispMode => PerFrameStage {
            overlay_hook: Some(0x801C_E850),
            body: FrameBody::Master { param: 0 },
            body_can_abort: true,
        },
        // Mode 23 CARD - substitutes FUN_80017978 for the master driver.
        GameMode::CardMode => PerFrameStage {
            overlay_hook: None,
            body: FrameBody::CardDriver,
            body_can_abort: true,
        },
        // Every other per-frame (odd-indexed) mode routes through the
        // default handler.
        m if m.as_index() % 2 == 1 => PerFrameStage {
            overlay_hook: None,
            body: FrameBody::Master { param: 1 },
            body_can_abort: true,
        },
        _ => return None,
    };
    Some(stage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_frame_body_replaces_the_master_driver_outright() {
        // Mode 23 is the only mode whose body is not FUN_80016444.
        assert_eq!(
            per_frame_stage(GameMode::CardMode).unwrap().body,
            FrameBody::CardDriver
        );
        assert_eq!(
            CARD_FRAME_BODY,
            CardFrameBody {
                runs_master_driver: false,
                ticks_card_actor: true,
                returns: 0,
            }
        );
        // Every other per-frame mode does run it.
        for m in TABLE.iter().map(|e| e.mode) {
            let Some(stage) = per_frame_stage(m) else {
                continue;
            };
            if m == GameMode::CardMode {
                continue;
            }
            assert!(
                matches!(stage.body, FrameBody::Master { .. }),
                "{m:?} unexpectedly not on the master driver"
            );
        }
    }

    #[test]
    fn card_frame_body_never_aborts_the_frame_end_pass() {
        // FUN_80017978 ends `move v0,zero` with no other return path, so the
        // handler's abort test is structurally present but dead for CARD.
        assert_eq!(CARD_FRAME_BODY.returns, 0);
        assert!(per_frame_stage(GameMode::CardMode).unwrap().body_can_abort);
    }

    #[test]
    fn debug_mode_advance_row_geometry_matches_the_ported_table() {
        // The chord reads mode_table[cur].next out of the same 24-byte rows
        // TABLE transcribes, so the two must agree on the geometry.
        assert_eq!(DEBUG_MODE_ADVANCE_TABLE_BASE, 0x8007_078C);
        assert_eq!(DEBUG_MODE_ADVANCE_ROW_STRIDE, 24);
        assert_eq!(DEBUG_MODE_ADVANCE_NEXT_OFFSET, 0x0A);
        // The i16 sentinel the chord tests with `bltz` is what `next: None`
        // stands for, so at least one row has to carry it.
        assert!(
            TABLE.iter().any(|e| e.next.is_none()),
            "no self-managed mode - the -1 sentinel would be unreachable"
        );
    }

    #[test]
    fn core_state_reset_matches_retail_stores() {
        // FUN_80025CB4's literal stores, read off the disassembly
        // (li/sh + li/sw pairs at 0x80025CCC..0x80025D94).
        let r = CORE_STATE_RESET;
        assert_eq!(r.brightness, 0x80);
        assert_eq!(r.gte_h_zoom, 0xA0);
        assert_eq!(r.field_warm_entry, 0);
        assert_eq!(r.game_mode, 1, "CONFIG INIT hands off to CONFIG MODE");
        assert_eq!(r.data_field_index, 0xFFFF);
        assert_eq!(r.retail_ba36_b71c, 1);
        assert_eq!(r.b900, 0xFFFF_FFFF);
    }

    #[test]
    fn mode_init_stage_plans_match_retail_wrappers() {
        // The three thin INIT wrappers' overlay params + jal targets.
        let cfg = mode_init_stage(GameMode::ConfigInit).unwrap();
        assert_eq!(cfg.overlay_a_param, 0x4C);
        assert_eq!(cfg.overlay_entry, 0x801C_E8EC);
        assert!(cfg.runs_core_reset);
        let main = mode_init_stage(GameMode::MainInit).unwrap();
        assert_eq!(main.overlay_a_param, 2);
        assert_eq!(main.overlay_entry, 0x801D_6704);
        assert!(!main.runs_core_reset);
        let go = mode_init_stage(GameMode::GameOverInit).unwrap();
        assert_eq!(go.overlay_a_param, 7);
        assert_eq!(go.overlay_entry, 0x801C_E844);
        // Non-wrapper modes have no plan.
        assert!(mode_init_stage(GameMode::MainMode).is_none());
        assert!(mode_init_stage(GameMode::BattleInit).is_none());
    }

    /// The default handler covers 12 of the 14 per-frame modes; MAPDISP and
    /// CARD are the two exceptions, and each differs in exactly one field.
    #[test]
    fn per_frame_stage_separates_the_two_exception_handlers() {
        // FUN_80025EEC: the master driver with a0 = 1, no overlay hook.
        for m in [
            GameMode::ConfigMode,
            GameMode::MainMode,
            GameMode::BattleMode,
            GameMode::StrMode,
        ] {
            let s = per_frame_stage(m).unwrap();
            assert_eq!(s.body, FrameBody::Master { param: 1 }, "mode {m:?}");
            assert!(s.overlay_hook.is_none(), "mode {m:?}");
        }
        // FUN_80025F2C: `jal 0x801CE850` between the passes, and the master
        // driver takes 0 (`jal 0x80016444; _clear a0`).
        let map = per_frame_stage(GameMode::MapdispMode).unwrap();
        assert_eq!(map.overlay_hook, Some(0x801C_E850));
        assert_eq!(map.body, FrameBody::Master { param: 0 });
        // FUN_80025F74: a different driver entirely.
        let card = per_frame_stage(GameMode::CardMode).unwrap();
        assert_eq!(card.body, FrameBody::CardDriver);
        assert!(card.overlay_hook.is_none());
        // INIT (even-indexed) modes have no per-frame plan.
        assert!(per_frame_stage(GameMode::MainInit).is_none());
        assert!(per_frame_stage(GameMode::CardInit).is_none());
    }

    #[test]
    fn every_odd_mode_has_a_per_frame_stage_and_every_even_mode_has_none() {
        for i in 0..28usize {
            let m = GameMode::from_index(i).unwrap();
            assert_eq!(
                per_frame_stage(m).is_some(),
                i % 2 == 1,
                "mode {i} ({m:?}) parity"
            );
        }
    }

    #[test]
    fn a_frame_begin_skip_abandons_the_frame_before_the_world_ticks() {
        struct Noop;
        impl ModeHandler for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut w = World::default();
        let input = InputState::default();

        let before = w.field_frame_accum;
        w.frame_begin_skip = true;
        d.tick(&mut Noop, &mut w, &input);
        assert_eq!(
            w.field_frame_accum, before,
            "FUN_8001698C returned 1 - no frame ran"
        );
        assert!(!w.frame_begin_skip, "the request is consumed");
        assert_eq!(d.last_stage().unwrap().body, FrameBody::Master { param: 1 });

        // Default (nothing set) is the ordinary every-frame tick.
        d.tick(&mut Noop, &mut w, &input);
        assert!(w.field_frame_accum != before);
    }

    #[test]
    fn init_modes_ignore_the_frame_begin_skip() {
        struct Noop;
        impl ModeHandler for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::CardInit);
        let mut w = World::default();
        let before = w.field_frame_accum;
        w.frame_begin_skip = true;
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(
            w.field_frame_accum != before,
            "only the per-frame handlers carry the early-out"
        );
        assert!(d.last_stage().is_none());
    }

    #[test]
    fn resolve_frame_step_installs_the_floor_when_frameskip_is_off() {
        let mut w = World {
            frame_step_floor: 3,
            ..Default::default()
        };
        assert_eq!(w.resolve_frame_step(0x400, false), 3);
        assert_eq!(w.frame_step, 3);
        // With the gate on, a spike raises past the floor for one frame.
        assert_eq!(w.resolve_frame_step(0x400, true), 4);
        assert_eq!(w.resolve_frame_step(0x10, true), 3, "then decays to it");
    }

    /// Mode 0 CONFIG INIT is the sound-detach caller, and the `gp+0x804`
    /// latch makes every frame after the first a no-op.
    #[test]
    fn config_init_runs_the_sound_detach_exactly_once() {
        struct Noop;
        impl ModeHandler for Noop {
            fn run(&mut self, _m: GameMode, _w: &mut World, _i: &InputState) -> HandlerResult {
                HandlerResult::Continue
            }
        }
        let mut d = ModeDriver::new(GameMode::ConfigInit);
        let mut w = World::default();
        assert!(!w.sound_detach.is_detached());
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(w.sound_detach.is_detached());
        // A second frame in the same mode must not re-run it.
        assert!(!w.detach_sound());

        // MAIN INIT does not (its stage plan has runs_core_reset = false).
        let mut d = ModeDriver::new(GameMode::MainInit);
        let mut w = World::default();
        d.tick(&mut Noop, &mut w, &InputState::default());
        assert!(!w.sound_detach.is_detached());
    }

    /// The sound-release deadline is counted in vsyncs by `World::tick`, so
    /// it survives a cadence change unchanged.
    #[test]
    fn the_sound_release_timer_fires_through_the_world_tick() {
        let mut w = World::default();
        w.arm_sound_release(2);
        let mut fired = 0;
        for _ in 0..40 {
            w.tick();
            if w.take_pending_sound_release() {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "the deadline fires once and disarms");
        assert!(!w.sound_release.armed);
    }

    #[test]
    fn other_warp_stage_maps_sub_ids_to_overlays() {
        // sub 0..5 -> overlay params 0x4D..0x52 (PROT 972..977); sub 6
        // skips by 2 -> 0x55 (PROT 980, the dance overlay).
        let params: Vec<i32> = (0..7)
            .map(|s| other_warp_init_stage(s).unwrap().overlay_a_param)
            .collect();
        assert_eq!(params, vec![0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x55]);
        // Pinned attributions: fishing / slot / dance.
        assert_eq!(
            other_warp_init_stage(0).unwrap().overlay_a_param + 0x37F,
            972
        );
        assert_eq!(
            other_warp_init_stage(3).unwrap().overlay_a_param + 0x37F,
            975
        );
        assert_eq!(
            other_warp_init_stage(6).unwrap().overlay_a_param + 0x37F,
            980
        );
        // Entry table matches the 0x80010AE4 jump table.
        assert_eq!(other_warp_init_stage(2).unwrap().overlay_entry, 0x801C_EE80);
        assert_eq!(other_warp_init_stage(4).unwrap().overlay_entry, 0x801C_F00C);
        // Out-of-range sub-ids miss the retail `sltiu 7` bound.
        assert!(other_warp_init_stage(7).is_none());
        assert!(other_warp_init_stage(-1).is_none());
    }

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
    fn scene_mode_field_is_main_mode_not_mapdisp() {
        // Field/town gameplay is MainInit/MainMode (modes 2/3, game_mode
        // 0x03); MAPDISP (12/13) is the world-map display mode. The init
        // mode holds its successor's scene mode, same as Mapdisp/Battle/Str.
        assert_eq!(GameMode::MainInit.scene_mode(), SceneMode::Field);
        assert_eq!(GameMode::MainMode.scene_mode(), SceneMode::Field);
        assert_eq!(GameMode::MapdispInit.scene_mode(), SceneMode::WorldMap);
        assert_eq!(GameMode::MapdispMode.scene_mode(), SceneMode::WorldMap);
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
    fn scene_mode_for_card_modes_is_menu() {
        // The in-field pause menu runs under the CARD pair (game_mode 0x17 =
        // 23, CARD MODE): all six menu-open library captures hold
        // `_DAT_8007B83C = 0x17`. The init mode holds its successor's scene
        // mode like the other pairs.
        assert_eq!(GameMode::CardInit.scene_mode(), SceneMode::Menu);
        assert_eq!(GameMode::CardMode.scene_mode(), SceneMode::Menu);
        assert_eq!(GameMode::CardMode.as_index(), 0x17);
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
        let input = InputState::new();
        for _ in 0..3 {
            assert_eq!(d.tick(&mut h, &mut w, &input), HandlerResult::Continue);
        }
        assert_eq!(d.current(), GameMode::MapdispMode);
        assert_eq!(d.frames, 3);
        assert_eq!(d.frames_in_mode, 3);
        assert_eq!(w.mode, SceneMode::WorldMap);
    }

    #[test]
    fn handler_done_transitions_to_next_when_set() {
        struct DoneOnce {
            ticked: bool,
        }
        impl ModeHandler for DoneOnce {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                if self.ticked {
                    HandlerResult::Continue
                } else {
                    self.ticked = true;
                    HandlerResult::Done
                }
            }
        }
        // MainMode has next=ConfigInit - transition should land there.
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut h = DoneOnce { ticked: false };
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::ConfigInit);
        assert_eq!(d.frames_in_mode, 0);
    }

    #[test]
    fn handler_done_no_op_when_next_is_none() {
        // ConfigInit has next=None - Done should leave the mode unchanged.
        struct AlwaysDone;
        impl ModeHandler for AlwaysDone {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                HandlerResult::Done
            }
        }
        let mut d = ModeDriver::new(GameMode::ConfigInit);
        let mut h = AlwaysDone;
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::ConfigInit);
    }

    #[test]
    fn handler_goto_jumps_directly() {
        struct GoToBattle;
        impl ModeHandler for GoToBattle {
            fn run(&mut self, _: GameMode, _: &mut World, _: &InputState) -> HandlerResult {
                HandlerResult::GoTo(GameMode::BattleInit)
            }
        }
        let mut d = ModeDriver::new(GameMode::MapdispMode);
        let mut h = GoToBattle;
        let mut w = World::default();
        let input = InputState::new();
        d.tick(&mut h, &mut w, &input);
        assert_eq!(d.current(), GameMode::BattleInit);
    }

    #[test]
    fn field_demo_handler_spawns_actors_then_advances() {
        let mut d = ModeDriver::new(GameMode::MainInit);
        let mut h = FieldDemoHandler::new(4);
        let mut w = World::default();
        let input = InputState::new();
        // First tick: MainInit spawns actors and reports Done - driver
        // advances to MainInit's next entry (which is None per the table,
        // so we stay in MainInit). The actors should still be live.
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::Done);
        // 4 actors spawned at the staggered positions.
        assert!(w.actors[0].active);
        assert!(w.actors[3].active);
        assert!(!w.actors[4].active);
        assert_eq!(w.actors[1].move_state.world_x, 32 + 24);
    }

    #[test]
    fn field_demo_handler_main_mode_transitions_on_cross() {
        let mut d = ModeDriver::new(GameMode::MainMode);
        let mut h = FieldDemoHandler::new(0);
        let mut w = World::default();
        let mut input = InputState::new();
        // No press: stays.
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::Continue);
        assert_eq!(d.current(), GameMode::MainMode);
        // Cross press: transitions to MapdispInit.
        input.set_pad(crate::input::PadButton::Cross.mask());
        let r = d.tick(&mut h, &mut w, &input);
        assert_eq!(r, HandlerResult::GoTo(GameMode::MapdispInit));
        assert_eq!(d.current(), GameMode::MapdispInit);
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
