//! The scene selector's **entry beats**: per picked scene, the story
//! cutscene a player who arrives there sees, and how the selector stages it.
//!
//! A retail door's `0x3F` seats the party on its entry tile, and the field
//! loop's first post-arrival frame compares that tile against stale last-tile
//! globals (`FUN_801D1EC4` -> `FUN_801D5630`): a `.MAP` kind-1 gate-1 trigger
//! there spawns its partition-2 record through `FUN_8003BDE0`, whose C1 / C2
//! story-flag gates decide whether it runs. That spawn is the area's arrival
//! beat - Garmel's welcome, the Rogue's Tower entrance, a dungeon's opening
//! line - the first thing a player who walks in sees. A scene can open on its
//! story in two other ways. Its entry script (partition 1 record 0) tests a
//! flag the arriving door's record raised before its `0x3F` and spawns a
//! record with `0x44` (the Uru Mais dreams, the Bio Castle's last room), or
//! the beat's trigger lies a step or two past the arrival tile, on the rows
//! the party walks over first (the second Ra-Seru scene).
//!
//! The default picker seat ([`crate::scene_host::SceneHost::arm_picker_entry_seat`])
//! steps off every gate-1 tile and raises no door's flags, so no beat fires;
//! the play hosts' scene selectors ask
//! [`crate::scene_host::SceneHost::arm_picker_story_entry`] instead, which
//! stages a scene listed in [`PICKER_ENTRY_BEATS`] by its row
//! ([`crate::scene_host::SceneHost::arm_picker_beat`]): the seat its
//! [`BeatTrigger`] names, and the row's flags raised
//! ([`legaia_engine_core::world::World::seed_picker_beat_flags`]). Every other
//! scene keeps the default rule.
//!
//! The rows are generated, not curated: the disc-gated
//! `picker_entry_beats_disc` test walks every selector scene's inbound
//! arrivals in [`ArrivalCatalog::ranked`] order and, per arrival, tries the
//! three triggers in [`BeatTrigger`] order, each as a fresh picker visit after
//! the free-roam staging
//! ([`legaia_engine_core::world::World::stage_picker_entry`]) with the
//! arrival's hand-off flags and the record's missing C2 flags raised. It keeps
//! the first candidate whose record engages the player on the first ticks,
//! plays for a while and hands control back in the scene. Its output is the
//! block below, and the same test fails when the disc's answer and the block
//! disagree (`LEGAIA_PICKER_BEATS_WRITE=1` rewrites it). Candidates the
//! selector cannot stage (a C1 flag the staging holds set, a beat that leaves
//! the scene or starts a fight) are listed in [`PICKER_ENTRY_BEATS_HELD`].
//!
//! The one curated input is [`PICKER_BEATS_THAT_LEAVE`]: the scenes whose
//! beat may end in a scene change, because retail never hands the player
//! control there.
//!
//! [`ArrivalCatalog::ranked`]: legaia_engine_scene::scene_arrivals::ArrivalCatalog::ranked
// REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0

/// How a [`PickerEntryBeat`]'s record is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatTrigger {
    /// The arrival's entry tile carries the gate-1 trigger: the party is
    /// seated on it, and the first tick's stale-tile compare spawns the
    /// record.
    ArrivalTile,
    /// The scene's entry script spawns the record (`0x44`) behind a flag the
    /// arriving door's record raises before its `0x3F`: the row's flags carry
    /// it, and the party takes the default picker seat.
    EntryScript,
    /// A gate-1 trigger `step` tiles past the arrival tile, on the party's
    /// first steps into the scene: the party is seated on it.
    NearArrival,
}

/// A scene the selector opens on its story beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerEntryBeat {
    /// The picked scene (CDNAME label).
    pub scene: &'static str,
    /// The scene whose `0x3F` lands the party there.
    pub from: &'static str,
    /// The `0x3F`'s entry tile (low 7 bits of its entry bytes).
    pub tile: (u8, u8),
    /// Whole tiles from `tile` to the seat ([`BeatTrigger::NearArrival`];
    /// zero otherwise).
    pub step: (i8, i8),
    /// The arrival's facing sector (`dir & 7`).
    pub dir: u8,
    /// The partition-2 record the beat runs.
    pub record: u8,
    /// How the record is started.
    pub trigger: BeatTrigger,
    /// The story flags the selector raises for it: the arriving door's
    /// hand-off flags (the `0x5x` sets its record makes before the `0x3F`),
    /// then the record's C2 flags the staging does not hold.
    pub flags: &'static [u16],
}

/// Why an arrival beat is not staged by the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldReason {
    /// The record's C1 list names a flag the selector's staging holds set
    /// (flag `7`, the MAN loader's, marks developer leftovers).
    BlockedByFlags(&'static [u16]),
    /// The gates pass but the record does not install, ends at once, or is
    /// no story beat (a silent record shorter than a door or lift animation
    /// runs).
    DoesNotPlay,
    /// The beat ends in a scene change out of the picked scene, and the scene
    /// is not one of [`PICKER_BEATS_THAT_LEAVE`].
    LeavesScene(&'static str),
    /// The beat starts a battle.
    StartsBattle,
    /// The beat does not hand control back in a headless visit (`town01`'s
    /// gate record is the New Game opening, which runs on into name entry).
    DoesNotEnd,
}

/// An arrival beat the selector does not stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerEntryBeatHeld {
    /// The picked scene.
    pub scene: &'static str,
    /// The scene whose `0x3F` lands on the trigger.
    pub from: &'static str,
    /// The entry tile.
    pub tile: (u8, u8),
    /// How the record would be started.
    pub trigger: BeatTrigger,
    /// The partition-2 record (`0xFF` for an entry-script candidate that
    /// started nothing).
    pub record: u8,
    /// Why it is held.
    pub reason: HeldReason,
}

/// Scenes whose beat may end by leaving: cutscene scenes retail never hands
/// the player control in.
///
/// - `urudre2` (Noa's dream, Conkram) is entered only through `uru`
///   `P2[33]`, which raises `0x3F7` before its `0x3F`. The entry script tests
///   `0x3F7`, clears it and spawns `P2[9]` (`44 37`), and `P2[9]` ends in an
///   unconditional `0x3F` to `map01`.
/// - `dream` (Noa's Dreams) is entered only through an inn's `0x3F` onto the
///   map-centre tile, the gate-1 trigger of `P2[0]`. `P2[0]` spawns one of
///   four dream records by story flag (`0x35B..0x35E`), each spawns `P2[5]`,
///   and every arm of `P2[5]` ends in a `0x3F` back to the inn its `0x58x`
///   flag names.
pub const PICKER_BEATS_THAT_LEAVE: &[&str] = &["urudre2", "dream"];

/// The [`PICKER_ENTRY_BEATS`] row for `scene`.
pub fn picker_entry_beat(scene: &str) -> Option<&'static PickerEntryBeat> {
    PICKER_ENTRY_BEATS.iter().find(|b| b.scene == scene)
}

// BEGIN GENERATED by picker_entry_beats_disc (LEGAIA_PICKER_BEATS_WRITE=1)
/// Selector scenes opened on their story beat.
pub const PICKER_ENTRY_BEATS: &[PickerEntryBeat] = &[
    PickerEntryBeat {
        scene: "bylon",
        from: "garmel",
        tile: (0x7F, 0x7F),
        step: (0, 0),
        dir: 0,
        record: 17,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x194, 0x19A, 0x2AD, 0x28D, 0x00A],
    },
    PickerEntryBeat {
        scene: "retock",
        from: "map02",
        tile: (0x14, 0x4D),
        step: (-1, 0),
        dir: 6,
        record: 27,
        trigger: BeatTrigger::NearArrival,
        flags: &[0x02C, 0x002],
    },
    PickerEntryBeat {
        scene: "geremi",
        from: "map02",
        tile: (0x1B, 0x08),
        step: (0, 0),
        dir: 4,
        record: 0,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x028, 0x002],
    },
    PickerEntryBeat {
        scene: "balden",
        from: "map02",
        tile: (0x41, 0x19),
        step: (0, 0),
        dir: 2,
        record: 0,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x029, 0x002, 0x1D5],
    },
    PickerEntryBeat {
        scene: "rayman",
        from: "tunnela",
        tile: (0x40, 0x1D),
        step: (0, 0),
        dir: 0,
        record: 21,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[],
    },
    PickerEntryBeat {
        scene: "ropeway",
        from: "map02",
        tile: (0x1C, 0x0B),
        step: (0, 0),
        dir: 4,
        record: 6,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x02A, 0x002],
    },
    PickerEntryBeat {
        scene: "retockin",
        from: "dohaty",
        tile: (0x69, 0x32),
        step: (0, 0),
        dir: 0,
        record: 42,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x1D4, 0x65B],
    },
    PickerEntryBeat {
        scene: "rayman2",
        from: "tunnelb",
        tile: (0x0C, 0x2B),
        step: (0, 0),
        dir: 0,
        record: 22,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x1D5],
    },
    PickerEntryBeat {
        scene: "son",
        from: "map03",
        tile: (0x08, 0x09),
        step: (0, 0),
        dir: 0,
        record: 5,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x60B],
    },
    PickerEntryBeat {
        scene: "bubu1",
        from: "map03",
        tile: (0x46, 0x0F),
        step: (0, 0),
        dir: 0,
        record: 1,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x574, 0x3D5, 0x51F],
    },
    PickerEntryBeat {
        scene: "bubu2",
        from: "map03",
        tile: (0x45, 0x13),
        step: (0, 0),
        dir: 4,
        record: 3,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x02F, 0x002, 0x608, 0x3D3],
    },
    PickerEntryBeat {
        scene: "garmel",
        from: "map01",
        tile: (0x14, 0x09),
        step: (0, 0),
        dir: 4,
        record: 10,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x002],
    },
    PickerEntryBeat {
        scene: "keikoku",
        from: "map01",
        tile: (0x29, 0x51),
        step: (0, 0),
        dir: 4,
        record: 4,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x002],
    },
    PickerEntryBeat {
        scene: "dream",
        from: "bylon",
        tile: (0x40, 0x40),
        step: (0, 0),
        dir: 0,
        record: 0,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x582],
    },
    PickerEntryBeat {
        scene: "rikuroa",
        from: "noaru",
        tile: (0x46, 0x48),
        step: (0, 0),
        dir: 0,
        record: 58,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x449, 0x637, 0x44B, 0x548, 0x00B, 0x636, 0x00A, 0x01B],
    },
    PickerEntryBeat {
        scene: "retona",
        from: "map02",
        tile: (0x43, 0x50),
        step: (-1, 1),
        dir: 0,
        record: 12,
        trigger: BeatTrigger::NearArrival,
        flags: &[0x528, 0x531, 0x52F, 0x51C],
    },
    PickerEntryBeat {
        scene: "concnow",
        from: "map03",
        tile: (0x1E, 0x16),
        step: (0, 0),
        dir: 4,
        record: 13,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x032, 0x002],
    },
    PickerEntryBeat {
        scene: "taiku",
        from: "son",
        tile: (0x0A, 0x0A),
        step: (0, 0),
        dir: 0,
        record: 18,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x60C, 0x60B, 0x3A9],
    },
    PickerEntryBeat {
        scene: "urudre1",
        from: "uru",
        tile: (0x40, 0x40),
        step: (0, 0),
        dir: 0,
        record: 0,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x3F5],
    },
    PickerEntryBeat {
        scene: "urudre2",
        from: "uru",
        tile: (0x40, 0x40),
        step: (0, 0),
        dir: 0,
        record: 9,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x3F7],
    },
    PickerEntryBeat {
        scene: "kor",
        from: "korout",
        tile: (0x17, 0x06),
        step: (0, 0),
        dir: 0,
        record: 8,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x002],
    },
    PickerEntryBeat {
        scene: "kor3",
        from: "koin1b",
        tile: (0x1B, 0x0B),
        step: (0, 0),
        dir: 4,
        record: 15,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x42A],
    },
    PickerEntryBeat {
        scene: "korb3",
        from: "korb2",
        tile: (0x25, 0x27),
        step: (0, 0),
        dir: 0,
        record: 1,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[],
    },
    PickerEntryBeat {
        scene: "koin1",
        from: "kor3",
        tile: (0x17, 0x28),
        step: (0, 0),
        dir: 4,
        record: 8,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[],
    },
    PickerEntryBeat {
        scene: "koin4",
        from: "kor4",
        tile: (0x64, 0x5E),
        step: (1, 1),
        dir: 4,
        record: 3,
        trigger: BeatTrigger::NearArrival,
        flags: &[],
    },
    PickerEntryBeat {
        scene: "juui2",
        from: "noaru",
        tile: (0x1B, 0x71),
        step: (0, -2),
        dir: 0,
        record: 16,
        trigger: BeatTrigger::NearArrival,
        flags: &[],
    },
    PickerEntryBeat {
        scene: "deroa",
        from: "map03",
        tile: (0x15, 0x17),
        step: (0, 0),
        dir: 4,
        record: 4,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x002, 0x3E1],
    },
    PickerEntryBeat {
        scene: "conc2",
        from: "concnow",
        tile: (0x2A, 0x6F),
        step: (0, 0),
        dir: 0,
        record: 9,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x00A, 0x00B, 0x423],
    },
    PickerEntryBeat {
        scene: "jouine",
        from: "jouind",
        tile: (0x40, 0x40),
        step: (0, 0),
        dir: 0,
        record: 5,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x6CD, 0x5B8, 0x512],
    },
    PickerEntryBeat {
        scene: "rugi",
        from: "juui1",
        tile: (0x22, 0x58),
        step: (0, 0),
        dir: 4,
        record: 72,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x00B, 0x00A, 0x01B],
    },
    PickerEntryBeat {
        scene: "noaru",
        from: "juui1",
        tile: (0x24, 0x0D),
        step: (0, 0),
        dir: 4,
        record: 34,
        trigger: BeatTrigger::ArrivalTile,
        flags: &[0x00B, 0x01B],
    },
    PickerEntryBeat {
        scene: "conc3",
        from: "conc",
        tile: (0x0F, 0x60),
        step: (0, 0),
        dir: 0,
        record: 9,
        trigger: BeatTrigger::EntryScript,
        flags: &[0x53C],
    },
];

/// Beats the selector holds back.
pub const PICKER_ENTRY_BEATS_HELD: &[PickerEntryBeatHeld] = &[
    PickerEntryBeatHeld {
        scene: "town01",
        from: "map01",
        tile: (0x1D, 0x5B),
        trigger: BeatTrigger::ArrivalTile,
        record: 3,
        reason: HeldReason::DoesNotEnd,
    },
    PickerEntryBeatHeld {
        scene: "izumi",
        from: "map01",
        tile: (0x14, 0x0D),
        trigger: BeatTrigger::NearArrival,
        record: 2,
        reason: HeldReason::BlockedByFlags(&[0x007]),
    },
    PickerEntryBeatHeld {
        scene: "bylon",
        from: "map01",
        tile: (0x1C, 0x46),
        trigger: BeatTrigger::ArrivalTile,
        record: 19,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "bylon",
        from: "map01",
        tile: (0x0E, 0x3F),
        trigger: BeatTrigger::ArrivalTile,
        record: 19,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "bylon",
        from: "map01",
        tile: (0x3D, 0x46),
        trigger: BeatTrigger::ArrivalTile,
        record: 19,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "retock",
        from: "jagaroom",
        tile: (0x2E, 0x44),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("retockin"),
    },
    PickerEntryBeatHeld {
        scene: "retockin",
        from: "jagaroom",
        tile: (0x1B, 0x38),
        trigger: BeatTrigger::ArrivalTile,
        record: 47,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "balden2",
        from: "balden",
        tile: (0x67, 0x74),
        trigger: BeatTrigger::ArrivalTile,
        record: 11,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "rayman2",
        from: "tunnela",
        tile: (0x40, 0x1D),
        trigger: BeatTrigger::ArrivalTile,
        record: 21,
        reason: HeldReason::BlockedByFlags(&[0x007]),
    },
    PickerEntryBeatHeld {
        scene: "ropeway2",
        from: "rayman",
        tile: (0x0D, 0x60),
        trigger: BeatTrigger::ArrivalTile,
        record: 22,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "dolk",
        from: "map01",
        tile: (0x49, 0x72),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("rikuroa"),
    },
    PickerEntryBeatHeld {
        scene: "rikuroa",
        from: "rikuroa2",
        tile: (0x41, 0x24),
        trigger: BeatTrigger::ArrivalTile,
        record: 27,
        reason: HeldReason::BlockedByFlags(&[0x007]),
    },
    PickerEntryBeatHeld {
        scene: "rikuroa",
        from: "noaru",
        tile: (0x46, 0x48),
        trigger: BeatTrigger::ArrivalTile,
        record: 56,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "rikuroa",
        from: "map01",
        tile: (0x66, 0x0D),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("map01"),
    },
    PickerEntryBeatHeld {
        scene: "rikuroa",
        from: "map01",
        tile: (0x11, 0x10),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("map01"),
    },
    PickerEntryBeatHeld {
        scene: "stone",
        from: "map02",
        tile: (0x39, 0x24),
        trigger: BeatTrigger::ArrivalTile,
        record: 0,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "jagaroom",
        from: "retockin",
        tile: (0x27, 0x14),
        trigger: BeatTrigger::ArrivalTile,
        record: 6,
        reason: HeldReason::DoesNotPlay,
    },
    PickerEntryBeatHeld {
        scene: "taiku2",
        from: "korout",
        tile: (0x2F, 0x15),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("map03"),
    },
    PickerEntryBeatHeld {
        scene: "urudre3",
        from: "uru",
        tile: (0x40, 0x40),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::DoesNotEnd,
    },
    PickerEntryBeatHeld {
        scene: "korout",
        from: "kor",
        tile: (0x3C, 0x3C),
        trigger: BeatTrigger::ArrivalTile,
        record: 1,
        reason: HeldReason::LeavesScene("kor"),
    },
    PickerEntryBeatHeld {
        scene: "korout",
        from: "map03",
        tile: (0x42, 0x1C),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("kor"),
    },
    PickerEntryBeatHeld {
        scene: "juui1",
        from: "uru2",
        tile: (0x00, 0x00),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("noaru"),
    },
    PickerEntryBeatHeld {
        scene: "nilboa2",
        from: "map03",
        tile: (0x7F, 0x7F),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("map03"),
    },
    PickerEntryBeatHeld {
        scene: "concend",
        from: "map03",
        tile: (0x1E, 0x16),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("town0d"),
    },
    PickerEntryBeatHeld {
        scene: "koin1b",
        from: "kor3",
        tile: (0x1B, 0x0B),
        trigger: BeatTrigger::EntryScript,
        record: 255,
        reason: HeldReason::LeavesScene("kor3"),
    },
];
// END GENERATED
