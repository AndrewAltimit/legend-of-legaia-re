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

    /// Map a game mode to the [`SceneMode`] the World should run in, **with
    /// no warp sub-id in hand**.
    ///
    /// This is [`Self::scene_mode_with_warp`] passing `None`, and it therefore
    /// cannot answer for the `OTHER` pair (24 / 25) - it returns
    /// [`SceneMode::Title`] there. Callers that hold a mode word taken from a
    /// live machine (a capture, a trace) hold the sub-id register too and
    /// should pass it; see [`WARP_SUB_ID_ADDR`].
    pub fn scene_mode(self) -> SceneMode {
        self.scene_mode_with_warp(None)
    }

    /// Map the retail `(game_mode, warp sub-id)` **pair** to the [`SceneMode`]
    /// the World should run in. Init modes hold their successor's scene mode
    /// (init code prepares assets for the per-frame mode).
    ///
    /// The second argument is retail's own discriminator, the signed halfword
    /// at [`WARP_SUB_ID_ADDR`]; `None` means "not observed". It is load-bearing
    /// for exactly one mode pair, and that pair is where the port's `SceneMode`
    /// space is *finer* than the retail mode word: modes 24 / 25 (`OTHER` /
    /// `OTHER MODE`) host all five warp minigames, and only the sub-id says
    /// which. Every other mode ignores it.
    ///
    /// Two of the seven sub-ids (`1` / `2`) are dev modules the engine does not
    /// implement, and out-of-range values are a desynced read; all three fall
    /// back to [`SceneMode::Title`], the same answer the mode word alone gives.
    pub fn scene_mode_with_warp(self, warp_sub_id: Option<i16>) -> SceneMode {
        // The OTHER pair first: it is the one arm the mode word cannot decide.
        if matches!(self, GameMode::OtherInit | GameMode::OtherMode) {
            return warp_sub_id
                .and_then(|s| u8::try_from(s).ok())
                .and_then(crate::minigame_entry::MinigameSubId::from_sub_id)
                .and_then(|slot| slot.scene_mode())
                .unwrap_or(SceneMode::Title);
        }
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

impl GameMode {
    /// The **per-frame** retail mode a [`SceneMode`] runs under - the inverse
    /// of [`Self::scene_mode_with_warp`], and the map anything translating the
    /// engine's dispatch state back into `_DAT_8007B83C` space needs.
    ///
    /// Deliberately lossy in one direction and only that one: the five warp
    /// minigames all answer `OtherMode`, because in retail they *are* the same
    /// mode and only [`WARP_SUB_ID_ADDR`] separates them. Recovering the
    /// original `SceneMode` therefore needs the sub-id back;
    /// [`crate::minigame_entry::MinigameSubId::scene_mode`] is the other half.
    ///
    /// `None` for [`SceneMode::Title`]. The port uses that variant for "no
    /// scene loaded" - boot, an unhosted world, the gap between scenes - which
    /// is a state the retail mode word has no single answer for, not a mode.
    pub fn for_scene_mode(mode: SceneMode) -> Option<GameMode> {
        Some(match mode {
            SceneMode::Field => GameMode::MainMode,
            SceneMode::WorldMap => GameMode::MapdispMode,
            SceneMode::Battle => GameMode::BattleMode,
            SceneMode::Menu => GameMode::CardMode,
            SceneMode::Cutscene => GameMode::StrMode,
            SceneMode::Dance
            | SceneMode::Fishing
            | SceneMode::SlotMachine
            | SceneMode::BakaFighter
            | SceneMode::MuscleDome => GameMode::OtherMode,
            SceneMode::Title => return None,
        })
    }
}

/// PSX-virtual address of retail's **warp sub-id register** - the second half
/// of the `(game_mode, sub-id)` pair [`GameMode::scene_mode_with_warp`] needs.
///
/// Read off the disassembly at both ends:
///
/// - **Writer**, the field VM's op `0x3E` door-warp arm (`FUN_801DE840` case
///   `0x3e`, `0x801E07B0..0x801E07B8`): `addiu v1,v1,-0x64` computes
///   `sub_id = op0 - 100`, then `sh v1,-0x45cc(v0)` with `v0 = lui 0x8008`
///   stores it here, in the delay slot of the same instruction pair that puts
///   `0x18` into the mode word `_DAT_8007B83C`.
/// - **Reader**, the mode-24 `OTHER` init (`FUN_80025980`, `0x80025A14` and
///   `0x80025A50`): `lh a0,-0x45cc(a0)` picks the overlay-load param and
///   `lh v1,-0x45cc(v1)` indexes the seven-wide entry table at `0x80010AE4`
///   behind an `sltiu 7` bound. The init's last act is `li v0,0x19; sh v0,
///   -0x47c4(at)` - it hands the mode word to `OTHER MODE` and **leaves the
///   sub-id register standing**, which is what makes the pair readable for
///   every frame the minigame runs, not just the init frame.
///
/// Both loads are `lh`, so the register is a signed 16-bit word.
// REF: FUN_801DE840 case 0x3e (writer), FUN_80025980 (reader)
pub const WARP_SUB_ID_ADDR: u32 = 0x8007_BA34;

/// Read the warp sub-id register out of a main-RAM image (signed 16-bit LE).
///
/// The companion of
/// [`read_game_mode`](crate::capture_observations::cutscene_trigger_corpus::read_game_mode):
/// a capture-side consumer needs both words to resolve a [`SceneMode`], and
/// taking only the mode word is what makes a live minigame frame read as
/// `Title`.
pub fn read_warp_sub_id(main_ram: &[u8]) -> Option<i16> {
    let off = (WARP_SUB_ID_ADDR - 0x8000_0000) as usize;
    legaia_bytes::i16_le(main_ram, off)
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
    /// The warp sub-id register [`WARP_SUB_ID_ADDR`], which the driver has to
    /// carry beside the mode word: the mode word alone cannot say which of the
    /// five minigames modes 24 / 25 are running. `None` until a door-warp
    /// stages one, mirroring the fact that retail's register is only meaningful
    /// once the `0x3E` arm has written it.
    warp_sub_id: Option<i16>,
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
            warp_sub_id: None,
        }
    }

    pub fn current(&self) -> GameMode {
        self.current
    }

    /// The staged warp sub-id ([`WARP_SUB_ID_ADDR`]).
    pub fn warp_sub_id(&self) -> Option<i16> {
        self.warp_sub_id
    }

    /// Stage a warp sub-id, the way the field VM's `0x3E` arm writes
    /// `_DAT_8007BA34` before handing the mode word to `0x18`. Retail never
    /// clears the register, so neither does this - a host that wants it gone
    /// passes `None` explicitly.
    pub fn set_warp_sub_id(&mut self, sub_id: Option<i16>) {
        self.warp_sub_id = sub_id;
    }

    /// The [`SceneMode`] the driver's current `(mode, sub-id)` pair resolves
    /// to. This, not [`GameMode::scene_mode`], is what the driver installs into
    /// the World each frame.
    pub fn scene_mode(&self) -> SceneMode {
        self.current.scene_mode_with_warp(self.warp_sub_id)
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
        // idempotent - the World's tick path keys off it. Resolved from the
        // `(mode, warp sub-id)` PAIR: keying on the mode word alone drops a
        // live fishing / dance / casino / duel / dome session into
        // `SceneMode::Title`, because all five share mode `0x19`.
        world.mode = self.scene_mode();
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

/// One static store into the master mode word `_DAT_8007B83C`.
///
/// Every mode transition retail performs is one of these: an `sh` of a
/// literal into `0x8007B83C`, reached as `lui 0x8007 + 0xB83C`,
/// `lui 0x8008 - 0x47C4`, or `0x524(gp)`. The three encodings are the same
/// address, which is why a scan that only knows one of them under-reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeWordStore {
    /// VA of the `sh`.
    pub store_pc: u32,
    /// The mode the store writes.
    pub mode: GameMode,
}

/// The mode an INIT handler hands its RUN sibling, and the store that does it.
///
/// This is the INIT column's own exit, not a table field: `ModeEntry::next`
/// is the mode table's `+0x0A` word, which the *debug* advance chord reads,
/// while a live INIT handler ends by storing its successor itself. Every row
/// below is one such store, located by scanning each image for writes to
/// `_DAT_8007B83C` in all three addressing forms and reading back the literal
/// last loaded into the stored register.
///
/// `MonsterTest` is the row that shows why this cannot be `index + 1`: mode 4
/// stores `0`, so it bounces to the debug menu without ever reaching mode 5
/// ([`ModeInitBare`]).
///
/// `ReadInit`'s store is the only one outside `SCUS_942.54` - mode 16 jumps
/// into whatever sits in overlay slot A, and on the boot path that is
/// `init.pak` (PROT 0895), whose logo pass ends `li v0,0x11` /
/// `sh v0,-0x47c4(v1)` at `0x801CEC94`, in the delay slot of a `jal`.
///
/// `GameOverInit` is deliberately absent: its handler's hand-off lives in
/// PROT 0902, which no scan here covers, so the port does not claim a
/// successor for mode 18.
pub const INIT_HANDOFFS: &[(GameMode, ModeWordStore)] = &[
    (
        GameMode::ConfigInit,
        ModeWordStore {
            store_pc: 0x8002_5D20,
            mode: GameMode::ConfigMode,
        },
    ),
    (
        GameMode::MainInit,
        ModeWordStore {
            store_pc: 0x8002_5E50,
            mode: GameMode::MainMode,
        },
    ),
    (
        GameMode::MonsterTest,
        ModeWordStore {
            store_pc: 0x8002_6120,
            mode: GameMode::ConfigInit,
        },
    ),
    (
        GameMode::MapdispInit,
        ModeWordStore {
            store_pc: 0x8002_5DF8,
            mode: GameMode::MapdispMode,
        },
    ),
    (
        GameMode::ReadInit,
        ModeWordStore {
            store_pc: 0x801C_EC94,
            mode: GameMode::ReadMode,
        },
    ),
    (
        GameMode::BattleInit,
        ModeWordStore {
            store_pc: 0x8005_5E4C,
            mode: GameMode::BattleMode,
        },
    ),
    (
        GameMode::CardInit,
        ModeWordStore {
            store_pc: 0x8002_5974,
            mode: GameMode::CardMode,
        },
    ),
    (
        GameMode::OtherInit,
        ModeWordStore {
            store_pc: 0x8002_5B04,
            mode: GameMode::OtherMode,
        },
    ),
];

/// The mode an INIT handler leaves the word at, or `None` when `mode` is not
/// an INIT mode whose hand-off is pinned ([`INIT_HANDOFFS`]).
pub fn init_successor(mode: GameMode) -> Option<GameMode> {
    INIT_HANDOFFS
        .iter()
        .find(|(m, _)| *m == mode)
        .map(|(_, s)| s.mode)
}

/// The retail **boot mode chain**, in order, each step with the store that
/// takes it.
///
/// Read end to end off the disassembly, and it corrects two readings that
/// have been carried in prose:
///
/// * The title screen does **not** run under mode `0x10`. Mode 16 `READ INIT`
///   is one frame: `FUN_8002612C` jumps into slot A, which on the boot path
///   is `init.pak`'s logo pass, and that pass ends by storing `0x11`
///   (`0x801CEC94`). The publisher logos animate under `READ MODE` as
///   ordinary actors.
/// * The title runs under `CARD MODE` (`0x17`) - the same mode word as the
///   in-field pause menu. `init.pak`'s phase-3 arm runs
///   [`CORE_STATE_RESET`] and stores `0x16` `CARD INIT` at `0x801CF4D4`
///   whenever the entry word `0x8007BB00` is non-zero (`init.pak` raises it
///   itself), and mode 22's handler `FUN_8002574C` hands the word to `0x17`
///   at `0x80025974`. That is why the title dispatcher `FUN_801DD35C` is
///   resident in the *menu* overlay and is spawned by mode 22: the front-end
///   and the pause menu are one mode.
///
/// The `0` arm beside that store (`0x801CF4E4`, entry word zero) is the dev
/// route: it writes `CONFIG INIT`, the debug menu, instead.
///
/// The chain's last two steps are the title dispatcher's own NEW GAME store
/// (`0x801DFC00`, `legaia_engine_vm::title_overlay`) and mode 2's hand-off.
pub const BOOT_MODE_CHAIN: &[ModeWordStore] = &[
    ModeWordStore {
        store_pc: 0x8001_D5B8,
        mode: GameMode::ReadInit,
    },
    ModeWordStore {
        store_pc: 0x801C_EC94,
        mode: GameMode::ReadMode,
    },
    ModeWordStore {
        store_pc: 0x801C_F4D4,
        mode: GameMode::CardInit,
    },
    ModeWordStore {
        store_pc: 0x8002_5974,
        mode: GameMode::CardMode,
    },
    ModeWordStore {
        store_pc: 0x801D_FC00,
        mode: GameMode::MainInit,
    },
    ModeWordStore {
        store_pc: 0x8002_5E50,
        mode: GameMode::MainMode,
    },
];

/// What a mode change costs, beside the new word.
///
/// Retail's dispatcher runs a fixed sequence whenever the handler it just
/// called left `gp[0x524] != gp[0x494]` (`0x800161B8..0x80016200`):
///
/// ```text
///   FUN_8003DE7C(0)          ; CD read-wait poll
///   FUN_8003ED04(0)          ; overlay-load wait
///   FUN_80016230()           ; mode-transition routine
///   FUN_80058104(0)          ; sound teardown
///   FUN_8001822C(gp+0x4F8)   ; re-publish the pad reports
///   gp+0x3D8 = 0             ; frame-begin-skip flag
///   gp+0x538 = 0             ; 0x8007B850, the live pad word
///   0x8007B938 = 0
///   gp+0x55C = 0             ; 0x8007B874, the pad *edge* word
///   gp+0x564 = gp+0x494 = new mode
/// ```
///
/// The two pad clears are the half with observable behaviour: the button that
/// caused a transition is not also delivered as the new mode's first input.
/// The engine's equivalent is [`crate::input::InputState::clear_edges`], which
/// drops the edges and leaves what is held alone.
///
/// The `0x8007B938` clear is a fourth store the earlier three-clear reading of
/// this block missed, and `gp+0x564` / `gp+0x494` are *copies of the new mode*,
/// not clears.
// REF: FUN_8003DE7C, FUN_8003ED04, FUN_80016230, FUN_80058104, FUN_8001822C
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeChangeEdge {
    /// The mode the word held before the change (`gp+0x494`).
    pub from: GameMode,
    /// The mode it holds now (`gp+0x524`).
    pub to: GameMode,
    /// The engine performed the pad-edge swallow (`gp+0x538` / `gp+0x55C`).
    pub swallowed_pad_edges: bool,
    /// The engine cleared the frame-begin-skip flag (`gp+0x3D8`).
    pub cleared_frame_begin_skip: bool,
}

/// The staging an INIT mode asks its host for, in the one shape a host can
/// match on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInitPlan {
    /// The [`ModeInitStage`] wrapper shape - an overlay-A request plus the
    /// entry the handler calls once the load lands.
    Stage(ModeInitStage),
    /// The mode-24 warp dispatcher's per-sub-id staging
    /// ([`other_warp_init_stage`]).
    Warp(ModeInitStage),
    /// An INIT handler that stages nothing ([`ModeInitBare`]).
    Bare(ModeInitBare),
}

/// One frame's worth of mode-table dispatch, as a [`ModeSeat`] resolved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeFrame {
    /// The mode word this frame ran under.
    pub game_mode: GameMode,
    /// The [`SceneMode`] the `(mode, warp sub-id)` pair resolves to.
    pub scene_mode: SceneMode,
    /// The transition this frame opened with, if the word changed.
    pub edge: Option<ModeChangeEdge>,
    /// The INIT column's staging plan, for an INIT (even-indexed) mode. The
    /// seat resolves it and then performs that mode's hand-off store, so a
    /// host sees each plan on exactly the frame retail's handler ran.
    pub init: Option<ModeInitPlan>,
    /// The per-frame staging plan, for a per-frame (odd-indexed) mode.
    pub stage: Option<PerFrameStage>,
    /// Whether this mode's handler calls the master frame driver - `false`
    /// only for `CARD MODE` ([`runs_master_frame_driver`]).
    pub runs_master_driver: bool,
}

/// A host's **seat at the mode table**: the port's copy of `_DAT_8007B83C`,
/// plus the dispatch retail's `main` loop performs around it.
///
/// PORT: FUN_80015E90 (the mode-table loop, `0x8001615C..0x8001620C`)
///
/// Retail's outermost level is a three-line loop over one halfword: index the
/// 28-entry table at `0x8007078C`, call `+0x10`, and if the handler changed
/// the word, run the transition edge. The port has the table
/// ([`TABLE`]), the per-frame level ([`per_frame_stage`]) and the inner frame
/// driver (`World::tick`); this is the outer level, and the thing that makes
/// it a seat rather than a mirror is that its **writes are the port's own
/// transitions**. A host calls [`Self::enter`] where retail's code stores the
/// word, and gets back the INIT column's staging plan for that mode.
///
/// ## What owns what
///
/// The word is the seat's. [`SceneMode`] stays the scene sessions' - they own
/// the loaded assets, and the port's minigames are resident rules engines
/// rather than paged overlays, so a session outlives the mode word that
/// staged it. The two are reconciled once per frame by
/// [`Self::adopt_scene_mode`], which writes the word when a session moved the
/// world somewhere the word does not name. That direction is lossy exactly
/// where [`GameMode::for_scene_mode`] says it is (the five warp minigames all
/// answer `OTHER MODE`), so the seat stages the sub-id alongside, and
/// [`Self::scene_mode`] round-trips.
///
/// An INIT frame is never adopted away: the word an [`Self::enter`] left is
/// what the frame runs under, and the hand-off to the RUN sibling
/// ([`INIT_HANDOFFS`]) happens after the host has staged what the plan named.
#[derive(Debug)]
pub struct ModeSeat {
    driver: ModeDriver,
    /// `gp+0x494` - the word as of the last edge, which is what the loop's
    /// `bne` compares against.
    previous: GameMode,
    /// Mode changes taken since the seat opened.
    edges: u64,
    /// `_DAT_8007BB00` - the **front-end entry word**, the companion store of
    /// [`crate::field_submode::request_card_mode`].
    ///
    /// It is the flag `init.pak`'s hand-off arm reads to choose between the
    /// front end and the debug menu ([`Self::boot_handoff`]), and the flag the
    /// title dispatcher's `Init` reads to route to `0x11` instead of the
    /// retail-unreachable `0x02` (`0x801DD97C`,
    /// [`legaia_engine_vm::title_overlay::ENTRY_WORD_ADDR`]). `init.pak`
    /// raises it itself on a cold boot, which is why the seat opens with it
    /// set.
    entry_word: u32,
}

impl ModeSeat {
    /// Open a seat at the retail boot mode: `0x10` `READ INIT`, the word
    /// `0x8001D5B8` writes before the first dispatch
    /// ([`BOOT_MODE_CHAIN`]).
    pub fn new_at_boot() -> Self {
        Self::new(GameMode::ReadInit)
    }

    /// Open a seat at an arbitrary mode (a host resuming mid-game, a test).
    pub fn new(start: GameMode) -> Self {
        Self {
            driver: ModeDriver::new(start),
            previous: start,
            edges: 0,
            entry_word: legaia_engine_vm::title_overlay::ENTRY_WORD_COLD_BOOT,
        }
    }

    /// The current mode word.
    pub fn game_mode(&self) -> GameMode {
        self.driver.current()
    }

    /// The word as of the last edge (`gp+0x494`).
    pub fn previous_mode(&self) -> GameMode {
        self.previous
    }

    /// Mode changes taken since the seat opened.
    pub fn edges(&self) -> u64 {
        self.edges
    }

    /// The staged warp sub-id ([`WARP_SUB_ID_ADDR`]).
    pub fn warp_sub_id(&self) -> Option<i16> {
        self.driver.warp_sub_id()
    }

    /// The `(mode, sub-id)` pair's [`SceneMode`].
    pub fn scene_mode(&self) -> SceneMode {
        self.driver.scene_mode()
    }

    /// The debug label of the current mode's table row.
    pub fn mode_name(&self) -> &'static str {
        self.driver.entry().name
    }

    /// The front-end entry word `_DAT_8007BB00`.
    pub fn entry_word(&self) -> u32 {
        self.entry_word
    }

    /// Request the front end, both stores: the mode word to `CARD INIT` and
    /// the entry word raised.
    ///
    /// This is [`crate::field_submode::request_card_mode`]'s pair applied to
    /// the seat - the leaf is seven instructions and two stores, and this is
    /// where they land. Retail's field image calls it to hand the frame to
    /// the card / title screen; the port's hosts call it to open the pause
    /// menu and to re-enter the front end.
    pub fn request_card_mode(&mut self) {
        let req = crate::field_submode::request_card_mode();
        if let Some(mode) = GameMode::from_index(req.game_mode as usize) {
            self.driver.jump_to(mode);
        }
        self.entry_word = req.flag;
    }

    /// The mode the boot pass hands off to once the publisher logos finish -
    /// retail's `init.pak` phase-3 arm.
    ///
    /// `0x801CF490..0x801CF4E8`: on `state[0x801F3EB0] == 3` it runs
    /// [`CORE_STATE_RESET`] and then branches on the entry word - non-zero
    /// stores `0x16` `CARD INIT` (the front end) at `0x801CF4D4`, zero stores
    /// `0` `CONFIG INIT` (the debug menu) at `0x801CF4E4` and clears the word.
    /// The seat performs the branch and the store, and returns the mode so a
    /// host can raise the screen the new mode owns.
    pub fn boot_handoff(&mut self) -> GameMode {
        let to = if self.entry_word == 0 {
            GameMode::ConfigInit
        } else {
            GameMode::CardInit
        };
        if to == GameMode::ConfigInit {
            self.entry_word = 0;
        }
        self.driver.jump_to(to);
        to
    }

    /// Stage the warp sub-id, as the field VM's `0x3E` door-warp arm does
    /// before handing the word to `OTHER INIT`.
    pub fn stage_warp(&mut self, sub_id: Option<i16>) {
        self.driver.set_warp_sub_id(sub_id);
    }

    /// Write the mode word - the port's counterpart of one of retail's `sh`
    /// stores. The edge is taken on the next [`Self::frame`], the way the
    /// dispatcher takes it after the handler returns.
    pub fn write_mode(&mut self, mode: GameMode) {
        self.driver.jump_to(mode);
    }

    /// Enter an INIT mode **now**: take the edge, resolve the INIT column's
    /// staging plan, and leave the word at the mode's RUN sibling.
    ///
    /// This is one retail INIT handler's whole body minus the overlay load
    /// the port replaces with native scene entry: the caller performs what
    /// the returned plan names, and the seat performs the hand-off store
    /// ([`INIT_HANDOFFS`]). It is [`Self::frame`] with the word written
    /// first, so a host that enters a mode mid-frame and a host that lets the
    /// seat reach it on its own run the same code. `None` comes back for a
    /// mode that stages nothing.
    ///
    /// `world` is needed for the edge itself ([`ModeChangeEdge`]).
    pub fn enter(&mut self, mode: GameMode, world: &mut World) -> Option<ModeInitPlan> {
        self.write_mode(mode);
        self.frame_inner(world, true).init
    }

    /// The INIT column's plan for `mode`, without entering it.
    pub fn init_plan(&self, mode: GameMode) -> Option<ModeInitPlan> {
        if matches!(mode, GameMode::OtherInit) {
            return self
                .driver
                .warp_sub_id()
                .and_then(other_warp_init_stage)
                .map(ModeInitPlan::Warp);
        }
        if let Some(stage) = mode_init_stage(mode) {
            return Some(ModeInitPlan::Stage(stage));
        }
        mode_init_bare(mode).map(ModeInitPlan::Bare)
    }

    /// Reconcile the word with a [`SceneMode`] a scene session moved the
    /// world to. Returns the write it made, if any.
    ///
    /// Never overwrites an INIT frame, and never writes for
    /// [`SceneMode::Title`] - the port uses that variant for "no scene
    /// loaded", which is a state the retail word has no single answer for.
    pub fn adopt_scene_mode(&mut self, scene: SceneMode) -> Option<GameMode> {
        if init_successor(self.driver.current()).is_some() {
            return None;
        }
        if self.driver.scene_mode() == scene {
            return None;
        }
        let target = GameMode::for_scene_mode(scene)?;
        if matches!(target, GameMode::OtherMode) {
            // The lossy arm: five scene modes share `OTHER MODE`, so the word
            // alone would not round-trip. Stage the sub-id retail's warp arm
            // would have left standing.
            let sub = crate::minigame_entry::MinigameSubId::ALL
                .iter()
                .find(|s| s.scene_mode() == Some(scene))
                .map(|s| i16::from(s.sub_id()));
            self.driver.set_warp_sub_id(sub);
        }
        self.driver.jump_to(target);
        Some(target)
    }

    /// Take the mode-change edge if the word moved since the last one.
    ///
    /// `swallow` decides whether the edge performs retail's pad-edge clears,
    /// and the answer is not "always" - see [`Self::frame`] for why an
    /// adopted change must not.
    fn take_edge(&mut self, world: &mut World, swallow: bool) -> Option<ModeChangeEdge> {
        let to = self.driver.current();
        if to == self.previous {
            return None;
        }
        let from = self.previous;
        if swallow {
            // The two pad clears (`gp+0x538` / `gp+0x55C`): the button that
            // caused the transition is not delivered again as the new mode's
            // first input.
            world.input.clear_edges();
        }
        // `gp+0x3D8`.
        world.frame_begin_skip = false;
        self.previous = to;
        self.edges += 1;
        Some(ModeChangeEdge {
            from,
            to,
            swallowed_pad_edges: swallow,
            cleared_frame_begin_skip: true,
        })
    }

    /// Drive one frame of the mode table: take any pending edge, resolve the
    /// frame's staging plan - the INIT column's for an INIT mode, the
    /// per-frame column's otherwise - and hand the word on where the INIT
    /// handler would have.
    ///
    /// A host calls this once per frame before its own frame body; the
    /// [`ModeFrame::runs_master_driver`] field is the same rule
    /// `World::tick` applies internally, surfaced so a host can skip its own
    /// render / actor work under `CARD MODE` too.
    ///
    /// **This edge does not swallow the pad, and the reason is an ordering
    /// mismatch rather than a fidelity choice.** Retail's transition block
    /// clears the pad words and the *next* loop pass polls the pad fresh, so
    /// the clear only ever discards the mode it left. The port's hosts publish
    /// a pad word immediately *before* each tick, so clearing here would
    /// discard this frame's own input - the button the player is pressing at
    /// the new mode, not the one that left the old one. The swallow therefore
    /// belongs to the transitions a host performs synchronously mid-frame,
    /// which is what [`Self::enter`] does.
    pub fn frame(&mut self, world: &mut World) -> ModeFrame {
        self.frame_inner(world, false)
    }

    fn frame_inner(&mut self, world: &mut World, swallow: bool) -> ModeFrame {
        let edge = self.take_edge(world, swallow);
        let mode = self.driver.current();
        let out = ModeFrame {
            game_mode: mode,
            scene_mode: self.driver.scene_mode(),
            edge,
            init: self.init_plan(mode),
            stage: per_frame_stage(mode),
            runs_master_driver: runs_master_frame_driver(mode),
        };
        // An INIT mode is one frame. Retail's handler ends by storing its own
        // successor ([`INIT_HANDOFFS`]), so the word has moved on by the time
        // the loop comes round again - which is also what keeps a seat from
        // parking on an INIT mode nothing dispatched.
        if let Some(next) = init_successor(mode) {
            self.driver.jump_to(next);
        }
        out
    }
}

impl Default for ModeSeat {
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
// PARTIALLY WIRED: `crate::minigame_entry::MinigameSubId::prot_index` /
// `overlay_init_va` call this for every mode-24 door-warp, and the field-VM
// `0x3E` arm now reaches it - `SceneHost::drain_minigame_warp` reads the
// staged sub-id's PROT entry off the disc, parses its tables and installs the
// session. So the staging *plan* has a live caller and the seven PROT indices
// are no longer duplicated anywhere. [`ModeSeat::init_plan`] reaches it from
// the other side: a host that enters `OTHER INIT` with a sub-id staged gets
// this plan back.
//
// What is still absent is the mode-table overlay-*residency* model: nothing
// loads the image at a base and `jalr`s `overlay_entry` out of the
// `0x80010AE4` table. The engine's minigames are resident Rust rules engines
// entered as suspended in-place scene modes, which is also why the
// warp-shared reset half of the retail body has no state to clear. Same
// missing prerequisite as `crate::overlay_loader`.
//
// The reference scan sharpens what "no dispatcher" means here: `0x80025980`
// has **no `jal` anywhere on the disc**. Its one reference of any form is the
// word at `0x800709DC`, which is `mode_table[24] + 0x10` in the 28 x 24-byte
// table at `0x8007078C` - so retail reaches it only by indexing that table,
// and `legaia_asset::mode_table` already recovers the table from the disc.
// The gap is a dispatcher that calls the slot, not a table to call it from.
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

/// An INIT handler that is **not** the [`ModeInitStage`] wrapper shape.
///
/// Three of the mode table's INIT slots skip staging entirely. They open no
/// overlay request, run no [`CORE_STATE_RESET`], and wait on nothing - which
/// is the whole point of recording them: a reader who assumes every INIT row
/// stages an overlay will look for a load that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInitBare {
    /// The handler's entire body is a half-word store of the mode index into
    /// `_DAT_8007B83C`, then `jr ra`.
    SetsMode(GameMode),
    /// The handler's entire body is a frame, one `jal` to this VA, and the
    /// epilogue. Nothing is passed and nothing is returned.
    Calls(u32),
}

/// The bare INIT handlers, beside [`mode_init_stage`]'s staging ones.
///
// PARTIALLY WIRED: [`ModeSeat::init_plan`] resolves this for every mode a
// host enters, and `engine-shell`'s `BootSession` is that host - it enters
// `MAIN INIT` at field entry and `CARD INIT` at menu open, and the seat
// performs each mode's own hand-off store ([`INIT_HANDOFFS`]). So the INIT
// column is walked.
//
// What the walk does *not* do is stage an overlay. Two of these three rows
// have no destination even in principle - mode 4 is a bounce back to the debug
// menu, and mode 16's `jal` target resolves per image - and mode 20 is the row
// a battle-entry pass could take, which the port reaches through
// `SceneMode::Battle` instead. The residency model is the same one
// `crate::overlay_loader` is missing.
//
// The `(mode, warp sub-id)` bridge ([`GameMode::scene_mode_with_warp`]) was
// never the blocker here and the note that said so is superseded: it closes
// the mode-24 / 25 ambiguity, which is about which SceneMode a *running* mode
// maps to.
// PORT: FUN_8002611c (mode-4 MONSTER TEST INIT)
// PORT: FUN_8002612c (mode-16 READ INIT)
// PORT: FUN_800565d8 (mode-20 BATTLE INIT)
// REF: FUN_80055b6c (the battle-scene setup mode 20 calls)
/// | Mode | Handler | Body |
/// |---|---|---|
/// | 4 MONSTER TEST INIT | `FUN_8002611C` | `sh zero, _DAT_8007B83C`; `jr ra` |
/// | 16 READ INIT | `FUN_8002612C` | `jal 0x801CE9C0` |
/// | 20 BATTLE INIT | `FUN_800565D8` | `jal 0x80055B6C` |
///
/// **Mode 4 bounces.** Its four instructions write mode `0`, so MONSTER TEST
/// INIT hands control straight back to CONFIG (the debug menu) on the
/// dispatcher's next pass without ever reaching mode 5 MONSTER MODE - the
/// same body shape as the mode-13-column reset leaf `FUN_8002B904`, and the
/// reason the debug menu's monster-test entry appears to do nothing. The
/// store is `sh`, matching every other retail writer of that word.
///
/// **Mode 16 jumps into slot A without loading it.** Unlike modes 0/2/18/24
/// it never calls the overlay loader `FUN_8003EBE4`, so `0x801CE9C0` is
/// whatever image is resident in overlay slot A. That does **not** make it a
/// dead jump: in PROT 0895 (`init.pak`) the VA is a real entry with a clean
/// `addiu sp,sp,-0x230` prologue, and it is the publisher-logo boot pass -
/// see [`READ_INIT_TARGET`] and `crate::publisher_logos`.
///
/// **Mode 20 is the one live row.** `FUN_80055B6C` is the battle-scene setup
/// entry, resident in `SCUS_942.54`, so BATTLE INIT is a real call and not a
/// stub. Battle entry in the port does not run through the mode table - see
/// [`GameMode::scene_mode`] - so this records the retail chain rather than
/// driving it.
pub fn mode_init_bare(mode: GameMode) -> Option<ModeInitBare> {
    match mode {
        GameMode::MonsterTest => Some(ModeInitBare::SetsMode(GameMode::ConfigInit)),
        GameMode::ReadInit => Some(ModeInitBare::Calls(READ_INIT_TARGET)),
        GameMode::BattleInit => Some(ModeInitBare::Calls(0x8005_5B6C)),
        _ => None,
    }
}

/// The VA mode 16 READ INIT `jal`s.
///
/// It resolves per image, because `FUN_8002612C` performs no overlay load of
/// its own: whatever slot A last received is what mode 16 enters. In the boot
/// overlay PROT 0895 (`init.pak`) it is a **real entry** - slot-A base
/// `0x801CE818` + `0x1A8`, opening `addiu sp,sp,-0x230` - and it is the body
/// that uploads the four publisher-logo TIMs, spawns the two boot actors, and
/// leaves game mode `0x11`. In the debug-menu overlay the same VA is interior
/// to `FUN_801CE97C`'s global-clear block, which is the reading this constant
/// used to carry and which held only because 0895 was not yet mapped.
pub const READ_INIT_TARGET: u32 = 0x801C_E9C0;

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
/// REF: FUN_80017978 - ported, but the `PORT:` tag lives on
/// [`per_frame_stage`], the function that materialises this descriptor. A tag
/// here anchors liveness to a plain data `struct` with no `impl`, which falls
/// back to *file* scope and reports the whole of `mode.rs` as the port's reach.
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
/// REF: FUN_80025eec, FUN_80025f2c, FUN_80025f74 - the three handlers this
/// shape describes. Their `PORT:` tags live on [`per_frame_stage`], which is
/// what resolves a mode to one of them; a tag here would anchor liveness to a
/// plain data `struct` with no `impl`, i.e. to the whole file.
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

/// Does the per-frame handler for `mode` run the **master frame driver**
/// `FUN_80016444`?
///
/// This is [`per_frame_stage`]'s `body` field asked as a question, and it is
/// the one bit of the mode table that decides whether a frame advances the
/// actor pool at all. `FUN_80016444` is the five `FUN_8002519C` tick passes,
/// the five render passes and the display flip; a mode that does not call it
/// runs *none* of that.
///
/// Exactly one shipped mode answers `false`: 23 `CARD`, whose handler
/// `FUN_80025F74` substitutes `FUN_80017978` for the master driver
/// ([`CARD_FRAME_BODY`]). `FUN_80017978` is 18 instructions with three `jal`s
/// and no `0x80016444` among them (`0x80017978..0x800179BC`), so while the
/// pause menu owns the frame retail advances no actor, no effect and no
/// animation - only the CARD actor's own `+0x0C` handler.
///
/// INIT modes have no per-frame handler and answer `true`: they are not
/// frames, and treating them as suspended would stop the world during a
/// single-frame init.
///
/// REF: FUN_80016444, FUN_80017978
pub fn runs_master_frame_driver(mode: GameMode) -> bool {
    match per_frame_stage(mode) {
        Some(stage) => matches!(stage.body, FrameBody::Master { .. }),
        None => true,
    }
}

/// Per-frame staging plan for a mode. `None` for the INIT (even-indexed)
/// modes, which use [`mode_init_stage`] instead.
///
/// This is where the four per-frame-handler addresses are anchored, because it
/// is the only function that turns a mode id into one of them. The shapes they
/// resolve to are [`PerFrameStage`] and [`CardFrameBody`]; both are plain data
/// `struct`s with no `impl`, so a tag on either widens the port's liveness
/// verdict to the whole file instead of naming this routine.
///
/// PORT: FUN_80025eec (the default handler - 12 of the 14 per-frame modes)
/// PORT: FUN_80025f2c (mode 13 MAPDISP)
/// PORT: FUN_80025f74 (mode 23 CARD)
/// PORT: FUN_80017978 (mode 23's body, [`CARD_FRAME_BODY`])
///
/// PARTIALLY WIRED: one field of the resolved stage is load-bearing on every
/// host. [`runs_master_frame_driver`] asks this function whether the current
/// mode's handler calls `FUN_80016444`, and
/// [`World::tick`](crate::world::World::tick) suspends its actor / effect /
/// move-VM passes when the answer is no. That is how the port encodes "the
/// pause menu freezes the world" - read off mode 23's `FUN_80017978`
/// substitution ([`CARD_FRAME_BODY`]) rather than written as a
/// `SceneMode::Menu` literal, so the rule cannot drift from its provenance.
/// `FUN_80025F74` and `FUN_80017978` therefore have a live caller on all three
/// hosts, and `CARD_FRAME_BODY` is no longer read only by this file's tests.
///
/// The rest of the shape is still unreached: the `overlay_hook` (mode 13's
/// `FUN_801CE850`), the master driver's `param`, and the abort branch are
/// consulted by nothing outside [`ModeDriver`], whose one remaining
/// prerequisite is a production owner. The engine's hosts drive frames from
/// `SceneHost` / `World` directly and never advance the retail mode word, so
/// there is no seat for a mode-table driver yet. `FUN_80025EEC` in particular
/// is not dead retail code - a five-form reference scan puts it in twelve slots
/// of the mode table at `0x8007078C`, every odd-indexed (per-frame) mode.
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
    fn the_boot_chain_is_ordered_and_every_step_names_its_store() {
        // Six stores, in the order a cold boot performs them. Each `mode` is
        // the literal the `sh` at `store_pc` writes.
        let modes: Vec<GameMode> = BOOT_MODE_CHAIN.iter().map(|s| s.mode).collect();
        assert_eq!(
            modes,
            vec![
                GameMode::ReadInit,
                GameMode::ReadMode,
                GameMode::CardInit,
                GameMode::CardMode,
                GameMode::MainInit,
                GameMode::MainMode,
            ]
        );
        // The title is not mode 0x10: that word belongs to the logo INIT, and
        // the title shares CARD MODE with the pause menu.
        assert_eq!(GameMode::ReadInit.as_index(), 0x10);
        assert_eq!(GameMode::CardMode.as_index(), 0x17);
        assert_eq!(GameMode::CardMode.scene_mode(), SceneMode::Menu);
        // Each INIT step in the chain is followed by the successor its own
        // handler stores.
        for pair in BOOT_MODE_CHAIN.windows(2) {
            if let Some(next) = init_successor(pair[0].mode) {
                assert_eq!(next, pair[1].mode, "{:?} hand-off", pair[0].mode);
                assert_eq!(
                    pair[1].store_pc,
                    INIT_HANDOFFS
                        .iter()
                        .find(|(m, _)| *m == pair[0].mode)
                        .unwrap()
                        .1
                        .store_pc,
                    "the chain and the hand-off table must cite the same store"
                );
            }
        }
    }

    #[test]
    fn mode_four_bounces_instead_of_advancing() {
        // The row that stops `init_successor` from being `index + 1`.
        assert_eq!(
            init_successor(GameMode::MonsterTest),
            Some(GameMode::ConfigInit)
        );
        assert_eq!(
            mode_init_bare(GameMode::MonsterTest),
            Some(ModeInitBare::SetsMode(GameMode::ConfigInit))
        );
        // Mode 18's hand-off lives in PROT 0902 and is not claimed here.
        assert_eq!(init_successor(GameMode::GameOverInit), None);
    }

    #[test]
    fn entering_an_init_mode_returns_its_plan_and_leaves_the_run_sibling() {
        let mut world = World::new();
        let mut seat = ModeSeat::new_at_boot();
        assert_eq!(seat.game_mode(), GameMode::ReadInit);

        let plan = seat.enter(GameMode::MainInit, &mut world);
        match plan {
            Some(ModeInitPlan::Stage(st)) => {
                assert_eq!(st.overlay_a_param, 2);
                assert_eq!(st.overlay_entry, 0x801D_6704);
            }
            other => panic!("MAIN INIT should stage the field overlay, got {other:?}"),
        }
        assert_eq!(seat.game_mode(), GameMode::MainMode);
        assert_eq!(seat.scene_mode(), SceneMode::Field);
    }

    #[test]
    fn an_entered_mode_swallows_the_pad_edge_that_caused_it() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::MainMode);
        // A frame with Start newly pressed - the edge that opens the menu.
        world.set_pad(0);
        world.set_pad(crate::input::PadButton::Start.mask());
        assert!(world.input.just_pressed(crate::input::PadButton::Start));

        seat.enter(GameMode::CardInit, &mut world);
        assert_eq!(seat.game_mode(), GameMode::CardMode);
        // Held is untouched; only the edge is gone.
        assert!(!world.input.just_pressed(crate::input::PadButton::Start));
        assert!(world.input.pressed(crate::input::PadButton::Start));
        assert_eq!(seat.edges(), 1);
    }

    /// The other direction, and the one a host's frame loop depends on: an
    /// edge the seat *adopts* leaves the pad alone, because the host has
    /// already published this frame's word by the time the frame runs.
    #[test]
    fn an_adopted_mode_change_leaves_this_frames_input_alone() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::MainMode);
        seat.adopt_scene_mode(SceneMode::Battle);
        world.set_pad(0);
        world.set_pad(crate::input::PadButton::Circle.mask());

        let f = seat.frame(&mut world);
        let edge = f.edge.expect("the word moved, so the edge is taken");
        assert_eq!(edge.to, GameMode::BattleMode);
        assert!(!edge.swallowed_pad_edges);
        assert!(
            world.input.just_pressed(crate::input::PadButton::Circle),
            "the frame's own input survives an adopted transition"
        );
    }

    #[test]
    fn card_mode_is_the_one_mode_that_runs_no_master_driver() {
        let mut world = World::new();
        let mut seat = ModeSeat::new(GameMode::CardMode);
        let f = seat.frame(&mut world);
        assert!(!f.runs_master_driver);
        assert_eq!(f.stage.unwrap().body, FrameBody::CardDriver);

        let mut seat = ModeSeat::new(GameMode::MainMode);
        assert!(seat.frame(&mut world).runs_master_driver);
    }

    #[test]
    fn adopting_a_scene_mode_round_trips_every_variant_the_word_can_name() {
        let mut seat = ModeSeat::new(GameMode::MainMode);
        for scene in [
            SceneMode::Field,
            SceneMode::WorldMap,
            SceneMode::Battle,
            SceneMode::Menu,
            SceneMode::Cutscene,
            SceneMode::Fishing,
            SceneMode::Dance,
            SceneMode::SlotMachine,
            SceneMode::BakaFighter,
            SceneMode::MuscleDome,
        ] {
            seat.adopt_scene_mode(scene);
            assert_eq!(
                seat.scene_mode(),
                scene,
                "the word plus the staged sub-id must name {scene:?} again"
            );
        }
        // Title is the port's "no scene loaded"; the retail word has no answer
        // for it, so the seat leaves the word where it was.
        let before = seat.game_mode();
        assert_eq!(seat.adopt_scene_mode(SceneMode::Title), None);
        assert_eq!(seat.game_mode(), before);
    }

    #[test]
    fn an_init_frame_is_never_adopted_away() {
        let mut seat = ModeSeat::new(GameMode::MainInit);
        assert_eq!(seat.adopt_scene_mode(SceneMode::Battle), None);
        assert_eq!(seat.game_mode(), GameMode::MainInit);
    }

    #[test]
    fn the_boot_handoff_branches_on_the_entry_word() {
        // Cold boot: `init.pak` raises the word itself, so the front end.
        let mut seat = ModeSeat::new(GameMode::ReadMode);
        assert_eq!(seat.entry_word(), 1);
        assert_eq!(seat.boot_handoff(), GameMode::CardInit);

        // The dev route: word clear, and the arm clears it again.
        let mut seat = ModeSeat::new(GameMode::ReadMode);
        seat.entry_word = 0;
        assert_eq!(seat.boot_handoff(), GameMode::ConfigInit);
        assert_eq!(seat.entry_word(), 0);

        // The request leaf writes both stores.
        let mut seat = ModeSeat::new(GameMode::MainMode);
        seat.entry_word = 0;
        seat.request_card_mode();
        assert_eq!(seat.game_mode(), GameMode::CardInit);
        assert_ne!(seat.entry_word(), 0);
    }

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

    #[test]
    fn bare_init_handlers_are_disjoint_from_the_staging_ones() {
        // Mode 4 writes the mode word and returns: it never reaches mode 5.
        assert_eq!(
            mode_init_bare(GameMode::MonsterTest),
            Some(ModeInitBare::SetsMode(GameMode::ConfigInit))
        );
        assert_eq!(
            mode_init_bare(GameMode::ReadInit),
            Some(ModeInitBare::Calls(0x801C_E9C0))
        );
        assert_eq!(READ_INIT_TARGET, 0x801C_E9C0);
        assert_eq!(
            mode_init_bare(GameMode::BattleInit),
            Some(ModeInitBare::Calls(0x8005_5B6C))
        );

        // The two shapes never overlap: a mode is staged or bare, not both.
        for m in [
            GameMode::ConfigInit,
            GameMode::MainInit,
            GameMode::GameOverInit,
            GameMode::MonsterTest,
            GameMode::ReadInit,
            GameMode::BattleInit,
            GameMode::MainMode,
        ] {
            assert!(
                mode_init_stage(m).is_none() || mode_init_bare(m).is_none(),
                "{m:?} claims both an overlay stage and a bare body"
            );
        }
        assert!(mode_init_bare(GameMode::MainMode).is_none());
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

    /// The OTHER pair is the one place the retail mode word under-determines
    /// the engine's `SceneMode`, and the sub-id is what closes the gap.
    #[test]
    fn the_other_pair_needs_the_warp_sub_id_to_resolve() {
        use crate::minigame_entry::MinigameSubId;
        // The live minigame mode is 0x19, not 0x18: 0x18 is the init half that
        // runs for one frame and hands the word on.
        assert_eq!(GameMode::OtherInit.as_index(), 0x18);
        assert_eq!(GameMode::OtherMode.as_index(), 0x19);
        // Mode word alone: no answer. This is what a host keying on
        // `scene_mode()` would have installed for a live minigame frame.
        assert_eq!(GameMode::OtherMode.scene_mode(), SceneMode::Title);
        // Mode word + sub-id: every playable slot resolves, on BOTH halves of
        // the pair (init modes hold their successor's scene mode).
        for slot in MinigameSubId::ALL {
            let sub = Some(i16::from(slot.sub_id()));
            let expect = slot.scene_mode().unwrap_or(SceneMode::Title);
            assert_eq!(
                GameMode::OtherMode.scene_mode_with_warp(sub),
                expect,
                "sub_id {} ({})",
                slot.sub_id(),
                slot.label()
            );
            assert_eq!(GameMode::OtherInit.scene_mode_with_warp(sub), expect);
        }
        // All five playable slots land on five DISTINCT scene modes - the
        // partition the retail mode word cannot express.
        let modes: Vec<_> = MinigameSubId::ALL
            .into_iter()
            .filter_map(|s| s.scene_mode())
            .collect();
        assert_eq!(modes.len(), 5);
        for (i, a) in modes.iter().enumerate() {
            assert!(
                !modes[i + 1..].contains(a),
                "two warp slots share {a:?} - the sub-id would not separate them"
            );
        }
        // Out of range and the two dev slots fall back to the mode word's own
        // answer rather than to a neighbouring minigame.
        assert_eq!(
            GameMode::OtherMode.scene_mode_with_warp(Some(7)),
            SceneMode::Title
        );
        assert_eq!(
            GameMode::OtherMode.scene_mode_with_warp(Some(-1)),
            SceneMode::Title
        );
        assert_eq!(
            GameMode::OtherMode.scene_mode_with_warp(Some(1)),
            SceneMode::Title
        );
    }

    /// Every mode outside the OTHER pair ignores the sub-id: the pair is one
    /// exception, not a second axis.
    #[test]
    fn the_sub_id_only_moves_the_other_pair() {
        for i in 0..28usize {
            let m = GameMode::from_index(i).unwrap();
            if matches!(m, GameMode::OtherInit | GameMode::OtherMode) {
                continue;
            }
            for sub in [None, Some(-1), Some(0), Some(3), Some(6), Some(9)] {
                assert_eq!(
                    m.scene_mode_with_warp(sub),
                    m.scene_mode(),
                    "mode {i} ({m:?}) moved on sub-id {sub:?}"
                );
            }
        }
    }

    /// The register is a signed halfword at `0x8007BA34`; the reader has to
    /// agree with the `lh` the mode-24 init issues.
    #[test]
    fn the_warp_sub_id_reader_matches_the_retail_register() {
        assert_eq!(WARP_SUB_ID_ADDR, 0x8007_BA34);
        let mut ram = vec![0u8; 0x20_0000];
        let off = (WARP_SUB_ID_ADDR - 0x8000_0000) as usize;
        ram[off..off + 2].copy_from_slice(&6i16.to_le_bytes());
        assert_eq!(read_warp_sub_id(&ram), Some(6));
        // `lh`, so the top bit sign-extends rather than reading as 0xFFFF.
        ram[off..off + 2].copy_from_slice(&(-1i16).to_le_bytes());
        assert_eq!(read_warp_sub_id(&ram), Some(-1));
        assert_eq!(read_warp_sub_id(&[]), None);
    }

    /// The driver installs the PAIR into the World, not the mode word alone.
    #[test]
    fn the_driver_carries_the_sub_id_into_the_world() {
        let mut d = ModeDriver::new(GameMode::OtherMode);
        let mut h = NoopHandler;
        let mut w = World::default();
        let input = InputState::new();
        // With no staged sub-id the driver has nothing to resolve with.
        d.tick(&mut h, &mut w, &input);
        assert_eq!(w.mode, SceneMode::Title);
        // Stage the fishing door (sub-id 0) the way the `0x3E` arm does, with
        // a session installed - a minigame mode with no session self-heals
        // back to its return mode inside the world tick, which would mask the
        // install this test is about.
        w.enter_fishing(crate::fishing::FishingSession::new(
            Vec::new(),
            4,
            crate::fishing::FishingRecord::default(),
        ));
        w.mode = SceneMode::Title;
        d.set_warp_sub_id(Some(0));
        assert_eq!(d.warp_sub_id(), Some(0));
        d.tick(&mut h, &mut w, &input);
        assert_eq!(w.mode, SceneMode::Fishing);
        // Retail leaves the register standing across the init -> run handoff
        // (`FUN_80025980` writes only the mode word on its way out), so the
        // mode word moving does not lose the discriminator.
        d.jump_to(GameMode::OtherInit);
        assert_eq!(d.scene_mode(), SceneMode::Fishing);
        d.jump_to(GameMode::OtherMode);
        assert_eq!(d.scene_mode(), SceneMode::Fishing);
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
