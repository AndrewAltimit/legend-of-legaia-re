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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeInitStage {
    /// `FUN_8003EBE4` first argument (extraction PROT entry `param + 0x37F`).
    pub overlay_a_param: i32,
    /// VA of the loaded overlay's entry the handler `jal`s after the wait.
    pub overlay_entry: u32,
    /// Whether the handler runs [`CORE_STATE_RESET`] before staging.
    pub runs_core_reset: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

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
