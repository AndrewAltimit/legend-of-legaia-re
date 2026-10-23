//! What each mode's init routine stages: the core-state reset, the staged init plans and the bare inits.

use super::*;

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
/// `legaia_engine_scene::overlay_loader::load_overlay_a`), wait again, then a `jal`
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
//                    ported at engine-system::sound_state::SoundDetachLatch)
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
// missing prerequisite as `legaia_engine_scene::overlay_loader`.
//
// The reference scan sharpens what "no dispatcher" means here: `0x80025980`
// has **no `jal` anywhere on the disc**. Its one reference of any form is the
// word at `0x800709DC`, which is `mode_table[24] + 0x10` in the 28 x 24-byte
// table at `0x8007078C` - so retail reaches it only by indexing that table,
// and `legaia_game_tables::mode_table` already recovers the table from the disc.
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
// `legaia_engine_scene::overlay_loader` is missing.
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
/// see [`READ_INIT_TARGET`] and `legaia_engine_title::publisher_logos`.
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
