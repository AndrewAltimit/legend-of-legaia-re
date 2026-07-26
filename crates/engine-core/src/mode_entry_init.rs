//! One-time **mode-entry initialisers**: the straight-line routines a game
//! mode runs once when its overlay lands, before its per-frame loop takes
//! over.
//!
//! PORT: FUN_801D6704, FUN_801CF00C
//!
//! Two of them are ported here because they are the same shape - a fixed,
//! ordered sequence of global seeds, asset loads and actor spawns that ends by
//! handing control to a per-frame driver:
//!
//! - [`field_spawn`] / [`field_bgm_plan`] / [`field_prim_buffer_bytes`] /
//!   [`FIELD_INIT_STEPS`] - `FUN_801D6704`, the **field / town scene
//!   initialiser** ("MAIN_INIT", boot mode 2). Every field scene transition
//!   goes through it: it re-copies the scene name, loads the map + MAN +
//!   camera + fog, allocates the game-mode work buffer, resolves the BGM
//!   slot, spawns or sweeps the actor lists, seats the player, and finally
//!   writes the master game mode `3` so the field per-frame loop takes the
//!   next frame.
//! - [`duel_overlay_init`] - `FUN_801CF00C`, the **Baka Fighter duel arena**
//!   overlay initialiser: display env, OT depth, BGM, the two fighter
//!   installs, the streamed SFX/voice loads, and the round-win target, before
//!   the round SM takes over.
//!
//! ## Dump provenance, and one correction
//!
//! `see ghidra/scripts/funcs/overlay_dialog_mc4_801d6704.txt` (901
//! instructions, epilogue at `0x801d7510`) and
//! `ghidra/scripts/funcs/overlay_baka_fighter_801cf00c.txt`.
//!
//! The static-base dump `overlay_0897_801d6704.txt` is **incomplete**: its
//! instruction stream jumps `0x801d71b4 -> 0x801d72d4` (dropping the whole
//! two-part-BGM arm) and it carries no `jr ra`. The live-RAM captures at base
//! `0x801C0000` agree with each other on all 901 instructions and do have the
//! epilogue, so they are the dump of record for this function. `docs/tooling/
//! dump-corpus-integrity.md` is the general statement of that failure mode - a
//! base-correct dump can still have gaps.
//!
//! REF: FUN_80025B64 (the mode dispatcher that calls the field initialiser),
//! FUN_8003AEB0 (MAN decode + camera anchor), FUN_80017DD4 (camera-window
//! install), FUN_80024C88 (actor spawn from a position vector),
//! FUN_801D7518 (per-list actor sweep), FUN_801D3468 (the duel round SM)

/// Camera view-window extent in tiles, in the order the initialiser writes it
/// to the scratchpad pair `0x1F8003F8` / `0x1F8003FA` (`0x0E` wide by `0x10`
/// deep). Both the window install `FUN_80017DD4` and the sub-area rebuild
/// sweep read it back from there.
pub const FIELD_CAMERA_WINDOW_TILES: (i32, i32) = (0x0E, 0x10);

/// World units per collision tile.
const TILE_UNITS: i32 = 0x80;

/// Half a tile - the tile-centre bias every world position in the initialiser
/// carries.
const TILE_CENTRE: i32 = 0x40;

/// The cold-entry player seat: the centre of the camera view window.
///
/// `FUN_801D6704` spawns the player actor at `(0xA40, 0, 0xA40)` on a cold
/// entry, and [`crate::world::FIELD_COLD_SPAWN_XZ`] is the engine's mirror of
/// the X/Z component.
pub const FIELD_COLD_SPAWN: (i16, i16, i16) = (0xA40, 0, 0xA40);

/// How the field initialiser was entered - the field-entry mode global
/// `_DAT_8007B8B8`.
///
/// The initialiser tests this global three times and the arms are what make
/// cold and warp entry structurally different, not just differently
/// positioned:
///
/// - `Cold` (`0`): the player actor is **allocated** (from the resident
///   template) and an extra actor is spawned at [`FIELD_COLD_SPAWN`].
/// - `Warp` (`2`): the seven per-list actor sweeps run instead (`FUN_801D7518`
///   once per list), the player actor is reused, and the saved transition
///   coords `_DAT_80084568` / `_DAT_8008456C` overwrite the MAN camera anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEntryMode {
    /// `_DAT_8007B8B8 == 0` - a fresh field entry (the New Game opening).
    Cold,
    /// `_DAT_8007B8B8 == 2` - a scene-to-scene warp.
    Warp,
    /// Any other value. The initialiser's arms treat it as "neither": no
    /// extra-actor spawn and no saved-coord override.
    Other(u32),
}

impl FieldEntryMode {
    /// Classify the raw `_DAT_8007B8B8` word.
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Cold,
            2 => Self::Warp,
            other => Self::Other(other),
        }
    }
}

/// Where the field initialiser seats the player and the camera window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpawn {
    /// Tile the camera view window is installed on (`FUN_80017DD4`'s first
    /// two arguments).
    pub window_tile: (i32, i32),
    /// World `(x, z)` written to the player actor's `+0x14` / `+0x18`.
    pub player: (i16, i16),
    /// The extra actor the **cold** arm spawns from template `0x801F271C`,
    /// as an `(x, y, z)` vector. `None` on every non-cold entry.
    pub extra_actor: Option<(i16, i16, i16)>,
}

/// Round a world coordinate down to its tile index the way the initialiser
/// does: `v >= 0 ? v >> 7 : (v + 0x7F) >> 7` (an arithmetic shift biased so
/// negatives truncate toward zero).
fn tile_of(v: i32) -> i32 {
    if v >= 0 {
        v >> 7
    } else {
        (v + (TILE_UNITS - 1)) >> 7
    }
}

/// Resolve the player seat + camera window for a field entry.
///
/// PORT: FUN_801D6704 (`0x801d6e08..0x801d6fe0`).
///
/// `anchor` is the camera anchor pair the MAN decoder `FUN_8003AEB0` filled
/// (stack `sp+0x20` / `sp+0x22`); on a [`FieldEntryMode::Warp`] entry the
/// initialiser overwrites it with the low half of the saved transition coords
/// `_DAT_80084568` / `_DAT_8008456C` before this point, which is what `saved`
/// supplies. `recentre` is the one-shot request flag `_DAT_8007BACC` (cleared
/// by the initialiser after it is honoured) and `recentre_origin` is the map
/// tile origin pair `_DAT_8007B76C` / `_DAT_8007B770` its arm reads.
///
/// Two window forms:
///
/// - `recentre == false`: the window sits on the anchor's own tile and the
///   player keeps the anchor's exact world coordinates.
/// - `recentre == true`: the window is centred on the map origin
///   (`origin_x + width/2 - 1`, `origin_z + depth/2`) and the player is seated
///   at that tile's centre (`tile * 0x80 + 0x40`).
///
/// ## The sub-tile terms are dead
///
/// The initialiser computes the sub-tile remainders of the saved transition
/// coords (`saved % 0x80 - 0x40` per axis) at `0x801d6e1c..0x801d6e78`,
/// **gated on `_DAT_8007B8B8 == 2`**, and adds them to the extra actor's
/// spawn vector at `0x801d6fc8`/`0x801d6fd0`, **gated on
/// `_DAT_8007B8B8 == 0`**. Those two gates are mutually exclusive, so the
/// remainders can never reach an actor write: on a cold entry the registers
/// holding them are still their `0` initialisers, and on a warp entry the
/// only consumer is skipped. The cold seat is therefore exactly
/// [`FIELD_COLD_SPAWN`], and a warp's landing position comes from the
/// player-actor stores at `0x801d6f5c..0x801d6f7c` fed by the `recentre`
/// branch above - not from a sub-tile offset.
///
/// WIRED, and the chain is: the `legaia-engine` binary
/// (`commands/run.rs`, and the `play-window` redraw path) and
/// `engine_shell::BootSession::…` (`boot.rs`) both call
/// [`crate::scene::SceneHost::enter_field_scene`], which calls this to seat the
/// player on a cold field entry. Every other kernel in this module is inert and
/// says so individually - being in a live module is not itself evidence.
pub fn field_spawn(
    mode: FieldEntryMode,
    anchor: (i16, i16),
    saved: (i32, i32),
    recentre: bool,
    recentre_origin: (i32, i32),
) -> FieldSpawn {
    // A warp entry overwrites the MAN anchor with the saved transition coords
    // (stored as `u16`, read back signed).
    let seat = if mode == FieldEntryMode::Warp {
        (saved.0 as i16, saved.1 as i16)
    } else {
        anchor
    };
    let (window_tile, player) = if recentre {
        let tx = recentre_origin.0 + FIELD_CAMERA_WINDOW_TILES.0 / 2 - 1;
        let tz = recentre_origin.1 + FIELD_CAMERA_WINDOW_TILES.1 / 2;
        let wx = tx * TILE_UNITS + TILE_CENTRE;
        let wz = tz * TILE_UNITS + TILE_CENTRE;
        ((tx, tz), (wx as i16, wz as i16))
    } else {
        (
            (tile_of(i32::from(seat.0)), tile_of(i32::from(seat.1))),
            seat,
        )
    };
    FieldSpawn {
        window_tile,
        player,
        extra_actor: (mode == FieldEntryMode::Cold).then_some(FIELD_COLD_SPAWN),
    }
}

/// BGM ids below this are **relative** to the scene's sequence base; ids at or
/// above it name a slot directly.
pub const FIELD_BGM_DIRECT_ID_MIN: u32 = 0x7D0;

/// The constant the relative form adds after the sequence base.
pub const FIELD_BGM_BASE_BIAS: u32 = 6;

/// The one BGM id whose load is **two streams**, not one.
pub const FIELD_BGM_TWO_PART_ID: u32 = 0x814;

/// The two streaming-asset ids the [`FIELD_BGM_TWO_PART_ID`] arm loads, in
/// load order.
pub const FIELD_BGM_TWO_PART_STREAMS: [u32; 2] = [0x428, 0x422];

/// What the initialiser decides about BGM for this scene entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldBgmPlan {
    /// The resolved slot written to `_DAT_8007BAB8`.
    pub slot: u32,
    /// `true` when the resolved slot equals the slot already playing
    /// (`_DAT_8007BA9C`) - the latch `_DAT_8007BAD0` the wait barrier and the
    /// per-frame director read to avoid restarting the same track.
    pub already_playing: bool,
    /// `Some` when this entry must load the two-part track's streams.
    pub two_part_streams: Option<[u32; 2]>,
}

/// Resolve the BGM slot + the two-part load for a field entry.
///
/// PORT: FUN_801D6704 (`0x801d7050..0x801d70dc` and the two-part arm at
/// `0x801d7190..0x801d72d0`).
///
/// `bgm_id` is `_DAT_8007BAC8`, `seq_base` is `_DAT_80084540` (seeded earlier
/// in the same initialiser from `_DAT_8007B768` when that is non-negative),
/// `playing_slot` is `_DAT_8007BA9C`, and `two_part_latched` is the one-shot
/// `_DAT_8007B9B8` that stops the two-part streams being re-read on a second
/// entry to the same scene.
///
/// The relative form (`bgm_id < 0x7D0`) is `bgm_id + seq_base + 6`; the direct
/// form passes the id through. This is the same id space
/// [`crate::music_labels`] resolves - global id `2000 + i` is `music_01`
/// track `i`, and `0x7D0` is exactly `2000`, so "below 2000" *is* "not a
/// global track id".
///
/// NOT WIRED: the engine's BGM director selects a track from the scene's own
/// `scene_vab_stream` SEQ entries ([`crate::scene_assets`]) rather than from a
/// resident id, so it has neither of this kernel's two inputs: the pending-BGM
/// word `_DAT_8007BAC8` and the per-scene sequence base `_DAT_80084540`. The
/// slot arithmetic only means anything once a scene entry carries a retail BGM
/// **id**; today it carries a decoded SEQ. Wiring this is the same change that
/// would let [`crate::music_labels`] name the track a scene is about to start
/// rather than the one already playing.
pub fn field_bgm_plan(
    bgm_id: u32,
    seq_base: u32,
    playing_slot: u32,
    two_part_latched: bool,
) -> FieldBgmPlan {
    let slot = if bgm_id < FIELD_BGM_DIRECT_ID_MIN {
        bgm_id
            .wrapping_add(seq_base)
            .wrapping_add(FIELD_BGM_BASE_BIAS)
    } else {
        bgm_id
    };
    FieldBgmPlan {
        slot,
        already_playing: slot == playing_slot,
        two_part_streams: (!two_part_latched && bgm_id == FIELD_BGM_TWO_PART_ID)
            .then_some(FIELD_BGM_TWO_PART_STREAMS),
    }
}

/// Bytes of GPU primitive buffer per parsed scene TMD.
pub const FIELD_PRIM_BUFFER_BYTES_PER_TMD: u32 = 1 << 10;

/// The fixed primitive-buffer request the cutscene-ish arm makes instead of
/// scaling with the TMD count.
pub const FIELD_PRIM_BUFFER_FIXED: u32 = 0x2800;

/// Primitive-buffer size the initialiser asks `FUN_8001E3B8` for.
///
/// PORT: FUN_801D6704 (`0x801d73e0..0x801d7434`).
///
/// `tmd_count` is the MAN decoder's return (`FUN_8003AEB0`, printed by the
/// initialiser's own `tmds: %d` trace). `fixed` is the both-flags-set case
/// (`_DAT_8007B868 != 0` **and** `_DAT_8007B8BE != 0`), which also installs
/// the alternate double-buffer addresses; every other combination scales with
/// the TMD count. Retail encodes the scale as `(count << 16) >> 6`, i.e. 1 KB
/// per TMD on the sign-extended `s16` count.
///
/// NOT WIRED: the engine allocates no GPU primitive buffer. Retail sizes one
/// arena up front because the PSX ordering table is a fixed block the frame
/// builder fills; `engine-render` builds draw lists on wgpu and lets the
/// backend own the allocation, so there is no consumer for a byte count and no
/// `FUN_8001E3B8` counterpart to hand it to. The kernel is kept because the
/// **ratio** is a fidelity datum - it says a retail scene budgets 1 KB of
/// primitive space per parsed TMD - which a future faithful-mode arena would
/// need.
pub fn field_prim_buffer_bytes(tmd_count: i16, fixed: bool) -> u32 {
    if fixed {
        FIELD_PRIM_BUFFER_FIXED
    } else {
        ((i32::from(tmd_count) << 16) >> 6) as u32
    }
}

/// Total bytes of the per-scene actor work buffer the initialiser allocates.
pub const FIELD_ACTOR_WORK_BYTES: usize = 0x824;

/// Slot count written to the work buffer's `+0x2` header word.
pub const FIELD_ACTOR_WORK_SLOTS: u16 = 0x50;

/// Stride of one work-buffer slot - the initialiser's clear loop steps by this
/// while zeroing each slot's `+0xA9` byte.
pub const FIELD_ACTOR_WORK_STRIDE: usize = 0x18;

/// Number of per-list actor sweeps a warp entry runs (`FUN_801D7518` once per
/// actor list, over the scene control block's seven list heads).
pub const FIELD_ACTOR_LIST_SWEEPS: usize = 7;

/// Master game mode the initialiser leaves behind, so the field per-frame loop
/// ("MAIN MODE") takes the next frame.
pub const FIELD_NEXT_GAME_MODE: u8 = 3;

/// Master game mode that aborts the BGM wait barrier
/// (`0x801d72dc`: `_DAT_8007B83C == 0x16` breaks the retry loop).
pub const FIELD_BGM_WAIT_ABORT_MODE: i16 = 0x16;

/// One ordered step of the field scene initialiser.
///
/// The list is the *shape* of `FUN_801D6704` at statement granularity: it
/// records what the initialiser does and in what order, so an engine scene
/// entry can be diffed against retail's ordering (several steps are
/// order-sensitive - the map load must precede the descriptor walk, and the
/// BGM barrier must follow the slot resolve).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldInitStep {
    /// Copy the pending scene name into the active buffer and sync it
    /// (`FUN_80056758` / `FUN_8001D7F8`).
    SyncSceneName,
    /// Reset the per-scene counters and the fog / camera defaults.
    ResetSceneGlobals,
    /// Release the previous scene's streaming channels (`FUN_8001FF58` on
    /// channels `0`, `7`, `8`, `0xB`).
    ReleaseStreamChannels,
    /// Reset the field camera defaults (`FUN_80025C24`).
    ResetFieldCamera,
    /// Sweep every actor list on a warp entry, or allocate the player actor
    /// on a cold one.
    ActorLists,
    /// Load the scene's field assets (`FUN_8001F7C0` against the `.MAP` LBA).
    LoadFieldAssets,
    /// Walk the asset descriptor pairs (`FUN_80020224`, `a0 = 0`) - the sole
    /// runtime caller of the descriptor walker.
    WalkAssetDescriptors,
    /// Rebuild the object-grid marker bits (`FUN_80017BEC`).
    RefreshObjectGrid,
    /// Decode the scene MAN and take the camera anchor + TMD count
    /// (`FUN_8003AEB0`).
    DecodeMan,
    /// Spawn one CLUT-walk actor per kingdom slot-5 table entry
    /// (`FUN_80024CFC`).
    SpawnClutWalkActors,
    /// Install the camera view window and seat the player ([`field_spawn`]).
    SeatPlayer,
    /// Allocate + clear the actor work buffer
    /// ([`FIELD_ACTOR_WORK_BYTES`]).
    AllocActorWorkBuffer,
    /// Resolve the BGM slot and start the load ([`field_bgm_plan`]).
    ResolveBgm,
    /// Spin until the audio driver acknowledges the slot, unless the master
    /// game mode is [`FIELD_BGM_WAIT_ABORT_MODE`].
    AwaitBgmLoaded,
    /// Allocate the GPU primitive buffer ([`field_prim_buffer_bytes`]).
    AllocPrimBuffer,
    /// Hand off: write [`FIELD_NEXT_GAME_MODE`].
    EnterFieldLoop,
}

/// The field initialiser's step order.
///
/// REF: FUN_801D6704 (whole-body ordering) - a `REF:` and not a `PORT:` on
/// purpose. `port-catalog.py` can only anchor a tag to a `fn`, a type with an
/// `impl` block, or the whole module, so a `PORT:` on a `const` silently widens
/// to the file - and this file is live through [`field_spawn`], which made a
/// correct `NOT WIRED:` here read as a stale disclosure. The address keeps its
/// port anchors: the module tag above, plus [`field_spawn`],
/// [`field_bgm_plan`] and [`field_prim_buffer_bytes`], each of which carries
/// its own wiring verdict at the granularity it can be judged.
///
/// The list itself remains a diffable description rather than an executable
/// plan - nothing in the crate walks it. Of the kernels it names,
/// [`field_spawn`] *is* wired (scene entry seats the player through it);
/// walking the list would need a scene-entry driver that is a step machine
/// rather than a straight-line function.
pub const FIELD_INIT_STEPS: [FieldInitStep; 16] = [
    FieldInitStep::SyncSceneName,
    FieldInitStep::ResetSceneGlobals,
    FieldInitStep::ReleaseStreamChannels,
    FieldInitStep::ResetFieldCamera,
    FieldInitStep::ActorLists,
    FieldInitStep::LoadFieldAssets,
    FieldInitStep::WalkAssetDescriptors,
    FieldInitStep::RefreshObjectGrid,
    FieldInitStep::DecodeMan,
    FieldInitStep::SpawnClutWalkActors,
    FieldInitStep::SeatPlayer,
    FieldInitStep::AllocActorWorkBuffer,
    FieldInitStep::ResolveBgm,
    FieldInitStep::AwaitBgmLoaded,
    FieldInitStep::AllocPrimBuffer,
    FieldInitStep::EnterFieldLoop,
];

/// Everything the Baka Fighter duel-arena overlay initialiser seeds.
///
/// REF: FUN_801CF00C - the `PORT:` for this address sits on
/// [`duel_overlay_init`], the free function that builds this record. This is a
/// plain data struct with no `impl` block, and `port-catalog.py` widens a type
/// anchor with no `impl` to the whole file, which is live through
/// [`field_spawn`] - so a `PORT:` here reported a correct `NOT WIRED:` as a
/// stale disclosure. The wiring verdict lives on the function instead.
///
/// A straight-line routine: no branches except the asset-load form (a dev
/// by-name load when the debug flag `_DAT_8007B8C2` is clear, an id load when
/// it is set) and no loops. Each field below is one of its stores, keyed by
/// the runtime global it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuelOverlayInit {
    /// Display width handed to `FUN_8001DAF8` (`0x140` = 320).
    pub screen_width: u16,
    /// Ordering-table depth handed to `FUN_8001DCF8`.
    pub ot_depth: u8,
    /// Constant third argument of the BGM start call `FUN_80062004`. Not
    /// identified as a volume: the field initialiser passes `0x78` on one arm
    /// and `0xB4` on another, so it is a per-site tuning constant.
    pub bgm_arg3: u16,
    /// `_DAT_801DBED0` - the round-win **target**, and the drawn round-win pip
    /// count. Best of three, so `2`.
    pub round_win_target: u32,
    /// `_DAT_801DBEBC` - the stage counter seed.
    pub stage_seed: u32,
    /// `+0x4` and `+0x8` of the camera-target block at `0x800840B8` - the same
    /// two words the field-camera reset `FUN_80025C24` writes on field entry,
    /// re-seeded here for the arena view.
    pub camera_pair: (u32, u32),
    /// Scratchpad pair `0x1F8003F8` / `0x1F8003FA` - the duel's own view
    /// window, `6` by `6` tiles rather than the field's `0x0E` by `0x10`.
    pub window_tiles: (u16, u16),
    /// The two fighter installs (`FUN_801D4C50` with `0` then `1`).
    pub fighter_slots: [u8; 2],
    /// Streaming asset id loaded twice (the second time with the
    /// re-read flag set) - the duel SFX bank.
    pub sfx_stream_id: u32,
    /// Streaming asset id of the duel voice archive.
    pub voice_stream_id: u32,
}

/// The duel arena's initialiser constants.
///
/// PORT: FUN_801CF00C (`0x801cf00c..0x801cf384`).
///
/// NOT WIRED: the duel arena's engine entry point is the rules engine in
/// `baka_fighter.rs`, which starts from a match state rather than an overlay
/// load - there is no overlay-entry host in this crate to call an initialiser
/// from, and the module that owns the duel is outside this file's scope. The
/// values are the retail seeds a future duel scene host reads; the two that
/// already have engine mirrors (`round_win_target`, `fighter_slots`) agree
/// with the rules engine's own best-of-three.
pub const fn duel_overlay_init() -> DuelOverlayInit {
    DuelOverlayInit {
        screen_width: 0x140,
        ot_depth: 0x0C,
        bgm_arg3: 0x78,
        round_win_target: 2,
        stage_seed: 3,
        camera_pair: (0x2D0, 0x3980),
        window_tiles: (6, 6),
        fighter_slots: [0, 1],
        sfx_stream_id: 0x367,
        voice_stream_id: 0x415,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_entry_seats_at_the_window_centre() {
        let s = field_spawn(FieldEntryMode::Cold, (0x100, 0x200), (0, 0), false, (0, 0));
        // The extra actor is the (0xA40, 0, 0xA40) spawn, with no sub-tile
        // term - the terms are gated on the warp arm.
        assert_eq!(s.extra_actor, Some(FIELD_COLD_SPAWN));
        // The player itself keeps the MAN anchor.
        assert_eq!(s.player, (0x100, 0x200));
        assert_eq!(s.window_tile, (2, 4));
    }

    #[test]
    fn warp_entry_takes_the_saved_coords_and_spawns_no_extra_actor() {
        let s = field_spawn(
            FieldEntryMode::Warp,
            (0x100, 0x200),
            (0x0A44, 0x0BC0),
            false,
            (0, 0),
        );
        assert_eq!(s.player, (0x0A44, 0x0BC0));
        assert_eq!(s.extra_actor, None);
        assert_eq!(s.window_tile, (0x14, 0x17));
    }

    #[test]
    fn recentre_puts_the_window_on_the_map_origin() {
        let s = field_spawn(FieldEntryMode::Warp, (0, 0), (0, 0), true, (0x10, 0x20));
        // origin_x + 0x0E/2 - 1 = 0x16, origin_z + 0x10/2 = 0x28.
        assert_eq!(s.window_tile, (0x16, 0x28));
        assert_eq!(s.player, (0x16 * 0x80 + 0x40, 0x28 * 0x80 + 0x40));
    }

    #[test]
    fn negative_anchor_tiles_truncate_toward_zero() {
        // -1 .. -0x7F all live on tile 0 under retail's `+0x7F` bias.
        let s = field_spawn(FieldEntryMode::Cold, (-1, -0x7F), (0, 0), false, (0, 0));
        assert_eq!(s.window_tile, (0, 0));
        let s = field_spawn(FieldEntryMode::Cold, (-0x80, -0x81), (0, 0), false, (0, 0));
        assert_eq!(s.window_tile, (-1, -1));
    }

    #[test]
    fn bgm_relative_ids_bias_by_the_sequence_base() {
        let p = field_bgm_plan(4, 100, 0, false);
        assert_eq!(p.slot, 4 + 100 + 6);
        assert!(!p.already_playing);
        assert_eq!(p.two_part_streams, None);
    }

    #[test]
    fn bgm_global_ids_pass_through_and_latch_when_already_playing() {
        let p = field_bgm_plan(2000, 100, 2000, false);
        assert_eq!(p.slot, 2000);
        assert!(p.already_playing);
    }

    #[test]
    fn the_two_part_track_loads_both_streams_once() {
        let p = field_bgm_plan(FIELD_BGM_TWO_PART_ID, 0, 0, false);
        assert_eq!(p.slot, FIELD_BGM_TWO_PART_ID);
        assert_eq!(p.two_part_streams, Some(FIELD_BGM_TWO_PART_STREAMS));
        // The latch stops the second entry re-reading them.
        let p = field_bgm_plan(FIELD_BGM_TWO_PART_ID, 0, 0, true);
        assert_eq!(p.two_part_streams, None);
        // And no other id takes that arm.
        let p = field_bgm_plan(FIELD_BGM_TWO_PART_ID - 1, 0, 0, false);
        assert_eq!(p.two_part_streams, None);
    }

    #[test]
    fn prim_buffer_scales_at_one_kib_per_tmd() {
        assert_eq!(field_prim_buffer_bytes(1, false), 1 << 10);
        assert_eq!(field_prim_buffer_bytes(72, false), 72 << 10);
        assert_eq!(field_prim_buffer_bytes(72, true), FIELD_PRIM_BUFFER_FIXED);
    }

    #[test]
    fn duel_init_is_best_of_three_with_two_fighters() {
        let d = duel_overlay_init();
        assert_eq!(d.round_win_target, 2);
        assert_eq!(d.fighter_slots, [0, 1]);
        assert_eq!(d.screen_width, 320);
        // The duel's window is square and much smaller than the field's.
        assert_eq!(d.window_tiles, (6, 6));
        assert_ne!(
            (i32::from(d.window_tiles.0), i32::from(d.window_tiles.1)),
            FIELD_CAMERA_WINDOW_TILES
        );
    }

    #[test]
    fn the_init_step_list_keeps_the_order_sensitive_pairs() {
        let idx = |s: FieldInitStep| FIELD_INIT_STEPS.iter().position(|&x| x == s).unwrap();
        assert!(idx(FieldInitStep::LoadFieldAssets) < idx(FieldInitStep::WalkAssetDescriptors));
        assert!(idx(FieldInitStep::DecodeMan) < idx(FieldInitStep::SeatPlayer));
        assert!(idx(FieldInitStep::ResolveBgm) < idx(FieldInitStep::AwaitBgmLoaded));
        assert_eq!(
            *FIELD_INIT_STEPS.last().unwrap(),
            FieldInitStep::EnterFieldLoop
        );
    }
}
