//! The mode ids and the retail mode table: `SceneMode`, the `ModeWorld` seam, `GameMode` and `TABLE`.

/// Scene mode the world is running. Drives which top-level VMs tick and
/// which auxiliary state lives in the world.
///
/// **This is a finer partition than the retail mode word.** Five variants
/// below - `Dance`, `Fishing`, `SlotMachine`, `BakaFighter`, `MuscleDome` -
/// all correspond to the *same* retail per-frame mode, `0x19` `OTHER MODE`.
/// Retail tells them apart with a second register, the warp sub-id
/// `_DAT_8007BA34`, which the field-VM `0x3E` door-warp arm writes and the
/// mode-24 `OTHER` init reads to pick the overlay. So a retail mode word alone
/// does **not** determine a `SceneMode`, and anything mapping between the two
/// spaces has to carry the pair: see
/// [`GameMode::scene_mode_with_warp`](crate::mode::GameMode::scene_mode_with_warp)
/// and [`crate::minigame_entry::MinigameSubId`].
///
/// Note the off-by-one that reading `0x18` as the live mode invites: `0x18`
/// (24) is `OTHER`, the *init* half, which runs for a single frame and hands
/// the word to `0x19` (25). Keying a minigame on `0x18` therefore never sees a
/// live minigame frame at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneMode {
    /// Title / no scene. Only the actor VM and effect pool are live.
    #[default]
    Title,
    /// Field / town scene - field VM drives event flow.
    Field,
    /// Battle scene - battle action state machine runs over the actor table.
    Battle,
    /// Cutscene mode - actor VM runs but no field/battle dispatch.
    Cutscene,
    /// World-map mode - `WorldMapController` drives camera and entity ticks.
    WorldMap,
    /// Noa dance (rhythm) minigame - the beat clock + hit judge own the frame
    /// (`legaia_engine_minigames::dance::DanceGame`); field / battle dispatch is suspended. The
    /// hosting session preserves the suspended scene state and restores its
    /// mode on exit, like the pause menu. Retail runs it under `game_mode 0x19`
    /// (`OTHER MODE`) with warp sub-id `6` (the dance overlay, PROT 0980).
    Dance,
    /// Fishing minigame - the cast / fight / score loop owns the frame
    /// (`legaia_engine_fishing::fishing::PondSession`); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail runs it
    /// under `game_mode 0x19` (`OTHER MODE`) with warp sub-id `0` (the fishing
    /// overlay, PROT 0972).
    Fishing,
    /// Casino slot-machine minigame - the reel state machine owns the frame
    /// (`legaia_engine_minigames::slot_machine::SlotMachine`); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail runs it
    /// under `game_mode 0x19` (`OTHER MODE`) with warp sub-id `3` (the slot
    /// overlay, PROT 0975).
    SlotMachine,
    /// Baka Fighter duel minigame - the exchange / round / match state
    /// machine owns the frame (`legaia_engine_minigames::baka_fighter::BakaFight`); field /
    /// battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail runs it under `game_mode 0x19` (`OTHER MODE`) with warp
    /// sub-id `4` (the Baka Fighter overlay, PROT 0976).
    BakaFighter,
    /// Muscle Dome contest, an ordinary battle fought to a KO - the direction-entry /
    /// commit / resolve loop owns the frame (`legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession`); field
    /// / battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail: the arena runs *inside* the battle overlay (PROT 0898)
    /// on the `_DAT_8007bd24` context, entered through the `game_mode 0x18`
    /// (`OTHER`) door at warp sub-id `5` (PROT 0977) and running under
    /// `game_mode 0x19` (`OTHER MODE`) thereafter.
    MuscleDome,
    /// In-field pause menu - the retail CARD mode pair (`game_mode 0x17`,
    /// `CARD MODE`, which hosts both the memory-card UI and the pause menu;
    /// every menu-open capture holds `_DAT_8007B83C = 0x17`). Field/battle
    /// dispatch is suspended while the menu owns the frame; only the actor
    /// VM and effect pool run, like `Title`. The hosting session preserves
    /// the suspended scene state and restores its mode on close.
    Menu,
}

impl SceneMode {
    /// The variant's name (`"Field"`, `"SlotMachine"`, ...): the stable
    /// spelling the mode-trace oracle and the replay state traces record.
    pub fn name(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Field => "Field",
            Self::Battle => "Battle",
            Self::Cutscene => "Cutscene",
            Self::WorldMap => "WorldMap",
            Self::Dance => "Dance",
            Self::Fishing => "Fishing",
            Self::SlotMachine => "SlotMachine",
            Self::BakaFighter => "BakaFighter",
            Self::MuscleDome => "MuscleDome",
            Self::Menu => "Menu",
        }
    }
}

/// The slice of the game world the mode seat reads and writes each frame:
/// the scene mode it installs, the frame-begin skip latch it consults and
/// clears, the CONFIG INIT sound detach, the battle-intro hold on the mode
/// word, the pad edges a synchronous transition swallows, and the master
/// frame tick itself. engine-core's `World` implements it; nothing else in
/// the seat touches the world.
pub trait ModeWorld {
    /// The world's current [`SceneMode`].
    fn scene_mode(&self) -> SceneMode;
    /// Install the [`SceneMode`] the mode word resolves to.
    fn set_scene_mode(&mut self, mode: SceneMode);
    /// Take the frame-begin pass's skip result (`FUN_8001698C`): `true` when
    /// this frame gets a pad poll and a `VSync(0)` and nothing else.
    fn take_frame_begin_skip(&mut self) -> bool;
    /// Clear the frame-begin skip latch (`gp+0x3D8`) on a mode change.
    fn clear_frame_begin_skip(&mut self);
    /// Mode 0 CONFIG INIT's sound detach (`FUN_8002689C`).
    fn detach_sound(&mut self);
    /// `true` while the battle intro holds the mode word (retail's store of
    /// `0x14` waits for the transition clock).
    fn battle_mode_word_held(&self) -> bool;
    /// Drop this frame's pad edges (`gp+0x538` / `gp+0x55C`).
    fn clear_pad_edges(&mut self);
    /// Run one master frame of the world.
    fn tick_frame(&mut self);
}

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
            // The disc-recovered handler map (legaia_game_tables::mode_table)
            // confirms it structurally: mode 2's init handler FUN_80025B64
            // loads the field overlay + per-scene initializer and hands off
            // to mode 3. MainInit holds Field like the other init modes
            // below hold their successors'.
            GameMode::MainInit | GameMode::MainMode => SceneMode::Field,
            // MAPDISP (12/13) is the world-map DISPLAY mode, not the field -
            // pinned by the disc mode table (legaia_game_tables::mode_table): its
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
/// `legaia_parity::capture_observations::cutscene_trigger_corpus::read_game_mode`:
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
