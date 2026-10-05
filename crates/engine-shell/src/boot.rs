//! Top-level engine boot session.
//!
//! Composes the per-crate primitives ([`legaia_engine_core::scene::SceneHost`],
//! [`legaia_engine_core::camera::Camera`], the BGM director from
//! [`crate::bgm::AudioBgmDirector`]) into one struct the binary drives per
//! frame. Mirrors the retail boot flow:
//!
//! 1. Open the extracted PROT + CDNAME map.
//! 2. Load a starting scene (the binary defaults to `town01`).
//! 3. Pick the scene's primary VAB bank, upload it to the SPU, and stash
//!    in the BGM director for subsequent op-`0x35` triggers.
//! 4. Drive the world tick + camera tick + event routing each frame.
//!
//! No window / renderer here - the binary owns winit + wgpu (or in headless
//! CI mode, no window). [`BootSession::tick`] is the per-frame driver
//! callable from either path.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::AudioOut;
use legaia_engine_core::camera::Camera;
use legaia_engine_core::field_menu::{FieldMenuGate, FieldMenuSession};
use legaia_engine_core::field_menu_dispatch::{
    FieldMenuSubsession, SubsessionHandoff, finish_subsession, tick_open_subsession, tick_root_list,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::magic_xp::SpellLevelNotice;
use legaia_engine_core::options::OptionsState;
use legaia_engine_core::save_screen::{SaveCommit, SaveScreenFlow};
use legaia_engine_core::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_core::scene::{BgmDirector, DefaultMapIdResolver, SceneHost, SceneTickEvent};
use legaia_engine_core::world::SceneMode;

use crate::bgm::AudioBgmDirector;

/// Options for [`BootSession::enter_field_live`] - how much of the live
/// gameplay loop to arm when dropping into a field scene.
#[derive(Debug, Clone, Default)]
pub struct FieldLiveOpts {
    /// Arm the step-driven random-encounter roll
    /// (`World::toggles.live_gameplay_loop`). Independent of `player_battle`: a
    /// battle the engine is already in is always driven to resolution either
    /// way, so this decides only whether one *starts* on its own.
    pub live_loop: bool,
    /// Make battles player-driven (command menu) and install the
    /// Seru-learning registry a player-driven battle needs. (The item /
    /// spell / equipment catalogs are installed unconditionally now - the
    /// field pause-menu reads them regardless of these flags.)
    pub player_battle: bool,
    /// Battle<->Field BGM swap override. `None` keeps the shipped default
    /// (retail's standard battle theme, global BGM 2026 - see
    /// [`legaia_engine_core::live_loop::LiveLoopOpts::playable`]);
    /// `Some(0)` disables the swap; any other id replaces the track
    /// (scene-local ids resolve through the scene's BGM table, `>= 2000`
    /// through the global `music_01` pool).
    pub battle_bgm: Option<u16>,
}

impl FieldLiveOpts {
    /// Project onto the engine-core kernel's options
    /// ([`legaia_engine_core::live_loop::LiveLoopOpts`]) - the shared shape
    /// both hosts arm the loop through.
    pub fn to_live_loop_opts(&self) -> legaia_engine_core::live_loop::LiveLoopOpts {
        let mut opts = legaia_engine_core::live_loop::LiveLoopOpts::playable();
        opts.live_loop = self.live_loop;
        opts.player_battle = self.player_battle;
        if let Some(id) = self.battle_bgm {
            opts.battle_bgm = (id != 0).then_some(id);
        }
        opts
    }
}

/// Default scene the binary boots into when no `--scene` is supplied. Uses
/// the canonical first-town label from CDNAME.TXT.
pub const DEFAULT_BOOT_SCENE: &str = "town01";

/// Total SPU RAM in bytes (PSX hardware constant).
pub(crate) const SPU_RAM_BYTES: u32 = 512 * 1024;
/// Byte offset reserved for voice-0 / scratchpad - banks are allocated
/// above this. Mirrors the asset-viewer SEQ playback path.
pub(crate) const SPU_RESERVED_BYTES: u32 = legaia_engine_audio::spu_layout::SPU_RESERVED_BYTES;
/// SPU RAM reserved at the TOP of the map for the resident SFX banks: the
/// slot-0 system bank, and above it the region VAB slots `2` and `6` share
/// (one SPU base in retail, refilled per game mode). Carving a dedicated top
/// region keeps a scene-BGM upload from stomping the SFX samples.
///
/// The size is arithmetic, and the browser host's `SFX_BANK_SPU_BYTES` must
/// stay equal to it. PROT 0868's VAG bodies total 59136 bytes and the largest
/// bank the shared region takes in a mode the port stages it for, PROT 0869,
/// 188128 (PROT 0876 is 174192), so the pair needs 247264 and 0x3D000
/// (249856) holds them. It cannot go higher: the BGM region is what is left,
/// and one step up (`0x3E000`) leaves 266240 - under the two largest scene BGM
/// VABs on the disc (269632, 268496), i.e. it would start silencing music that
/// plays today.
/// One value with the browser host's by construction: both re-export
/// [`legaia_engine_audio::spu_layout::SFX_REGION_BYTES`].
pub const SFX_BANK_SPU_BYTES: u32 = legaia_engine_audio::spu_layout::SFX_REGION_BYTES;

/// One-time configuration for [`BootSession::open`].
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// Starting scene name (CDNAME label).
    pub scene: String,
    /// Whether to open the audio output. Set `false` for headless tests
    /// (cpal will fail to enumerate devices in CI).
    pub enable_audio: bool,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            scene: DEFAULT_BOOT_SCENE.to_string(),
            enable_audio: true,
        }
    }
}

/// Source of PROT.DAT + CDNAME.TXT bytes for a [`BootSession::open*`]
/// call. Internal - public construction is via the typed entry points
/// [`BootSession::open`] and [`BootSession::open_disc`].
enum SceneSource<'a> {
    Extracted(&'a Path),
    #[cfg(not(target_arch = "wasm32"))]
    Disc(&'a Path),
}

/// Per-frame session bundle. The binary owns one of these and calls
/// [`tick`](Self::tick) every frame.
pub struct BootSession {
    pub host: SceneHost,
    pub camera: Camera,
    pub audio: Option<Arc<AudioOut>>,
    pub bgm: Option<AudioBgmDirector>,
    /// Wall-clock frame counter, separate from `host.world.frame` (which
    /// includes pause-time skips when those land).
    pub frames: u64,
    /// New-game starting-party template parsed from the boot source's
    /// `SCUS_942.54`, if present. Used by [`BootSession::begin_new_game`] to
    /// seed a faithful starting roster; `None` when the executable couldn't be
    /// read (e.g. a raw PROT.DAT-only source), in which case New Game keeps the
    /// world's default scaffold party.
    pub starting_party: Option<legaia_asset::new_game::StartingParty>,
    /// New-game starting inventory decoded from the boot source's `SCUS_942.54`
    /// seed code (`FUN_80034A6C`), if present. Vanilla retail is Healing Leaf
    /// ×5; the starting-item randomizer rewrites it. Used by
    /// [`BootSession::begin_new_game`] to seed the opening bag faithfully.
    pub starting_inventory: Option<legaia_asset::new_game::StartingInventory>,
    /// Disc-accurate equipment modifier table keyed by real item ids, parsed
    /// from the boot source's `SCUS_942.54` ([`legaia_asset::equip_stats`]).
    /// Preferred over the fabricated-id vanilla catalog when installing the
    /// battle-stat equipment table; `None` on disc-free builds.
    pub equip_modifier_table: Option<legaia_engine_core::battle_stats::EquipmentTable>,
    /// Disc-pinned equip restrictions (character mask `+6` + slot category
    /// `+7`) keyed by real item ids, parsed from the same equipment stat-bonus
    /// table ([`legaia_asset::equip_stats`]). Drives the equip screen's
    /// per-character item gate
    /// ([`legaia_engine_core::equip_session::EquipSession::new_with_restrictions`]);
    /// `None` on disc-free builds.
    pub equip_restrictions: Option<legaia_engine_core::equipment::DiscEquipInfo>,
    /// The **raw** equipment stat-bonus records (`DAT_80074F68`) behind the two
    /// derived tables above. The Items screen's Throw Out list reads each
    /// record's `+7` flags byte, which neither derived form keeps; `None` on
    /// disc-free builds, where no row reports the no-discard bit.
    pub equip_stats: Option<legaia_asset::equip_stats::EquipStatTable>,
    /// Player Seru-magic catalog with MP cost + target shape read from the
    /// boot source's `SCUS_942.54` spell table ([`legaia_asset::spell_names`]).
    /// Preferred over the pinned `retail_seru_magic_catalog` when installing the
    /// battle spell catalog, so a randomized / translated disc is honoured;
    /// `None` on disc-free builds.
    pub spell_catalog: Option<legaia_engine_core::spells::SpellCatalog>,
    /// Static per-monster **steal** table (`DAT_80077828`, fields
    /// `[chance, item]`), parsed from the same `SCUS_942.54`
    /// ([`legaia_asset::steal_table`]). Installed on the world beside the
    /// equipment / spell / item catalogs so PROT 0941's Steal can resolve a
    /// monster-seat victim; `None` on disc-free builds, which leaves that leg
    /// drawing no roll at all.
    pub steal_table: Option<legaia_asset::steal_table::StealTable>,
    /// The real retail proportional dialog font, decoded straight from the
    /// boot source (`PROT.DAT`'s 4bpp font TIM + the `SCUS_942.54` advance
    /// table at `0x80073F1C`) - **no mednafen save state required**.
    ///
    /// Every native text draw goes through this font's metrics, and retail's
    /// glyph advance is proportional (`pen_x += widths[c] + 1`, pinned at
    /// `FUN_80036888` body `0x80036B9C`). The `extracted/font/` artifacts the
    /// legacy loader wants only exist after a `font-extract` run, which needs
    /// a save state - so a disc-only boot used to silently fall back to the
    /// fixed-width placeholder and rendered every string ~35% too wide. This
    /// field is the disc-derived fallback that keeps a plain
    /// `--disc <image>` boot on the retail metrics. `None` when the source
    /// carries neither the font TIM nor the executable.
    pub dialog_font: Option<legaia_font::Font>,
    /// The `0xCE` escape sprites (controller buttons, icons) off the boot
    /// source, for a host font that did not come from [`Self::dialog_font`]
    /// (an `extracted/font/` load) to attach
    /// ([`legaia_font::Font::with_escape_icons`]). `None` when unreachable.
    pub escape_icons: Option<legaia_font::escape_icons::EscapeIcons>,
    /// The session's **seat at the retail mode table**: the port's copy of
    /// `_DAT_8007B83C` ([`legaia_engine_core::mode::ModeSeat`]).
    ///
    /// [`Self::tick`] drives it once per frame, and the session's own
    /// transitions write it where retail's code stores the word - field entry
    /// through `MAIN INIT`, the pause menu through `CARD INIT`. Two things
    /// come out of it that nothing else in the port produces: the mode-change
    /// edge (which swallows the pad edge that caused the transition, the way
    /// `0x800161EC` / `0x800161F8` do) and a real `game_mode` for every frame
    /// the mode-trace oracle samples.
    pub mode_seat: legaia_engine_core::mode::ModeSeat,
    /// In-field pause-menu session, when open. Retail runs the pause menu
    /// under the CARD mode pair (`_DAT_8007B83C = 0x17`, `CARD MODE`, in
    /// every menu-open capture); the session-hosted equivalent holds
    /// [`World::mode`](legaia_engine_core::world::World::mode) at
    /// [`SceneMode::Menu`] while `Some`, suspending field dispatch
    /// underneath. Opened via [`BootSession::open_field_menu`] (or the
    /// Start-edge path inside [`BootSession::tick`]); the windowed host
    /// layers its sub-session UI stack on top of this same session.
    pub field_menu: Option<FieldMenuSession>,
    /// The sub-session beneath a **suspended** [`Self::field_menu`] - the
    /// screen a confirmed row opened (Items / Magic / Equip / Status /
    /// Options / Load / Save).
    ///
    /// Retail's pause menu is two levels deep: the root list suspends itself
    /// on confirm and the routed sub-screen owns the pad until it finishes.
    /// [`Self::tick`] drives that second level, so a driver that only calls
    /// `set_pad` + `tick` reaches every pause-menu screen rather than
    /// bouncing off the root list. Hosts that own their own UI stack (the
    /// windowed shell, the browser play page) never enter `tick`'s menu arm
    /// and keep driving their own.
    pub field_menu_sub: Option<FieldMenuSubsession>,
    /// Options the Config sub-session is built from, and written back to when
    /// it closes. Retail commits option edits inside the value popup and
    /// never reverts, so the session's final state is the state.
    pub options_state: OptionsState,
    /// Rack behind the Load / Save rows. Its *kind* is what puts the
    /// sub-session in retail's two-stage card flow
    /// ([`SaveRack::CardPorts`]), which is why a host supplies a rack rather
    /// than a mode flag. Defaults to an empty flat rack - a driver that wants
    /// the save legs calls [`Self::set_save_rack`].
    save_rack: SaveRack,
    /// The fifteen blocks behind each port of [`Self::save_rack`], indexed by
    /// port. Answers [`SaveScreenFlow::pending_read`] when the card-read beat
    /// resolves; a port past the end of this list reads as unmounted.
    save_port_blocks: Vec<Vec<SlotSnapshot>>,
    /// Driver for the two-stage card flow (pill row -> block grid). Shared
    /// with the windowed host so the second stage is one implementation.
    save_flow: SaveScreenFlow,
    /// The save / load pick the last finished Save sub-session produced, if
    /// any. **Not** acted on here: the persistence backend is host-owned (a
    /// save directory, a card image), so `BootSession` runs the *flow* and
    /// hands the caller the block it landed on.
    pub last_save_commit: Option<SaveCommit>,
    /// Spell level-up notice a menu cast produced. Retail's window 7 owns the
    /// pad until dismissed; headless drivers that don't render it just read
    /// and clear this.
    pub spell_level_notice: Option<SpellLevelNotice>,
    /// Art-learned notice a Hyper-Art book produced from the Items screen
    /// (retail's window 8); latched the same way.
    pub art_learned_notice: Option<legaia_engine_core::pause_screens::ArtLearnedNotice>,
    /// Scene mode the world ran before the pause menu opened, restored by
    /// [`BootSession::close_field_menu`].
    field_menu_resume: SceneMode,
    /// `true` when the caller drains the world's per-tick presentation queues
    /// itself after every [`Self::tick`] - the play window does. Otherwise
    /// the session does it ([`HostQueueMarks`]).
    host_drains_queues: bool,
    /// Queue lengths the last full [`Self::tick`] left behind.
    queue_marks: HostQueueMarks,
}

/// How much of each per-tick world queue a tick left behind.
///
/// Both play hosts consume these queues after every tick (the window's
/// `drain_and_route_field_events` / `drain_and_log_battle_events` /
/// minigame-cue drain; the browser runtime's twins), so an event still queued
/// when the next tick starts is one no host would ever see again. A driver
/// that only ticks the session - every headless run - consumed none of them,
/// and the queues grew without bound (`map01` queues field events every
/// frame). [`BootSession::tick`] therefore drops, at the start of a tick, the
/// entries the previous tick left and nobody took: the front `min(mark, len)`
/// of each queue. Anything queued **between** ticks (a scene entry's BGM
/// start, a caller's own push) sits behind the mark and survives to the tick
/// that consumes it, and a caller that reads a queue after `tick` - the BGM
/// oracles route the field queue themselves - still sees the whole tick.
///
/// A host that drains every tick itself opts out
/// ([`BootSession::set_host_drains_queues`]), which keeps the play window
/// byte-for-byte what it was.
///
/// The queues with a **world** side are not dropped but run: the effect
/// spawns (`World::route_battle_effect_spawns`), the summon / move-FX
/// requests, the effect scene-graph tick that retires what they seat, the
/// ANIMATE cues and the scripted VRAM effects all go through
/// [`BootSession::run_world_frame_tail`] before the marks are taken, so a
/// headless run executes the same world the play hosts do. Routing the spawns
/// alone would park the battle action state machine on an effect nothing
/// retires, which is why the tail runs whole.
#[derive(Debug, Default, Clone, Copy)]
struct HostQueueMarks {
    field_events: usize,
    battle_events: usize,
    hit_fx: usize,
    hit_events: usize,
    sfx_cues: usize,
    shout_cues: usize,
    xa_cues: usize,
    xa_prestage: usize,
    clut_stages: usize,
    effect_spawns: usize,
    minigame_sfx: usize,
}

impl HostQueueMarks {
    fn record(world: &legaia_engine_core::world::World) -> Self {
        Self {
            field_events: world.pending_field_events.len(),
            battle_events: world.pending_battle_events.len(),
            hit_fx: world.battle.hit_fx.len(),
            hit_events: world.battle.hit_events.len(),
            sfx_cues: world.audio.battle_sfx_cues.len(),
            shout_cues: world.audio.battle_shout_cues.len(),
            xa_cues: world.audio.battle_xa_cues.len(),
            xa_prestage: world.audio.battle_xa_prestage.len(),
            clut_stages: world.battle.clut_stages.len(),
            effect_spawns: world.battle.effect_spawns.len(),
            minigame_sfx: world.minigames.pending_sfx.len(),
        }
    }

    /// Drop the entries the last tick left that nobody consumed.
    fn drop_stale(self, world: &mut legaia_engine_core::world::World) {
        fn front<T>(q: &mut Vec<T>, n: usize) {
            let n = n.min(q.len());
            q.drain(..n);
        }
        front(&mut world.pending_field_events, self.field_events);
        front(&mut world.pending_battle_events, self.battle_events);
        front(&mut world.battle.hit_fx, self.hit_fx);
        front(&mut world.battle.hit_events, self.hit_events);
        front(&mut world.audio.battle_sfx_cues, self.sfx_cues);
        front(&mut world.audio.battle_shout_cues, self.shout_cues);
        front(&mut world.audio.battle_xa_cues, self.xa_cues);
        front(&mut world.audio.battle_xa_prestage, self.xa_prestage);
        front(&mut world.battle.clut_stages, self.clut_stages);
        front(&mut world.battle.effect_spawns, self.effect_spawns);
        front(&mut world.minigames.pending_sfx, self.minigame_sfx);
    }
}

/// Read + parse the new-game starting-party template from a boot source's
/// `SCUS_942.54`. Returns `None` (not an error) when the executable isn't
/// reachable or doesn't parse, so a boot never fails just because the seed
/// data is unavailable.
fn read_starting_party(source: &SceneSource<'_>) -> Option<legaia_asset::new_game::StartingParty> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::new_game::StartingParty::from_scus(&scus)
}

/// Read + decode the new-game starting-inventory seed from a boot source's
/// `SCUS_942.54` (`FUN_80034A6C`). Returns `None` when the executable isn't
/// reachable or doesn't decode, so a boot never fails on missing seed data.
fn read_starting_inventory(
    source: &SceneSource<'_>,
) -> Option<legaia_asset::new_game::StartingInventory> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::new_game::StartingInventory::from_scus(&scus)
}

/// Read the raw `SCUS_942.54` bytes from a boot source. Returns `None`
/// (not an error) when the executable isn't reachable.
fn read_scus(source: &SceneSource<'_>) -> Option<Vec<u8>> {
    use legaia_engine_core::Vfs;
    match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
    }
}

/// Build the retail proportional dialog font straight from the boot source:
/// the 4bpp font TIM at [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] inside
/// `PROT.DAT` supplies the glyph bitmaps and `SCUS_942.54` the per-character
/// advance table (`0x80073F1C`).
///
/// This is the disc-only path - it needs no `extracted/font/` artifacts and no
/// save state, so a `--disc <image>` boot renders text on retail metrics
/// instead of the fixed-width placeholder. Returns `None` (never an error)
/// when either half is unreachable.
fn read_dialog_font(
    index: &legaia_engine_core::scene::ProtIndex,
    source: &SceneSource<'_>,
) -> Option<legaia_font::Font> {
    let tim = index
        .prot_dat_raw_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .ok()?;
    let scus = read_scus(source)?;
    let font = legaia_font::Font::from_disc_tim_and_scus(&tim, &scus).ok()?;
    Some(attach_escape_icons(font, index, &scus))
}

/// Attach the `0xCE` escape sprites (controller buttons, icons) to `font`
/// from the boot-resident TIMs at the head of `PROT.DAT` and the SCUS sprite
/// records, so every shared text layout draws them
/// ([`legaia_font::Font::with_escape_icons`]). Unchanged when unreachable.
pub fn attach_escape_icons(
    font: legaia_font::Font,
    index: &legaia_engine_core::scene::ProtIndex,
    scus: &[u8],
) -> legaia_font::Font {
    use legaia_font::escape_icons::{ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
    match index.prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN) {
        Ok(head) => font.with_escape_icons_from_disc(&head, scus),
        Err(_) => font,
    }
}

/// Read + decode the sound-effect descriptor bank from a boot source's
/// `SCUS_942.54` (`DAT_8006F198`, see `sfx-table.md`). Returns `None` when the
/// executable isn't reachable or the table doesn't decode, so a boot never
/// fails on missing SFX data - the director just keeps its empty bank and
/// resolved cues no-op until one is staged.
fn read_sfx_bank(
    source: &SceneSource<'_>,
) -> Option<(legaia_engine_audio::SfxBank, Vec<(u8, u8)>)> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let table = legaia_asset::sfx_table::SfxTable::from_scus(&scus)?;
    let bank = legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.flags)),
    );
    // The routing half of the same table: which VAB slot each cue keys.
    Some((bank, table.cue_slots().collect()))
}

/// Demux + decode the battle **arts-voice shout** banks from a disc image:
/// the per-character CD-XA clip files (`XA2.XA` Vahn / `XA4.XA` Noa /
/// `XA6.XA` Gala, 16-channel short-mono banks) plus the `SCUS_942.54`
/// cue tables that map each art's action constant to its candidate-channel
/// pool (`legaia_art::arts_voice`, the `FUN_8004C140` tables).
///
/// Channel demux needs the raw 2352-byte sectors (the CD-XA subheaders carry
/// the channel number; a 2048-byte ISO view strips them), so this reads the
/// disc through [`legaia_iso::raw::RawDisc`] - extracted-directory boots
/// can't stage a shout bank. Returns `None` when the disc / executable /
/// tables don't resolve; the caller degrades to silent arts.
///
/// Public so disc-gated tests can build the same bank the boot path stages.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_arts_shout_bank(disc: &Path) -> Option<legaia_engine_audio::ArtsShoutBank> {
    use legaia_engine_audio::{ArtsShoutBank, ShoutClip};
    let scus = read_scus(&SceneSource::Disc(disc))?;
    let table = legaia_art::arts_voice::ArtsVoiceTable::parse_from_scus(&scus)?;
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = ArtsShoutBank::new();
    for cslot in 0u8..3 {
        let name = legaia_art::arts_voice::clip_file(cslot as usize)?;
        // ISO paths look like `XA/XA2.XA;1` - match on the file name.
        let rec = files.iter().find_map(|(path, rec)| {
            let base = path.rsplit('/').next().unwrap_or(path);
            let base = base.split(';').next().unwrap_or(base);
            base.eq_ignore_ascii_case(name).then_some(rec)
        })?;
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let streams = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors).ok()?;
        for s in &streams {
            // The shout banks are 4-bit mono; skip anything else (a stereo or
            // 8-bit stream here would be a mis-identified file).
            if s.stereo || s.bits_per_sample != 4 {
                continue;
            }
            let (pcm, _) = legaia_xa::decode(
                &s.audio,
                legaia_xa::DecodeOptions {
                    channels: legaia_xa::Channels::Mono,
                    sample_rate: s.sample_rate,
                    bits: legaia_xa::BitsPerSample::Four,
                },
            )
            .ok()?;
            // Trim the trailing channel-padding silence so a clip's audible
            // end matches the retail read-span cutoff closely enough for the
            // back-to-back promotion queue.
            let mut end = pcm.len();
            while end > 0 && pcm[end - 1].unsigned_abs() < 8 {
                end -= 1;
            }
            let mut pcm = pcm;
            pcm.truncate(end);
            if pcm.is_empty() {
                continue;
            }
            bank.insert_clip(
                cslot,
                s.ch_no,
                ShoutClip {
                    pcm,
                    sample_rate: s.sample_rate,
                },
            );
        }
        for (action, pool) in table.pools(cslot as usize) {
            bank.set_pool(cslot, action, pool.to_vec());
        }
    }
    bank.has_clips().then_some(bank)
}

/// Clip slots the battle's one-shot CD-XA cues address, as `(slot, file)`.
/// The animation cue tracks' party voice band (`0xC8..=0xFF` re-based
/// `+0x38`, `FUN_800508DC` -> `FUN_8004FE5C`) lands on `(id - 0x100) >> 3`
/// with the `1 / 3 / 5 -> 26 / 27 / 28` remap: Vahn's `0xC8..=0xD7` on
/// slots `0` / `26`, Noa's `0xD8..=0xE7` on `2` / `27`, Gala's
/// `0xE8..=0xF7` on `4` / `28` - Vahn's Spirit clip opens with `0xC8`,
/// `XA1.XA` channel 0. `26` also carries the melee kernel's `0x10C` sting
/// and `0x1D` = `XA30.XA` the per-character block grunt. Slot `i` is
/// `XA<i+1>.XA` by the boot-built clip table's own construction
/// (`docs/subsystems/audio.md`).
pub const BATTLE_XA_CLIP_SLOTS: &[(u8, &str)] = &[
    (0, "XA1.XA"),
    (2, "XA3.XA"),
    (4, "XA5.XA"),
    (26, "XA27.XA"),
    (27, "XA28.XA"),
    (28, "XA29.XA"),
    (0x1D, "XA30.XA"),
];

/// Demux + decode the battle **one-shot clip** banks from a disc image into
/// a generic `(clip_slot, channel)` bank: the files in
/// [`BATTLE_XA_CLIP_SLOTS`]. Every 4-bit channel is decoded (mono or
/// stereo, at its subheader rate); the file's channel count is recorded so
/// the retail read span can be divided by the interleave. Same disc-only
/// caveat as [`read_arts_shout_bank`]. `None` when nothing decodes.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_battle_xa_clip_bank(disc: &Path) -> Option<legaia_engine_audio::XaClipBank> {
    use legaia_engine_audio::{XaClip, XaClipBank};
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = XaClipBank::new();
    for &(slot, name) in BATTLE_XA_CLIP_SLOTS {
        let Some(rec) = files.iter().find_map(|(path, rec)| {
            let base = path.rsplit('/').next().unwrap_or(path);
            let base = base.split(';').next().unwrap_or(base);
            base.eq_ignore_ascii_case(name).then_some(rec)
        }) else {
            continue;
        };
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let Ok(streams) = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors) else {
            continue;
        };
        let widest = streams.iter().map(|s| s.ch_no).max().unwrap_or(0);
        bank.set_channel_count(slot, widest.saturating_add(1));
        for s in &streams {
            if s.bits_per_sample != 4 {
                continue;
            }
            let channels = if s.stereo {
                legaia_xa::Channels::Stereo
            } else {
                legaia_xa::Channels::Mono
            };
            let Ok((pcm, _)) = legaia_xa::decode(
                &s.audio,
                legaia_xa::DecodeOptions {
                    channels,
                    sample_rate: s.sample_rate,
                    bits: legaia_xa::BitsPerSample::Four,
                },
            ) else {
                continue;
            };
            if pcm.is_empty() {
                continue;
            }
            bank.insert(
                slot,
                s.ch_no,
                XaClip {
                    pcm,
                    sample_rate: s.sample_rate,
                    stereo: s.stereo,
                },
            );
        }
    }
    bank.has_clips().then_some(bank)
}

/// Read the gold-shop item data (per-id buy price + "names a real item" mask)
/// from a boot source's `SCUS_942.54` item table. Returns `None` when the
/// executable isn't reachable or its item table doesn't parse, so a boot never
/// fails on missing shop data - the engine then leaves shop stock host-supplied
/// and unpriced. See [`legaia_engine_core::shop_catalog`].
fn read_shop_item_data(
    source: &SceneSource<'_>,
) -> Option<legaia_engine_core::shop_catalog::ShopItemData> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_engine_core::shop_catalog::ShopItemData::from_scus(&scus)
}

/// Read + parse the static item-effect descriptor table (`DAT_800752C0`, see
/// `item-effect-table.md`) from a boot source's `SCUS_942.54`. Returns `None`
/// when the executable isn't reachable or the table doesn't parse, so a boot
/// never fails on missing item-effect data - the engine then keeps the curated
/// usability flags on its item catalog.
fn read_retail_item_effects(
    source: &SceneSource<'_>,
) -> Option<legaia_asset::item_effect::ItemEffectTable> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::item_effect::ItemEffectTable::from_scus(&scus)
}

/// Read the static equipment stat-bonus table (`DAT_80074F68`, see
/// `equipment-table.md`) from a boot source's `SCUS_942.54` and build both the
/// disc-accurate equipment modifier table (stat bonuses keyed by real item ids)
/// and the per-item equip restrictions (character mask `+6` + slot category
/// `+7`) from the single parse. Returns `None` when the executable isn't
/// reachable or the table doesn't parse, so a boot never fails on missing
/// equipment data - the engine then falls back to the (fabricated-id) vanilla
/// equipment catalog.
fn read_retail_equip_tables(
    source: &SceneSource<'_>,
) -> Option<(
    legaia_engine_core::battle_stats::EquipmentTable,
    legaia_engine_core::equipment::DiscEquipInfo,
    legaia_asset::equip_stats::EquipStatTable,
)> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let table = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus)?;
    let modifiers = legaia_engine_core::equipment::equip_modifier_table_from_disc(&table);
    let mut restrictions = legaia_engine_core::equipment::DiscEquipInfo::from_disc(&table);
    // The Goods candidate lists are class-2 ids the equipment stat table does
    // not contain, so their index comes from the item-effect table instead -
    // without it the equip screen's three Goods rows browse an empty list.
    if let Some(effects) = legaia_asset::item_effect::ItemEffectTable::from_scus(&scus) {
        restrictions.install_goods(&effects);
    }
    // The raw records travel too: the Throw Out list builder reads each
    // record's `+7` flags byte, which neither derived table keeps.
    Some((modifiers, restrictions, table))
}

/// Read the player Seru-magic catalog (MP cost + target shape from the spell
/// table, see `spell-table.md`) from a boot source's `SCUS_942.54`. Returns
/// `None` when the executable isn't reachable or doesn't parse, so a boot falls
/// back to the pinned `retail_seru_magic_catalog`.
fn read_retail_spell_catalog(
    source: &SceneSource<'_>,
) -> Option<legaia_engine_core::spells::SpellCatalog> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
}

impl BootSession {
    /// Open an extracted disc tree and load the configured scene. Errors if
    /// the directory isn't an extracted PROT or the scene name isn't in
    /// CDNAME.TXT.
    pub fn open(extracted_root: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Extracted(extracted_root), cfg)
    }

    /// Open the engine straight from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT`; no on-disk extraction
    /// step is required. Native targets only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_disc(disc_bin: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Disc(disc_bin), cfg)
    }

    fn open_with_source(source: SceneSource<'_>, cfg: &BootConfig) -> Result<Self> {
        // Parse the new-game starting-party template from the same source
        // (best-effort; never fails the boot).
        let starting_party = read_starting_party(&source);
        let starting_inventory = read_starting_inventory(&source);
        let (equip_modifier_table, equip_restrictions, equip_stats) =
            match read_retail_equip_tables(&source) {
                Some((m, r, s)) => (Some(m), Some(r), Some(s)),
                None => (None, None, None),
            };
        let spell_catalog = read_retail_spell_catalog(&source);
        let steal_table = read_scus(&source)
            .and_then(|scus| legaia_asset::steal_table::StealTable::from_scus(&scus));
        let mut host = match source {
            SceneSource::Extracted(root) => SceneHost::open_extracted(root)
                .with_context(|| format!("open extracted dir {}", root.display()))?,
            #[cfg(not(target_arch = "wasm32"))]
            SceneSource::Disc(path) => SceneHost::open_disc(path)
                .with_context(|| format!("open disc image {}", path.display()))?,
        };
        // Wire the CDNAME-derived map-id resolver so field-VM scene
        // transitions resolve to the right CDNAME label.
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        // Free-roam liveliness (NPC patrol routes + the ambient walk
        // mirror) on, as both play hosts run it: it is retail behaviour, and
        // it never engages a placement's script (only a touch does), so the headless drivers - ladders,
        // oracles, the retail comparison corpus - see the world a player sees.
        host.world.npcs.animate = true;

        // Retail proportional dialog font off the disc (no save state). See
        // `BootSession::dialog_font`.
        let dialog_font = read_dialog_font(&host.index, &source);
        let escape_icons = read_scus(&source).and_then(|scus| {
            use legaia_font::escape_icons::{EscapeIcons, ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
            let head = host
                .index
                .prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN)
                .ok()?;
            EscapeIcons::from_disc(&head, &scus).ok()
        });
        if dialog_font.is_none() {
            log::warn!(
                "dialog font not decodable from the boot source; \
                 text falls back to extracted/ or the placeholder"
            );
        }

        // Hand the host the retail new-game defaults so a cold `--scene X`
        // boot (no New Game confirm, no save loaded) seeds the template party
        // + starting bag at scene entry instead of leaving a zeroed scaffold
        // roster behind the pause menu. Guarded inside `enter_field_scene` -
        // it never fires once a party or save is installed.
        host.new_game_defaults =
            starting_party
                .clone()
                .map(|party| legaia_engine_core::new_game::NewGameDefaults {
                    party,
                    inventory: starting_inventory.clone(),
                    // Retail's Vahn-alone roster by default: every headless
                    // harness cold-boots `town01` as the opening. The
                    // `play-window` scene picker raises it (`window/run.rs`).
                    picker_party: false,
                    equip_stats: equip_stats.clone(),
                });

        // The static-SCUS progression tables (XP curve + Noa/Gala correction
        // divisors, stat growth, victory pose, XA cue durations, magic-XP
        // thresholds, accessory passives) through the one engine install the
        // browser play page's `load_disc` calls too. Best-effort: absent on a
        // disc-free build, where each consumer keeps its default. Persists
        // across New Game.
        let scus = read_scus(&source);
        // Both halves of the battle chip / caption labels - the overlay half
        // (banner sentences, `Spirit`, `Escape`, the Ra-Seru names) and the
        // SCUS half (`Begin`, `Run`, `Attack`, ... and the sparring fight's
        // opening caption) - through the one builder the browser runtime's
        // `load_disc` calls too.
        host.world.battle.ui_strings = legaia_engine_core::battle_open::battle_ui_strings_for_disc(
            &host.index,
            scus.as_deref(),
        );
        // The party cast trigger's anim-pair lists and the monster casts'
        // opening camera shots, off the same battle-overlay image, for every
        // session - headless ones included, since a monster cast stages its
        // shot from them (the browser runtime's `load_disc` reads the same).
        host.world.battle.spell_anim_pairs =
            legaia_engine_core::battle_open::spell_anim_pairs_from_prot(&host.index);
        if let Some(scus) = scus {
            host.world.install_retail_progression_tables(&scus);
            // Pause-menu text: item names + info-window descriptions,
            // spell names / descriptions, accessory passive lines. The
            // Items / Magic pause screens resolve their strings here.
            host.world.install_menu_text(&scus);
            // Install the randomizer's seru-trade config (the `--seru-trade`
            // blob in preserved rodata). No-op / disabled on a vanilla disc;
            // when present, vendors offer seru-for-seru trades. Persists across
            // New Game.
            host.world.install_seru_trade_config(&scus);
        }

        // Install the gold-shop item data (per-id buy price + name mask) from the
        // SCUS item table, so each field scene's merchant offers its real stock
        // at real prices (populated per scene by `enter_field_scene`). Persists
        // across New Game; absent on disc-free builds (stock stays host-supplied).
        if let Some(shop_data) = read_shop_item_data(&source) {
            host.world.shops.item_shop_data = Some(shop_data);
        }

        // Install the real item-effect descriptor table so the item catalog's
        // field/battle usability gating matches retail (e.g. cure/revive items
        // are battle-only). Best-effort: absent on disc-free builds, where the
        // catalog keeps its curated usability flags.
        if let Some(effects) = read_retail_item_effects(&source) {
            host.world.set_item_effects(effects);
        }

        host.load_scene(&cfg.scene)
            .with_context(|| format!("load scene '{}'", cfg.scene))?;

        // Audio + BGM director (optional - disabled for headless tests).
        let (audio, bgm) = if cfg.enable_audio {
            match AudioOut::new() {
                Ok(audio) => {
                    // AudioOut owns a cpal::Stream which is Send but not Sync.
                    // BootSession is single-threaded (binary + WASM both
                    // tick on one thread); the Arc just gives the BGM
                    // director a refcounted handle.
                    #[allow(clippy::arc_with_non_send_sync)]
                    let audio = Arc::new(audio);
                    // No scene bank is staged: retail loads a bank only with
                    // its track, into the one BGM slot, and a scene-local id
                    // plays a global fallback track
                    // (`legaia_engine_core::scene::SCENE_LOCAL_BGM_FALLBACK_ID`).
                    let mut director = AudioBgmDirector::new(audio.clone());
                    // Decode the static SFX descriptor bank from the same
                    // executable once; it names the program/tone/voice-count
                    // for each cue id, and the VAB slot each cue's category
                    // routes to. Best-effort - an empty bank just no-ops
                    // resolved cues.
                    if let Some((sfx, slots)) = read_sfx_bank(&source) {
                        director.set_sfx_bank(sfx);
                        director.set_sfx_cue_slots(slots);
                    }
                    // Stage the pinned SFX program banks (slot 0 = PROT 0868,
                    // slot 2 = PROT 0869) into the shared SPU region so a cue
                    // resolves against the bank its own category names, not
                    // whatever BGM VAB is open. Best-effort.
                    if let Err(e) = stage_sfx_vab(&mut director, &host) {
                        log::warn!("resident SFX banks not staged: {e:#}");
                    }
                    // Demux + decode the arts-voice shout banks (XA2/XA4/XA6)
                    // and the SCUS cue tables. Disc-image boots only (channel
                    // demux needs the raw CD-XA subheaders). Best-effort - an
                    // absent bank leaves arts silent.
                    #[cfg(not(target_arch = "wasm32"))]
                    if let SceneSource::Disc(path) = &source {
                        match read_arts_shout_bank(path) {
                            Some(bank) => director.set_shout_bank(bank),
                            None => log::warn!("arts-voice shout bank not staged"),
                        }
                        // The battle's one-shot clips (`XA27` stings, `XA30`
                        // grunts) - the melee kernel's two sound sites.
                        match read_battle_xa_clip_bank(path) {
                            Some(bank) => director.set_xa_clip_bank(bank),
                            None => log::warn!("battle CD-XA clip bank not staged"),
                        }
                        // The cast voices (seventeen files) are not decoded
                        // here: the director stages one channel span from
                        // the disc the first time a cast names it.
                        let n = director.set_xa_lazy_source(path);
                        if n == 0 {
                            log::warn!("no XA<n>.XA files resolved; cast voices stay silent");
                        }
                    }
                    (Some(audio), Some(director))
                }
                Err(e) => {
                    log::warn!("audio disabled - open failed: {e:#}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            host,
            camera: Camera::default(),
            audio,
            bgm,
            frames: 0,
            starting_party,
            starting_inventory,
            equip_modifier_table,
            equip_restrictions,
            equip_stats,
            spell_catalog,
            steal_table,
            dialog_font,
            escape_icons,
            field_menu: None,
            field_menu_sub: None,
            options_state: OptionsState::default(),
            save_rack: SaveRack::Blocks(Vec::new()),
            save_port_blocks: Vec::new(),
            save_flow: SaveScreenFlow::new(),
            last_save_commit: None,
            spell_level_notice: None,
            art_learned_notice: None,
            field_menu_resume: SceneMode::Field,
            mode_seat: legaia_engine_core::mode::ModeSeat::new_at_boot(),
            host_drains_queues: false,
            queue_marks: HostQueueMarks::default(),
        })
    }

    /// Declare that the caller drains the world's per-tick presentation
    /// queues itself after every [`Self::tick`] (the play window does, routing
    /// each into its renderer and audio). Off by default: the session then
    /// drops whatever the previous tick left unconsumed before the next one
    /// runs ([`HostQueueMarks`]), so a driver that only ticks does not grow
    /// the queues without bound.
    pub fn set_host_drains_queues(&mut self, on: bool) {
        self.host_drains_queues = on;
        self.queue_marks = HostQueueMarks::default();
    }

    /// The fog pool's render step the play hosts run from their draw pass
    /// (`World::fog_render_step` through the field follow frame), for a
    /// headless driver that renders nothing but must keep the world in step
    /// with a host that does.
    ///
    /// The step is not presentation-only: it ages the pool and writes the
    /// live count (`_DAT_8007BCA8`) and the depth view the ambient emitter's
    /// fog spawner reads on the next tick (`FogPool::spawn`'s cap and
    /// overworld depth test), and each spawn that passes draws the world
    /// `rand()` stream. A driver that skips it runs a fogged scene on a
    /// different stream from the window. Same guard as the hosts: nothing
    /// runs outside game mode 3 or while the script gate is clear. The
    /// centre is only the frame resolver's no-camera fallback.
    pub fn fog_render_tick(&mut self) {
        let world = &mut self.host.world;
        if !legaia_engine_core::world::World::fog_mode(world.mode) || !world.fog.gate {
            return;
        }
        let frame = legaia_engine_core::camera_view::resolve_field_camera(
            world,
            &self.camera,
            None,
            [0.0, 0.0],
        );
        if let Some(view) = frame.field_view() {
            let _ = world.fog_render_step(&view);
        }
    }

    /// Begin a New Game: clear the world to a fresh slate
    /// ([`legaia_engine_core::world::World::begin_new_game`]) and seed the
    /// starting party (Vahn) from the boot source's `SCUS_942.54` template.
    ///
    /// Mirrors the retail NEW GAME → field-launch chain (master mode 2 → 3,
    /// see `docs/subsystems/boot.md`). The opening scene
    /// ([`legaia_asset::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`, the
    /// prologue cutscene, which hands off to `town01`) is entered through the
    /// usual [`BootSession::enter_field_live`] path; this call only resets and
    /// seeds the world state. When the SCUS template isn't available the world
    /// keeps its default scaffold party so the slice stays runnable.
    pub fn begin_new_game(&mut self) {
        self.host.world.begin_new_game_seeded(
            self.starting_party.as_ref(),
            self.starting_inventory.as_ref(),
        );
    }

    /// Start a New Game end to end: the seeded slate
    /// ([`Self::begin_new_game`]), the title theme stopped so the prologue's
    /// own BGM (or its scripted silence) owns the audio, then the opening
    /// scene through the shared order
    /// ([`legaia_engine_core::resume::enter_new_game`]: the prologue cutscene
    /// `opdeene`, else `town01`). Returns the scene entered, or `None` when
    /// neither would enter (the world keeps its seeded slate on whatever
    /// scene was running).
    ///
    /// The browser play page's `play_new_game` is the paired entry: both
    /// hosts' title New Game and post-wipe New Game go through one of the two.
    pub fn start_new_game(&mut self, opts: &FieldLiveOpts) -> Option<&'static str> {
        self.begin_new_game();
        if let Some(bgm) = self.bgm.as_mut() {
            bgm.stop();
        }
        legaia_engine_core::resume::enter_new_game(|scene| {
            self.enter_field_live(scene, opts)?;
            self.confirm_scene_landed(scene)
        })
    }

    /// `Ok` when the host's loaded scene is `scene`. [`Self::enter_field_live`]
    /// logs a failed scene entry and returns `Ok` anyway (its oracle and
    /// headless callers keep running on the old scene by design), so the
    /// resume / New Game fallbacks test the landing itself - without this a
    /// save naming a scene that would not load "entered" it and skipped the
    /// fallback to the running scene.
    fn confirm_scene_landed(&self, scene: &str) -> Result<()> {
        match self.host.scene.as_ref() {
            Some(s) if s.name == scene => Ok(()),
            other => anyhow::bail!(
                "scene '{scene}' did not load (still on {:?})",
                other.map(|s| s.name.as_str())
            ),
        }
    }

    /// Open the in-field pause menu (the retail Start-press path into the
    /// CARD mode pair, `game_mode 0x17`). Builds a [`FieldMenuSession`]
    /// seeded with the world's money + play time - the same construction the
    /// windowed host uses - then remembers the current
    /// [`SceneMode`] and switches the world into [`SceneMode::Menu`], so
    /// field dispatch suspends while the menu owns the frame. Idempotent
    /// while a menu is already open.
    ///
    /// This is the **builder**, and it enforces only the refusal retail puts
    /// inside the controller itself (the engaged bit). A host's *Start edge*
    /// must additionally consult
    /// [`World::field_menu_open_allowed`](legaia_engine_core::world::World::field_menu_open_allowed),
    /// which adds the mode test, instead of spelling one out locally. That
    /// separation is deliberate: headless drivers and oracles legitimately
    /// build the session from any mode, but a pad route that hard-codes
    /// `SceneMode::Field` silently drops the three kingdom overworlds, and
    /// those are the only scenes where the Save row is legal at all.
    pub fn open_field_menu(&mut self) {
        if self.field_menu.is_some() {
            return;
        }
        // Start is inert while a dialogue engagement owns the player. Retail's
        // menu-open accept lives in `FUN_801D01B0`'s pre-movement header,
        // *after* the engaged-bit branch at `0x801D01F0` - so with
        // `player+0x10 & 0x80000` raised the pad never reaches it and no menu
        // (and no deny buzz) happens at all.
        // REF: FUN_801D01B0
        // A script's own menu press (a save point's `49 01`) comes from the
        // interaction the engagement belongs to, so it is not refused.
        if self.host.world.dialogue_owns_input() && !self.host.world.scripted_menu_open_pending() {
            return;
        }
        let world = &mut self.host.world;
        let mut session = FieldMenuSession::new();
        session.money = world.party.money.max(0) as u32;
        session.play_time_seconds = world.clock.play_time_seconds;
        // Sample the two row gates retail keeps as globals and reads at every
        // draw: the op-`0x49` entry context (`*_DAT_8007B450`, which blocks
        // Load) and the scene's save permission (`_DAT_8007B6A8`, seeded at
        // scene load from the MAN header bit, which blocks Save). Both are
        // scene-scoped and the menu suspends the field, so sampling once at
        // open is equivalent to retail's per-frame re-read.
        session.set_gate(FieldMenuGate {
            entry_context_kind: world.menu_entry_context_kind(),
            save_allowed: world.party.scene_save_allowed,
        });
        // Retail's driver picks the *starting* sub-screen off that same kind
        // byte, so a locked context opens on the notice panel rather than on
        // the root picker (`FUN_801DC6B4`, `0x801dc8d0..0x801dc8e4`).
        session.open_entry_screen();
        self.field_menu_resume = world.mode;
        world.mode = SceneMode::Menu;
        self.field_menu = Some(session);
        // Retail opens the menu by writing the mode word, not by calling the
        // menu: `CARD INIT` (22) stages the menu overlay and hands the word to
        // `CARD MODE` (23) at `0x80025974`, which is the mode every menu-open
        // capture holds. Going through the seat is what runs the mode-change
        // edge - in particular the pad-edge swallow, so the Start press that
        // opened the menu is not also delivered as the menu's first input.
        self.mode_seat.request_card_mode();
        let plan = self
            .mode_seat
            .enter(legaia_engine_core::mode::GameMode::CardInit, world);
        debug_assert!(
            plan.is_none(),
            "CARD INIT stages no overlay-A request in the port's model"
        );
    }

    /// Close the pause menu and restore the suspended scene mode (the mode
    /// the world ran when [`Self::open_field_menu`] fired). No-op when no
    /// menu is open.
    pub fn close_field_menu(&mut self) {
        if self.field_menu.take().is_some() {
            // The sub-session goes with it: a menu closed out from under an
            // open screen must not leave that screen holding the pad the next
            // time the menu opens.
            self.field_menu_sub = None;
            self.save_flow.reset();
            self.host.world.mode = self.field_menu_resume;
            // The word follows the world back out of `CARD MODE`; the seat
            // takes the edge (and the pad swallow) on the next frame, so the
            // confirm that closed the menu does not walk the player.
            self.mode_seat.adopt_scene_mode(self.field_menu_resume);
            // A scripted menu press (a save point's `49 01`, a `49 0D` ready
            // check) parks its op until the menu it opened closes; the close
            // resumes it once. Twin of `play_menu_close` on the browser host.
            self.host.world.release_menu_entry_context_park();
        }
    }

    /// Whether the in-field pause menu is open (the engine equivalent of
    /// retail `game_mode 0x17`; [`World::mode`](legaia_engine_core::world::World::mode)
    /// is [`SceneMode::Menu`] while `true`).
    pub fn field_menu_is_open(&self) -> bool {
        self.field_menu.is_some()
    }

    /// Install the rack the Load / Save rows build their sub-session against,
    /// plus the block list behind each of its ports.
    ///
    /// The rack's kind decides the flow ([`SaveRack::CardPorts`] = retail's
    /// two-stage pill-row -> block-grid screen), which is why it arrives as
    /// one value instead of a rack and a mode flag: a host cannot make that
    /// call differently from another host. `port_blocks[p]` answers the
    /// card-read beat for port `p`; ports past its end read as unmounted.
    pub fn set_save_rack(&mut self, rack: SaveRack, port_blocks: Vec<Vec<SlotSnapshot>>) {
        self.save_rack = rack;
        self.save_port_blocks = port_blocks;
        self.save_flow.reset();
    }

    /// The rack currently behind the Load / Save rows.
    pub fn save_rack(&self) -> &SaveRack {
        &self.save_rack
    }

    /// The two-stage card flow driving an open Save / Load sub-session -
    /// the block grid, its cursor, and which port was read.
    pub fn save_flow(&self) -> &SaveScreenFlow {
        &self.save_flow
    }

    /// Drive one frame of the open pause menu, both levels. Returns `true`
    /// when the root session reached an outcome and the caller should close.
    ///
    /// Retail's pause menu suspends the root list on confirm and hands the
    /// pad to the routed sub-screen; this is that second level. Before it
    /// existed here, a confirmed row was resumed on the spot, so a driver
    /// built on `set_pad` + [`Self::tick`] could move the root cursor and
    /// gate rows but could not enter Items / Equip / Save at all - while both
    /// shipped hosts implemented the stack privately. Keeping it on the
    /// session is what lets an oracle and a host walk the same screens.
    fn tick_field_menu(&mut self) -> bool {
        let pad = &self.host.world.input;
        // The edge word every menu surface in this subsystem reads. A held
        // mask is one event: `just_pressed` is `pad & !pad_prev`.
        let pressed = pad.pad() & !pad.pad_prev();

        if self.field_menu_sub.is_some() {
            self.tick_field_menu_sub(pressed);
        } else {
            // Root list. A confirm suspends it on the routed row; build that
            // row's sub-session and control moves there next frame.
            let menu = self.field_menu.as_mut().expect("field_menu is Some");
            if let Some(row) = tick_root_list(menu, pressed) {
                self.save_flow.reset();
                let world = &self.host.world;
                let chain_library = world.chain_library();
                self.field_menu_sub = Some(FieldMenuSubsession::build(
                    row,
                    world,
                    &self.options_state,
                    &self.save_rack,
                    &chain_library,
                    &world.tables.spell_catalog,
                    &world.tables.equipment_table,
                ));
            }
        }

        self.field_menu.as_ref().and_then(|m| m.outcome()).is_some()
    }

    /// Route one pad edge into the open sub-session and, when it finishes,
    /// drain its outcome into the world before resuming the root list -
    /// through the same engine steps both shipped hosts call
    /// ([`tick_open_subsession`], [`finish_subsession`]).
    fn tick_field_menu_sub(&mut self, pressed: u16) {
        let Some(active) = self.field_menu_sub.as_mut() else {
            return;
        };
        // The Save / Load rows run under the two-stage card flow: it
        // pre-empts the grid edges and resolves the card-read beat.
        let mut edge = pressed;
        if let FieldMenuSubsession::Save(s) = active {
            if let Some(port) = self.save_flow.pending_read(s) {
                let blocks = self
                    .save_port_blocks
                    .get(port as usize)
                    .cloned()
                    .unwrap_or_default();
                self.save_flow.install_blocks(port, blocks);
            }
            edge = self.save_flow.before_tick(s, pressed);
        }
        // No key table here: a headless driver has no bindings to rebind.
        let _ = tick_open_subsession(active, edge, None, &self.host.world);
        if !self
            .field_menu_sub
            .as_ref()
            .is_some_and(FieldMenuSubsession::is_done)
        {
            return;
        }
        let finished = self.field_menu_sub.take().expect("sub was Some");
        let done = finish_subsession(finished, &mut self.host.world);
        // A renderer-less driver just latches the notices.
        if done.spell_level_notice.is_some() {
            self.spell_level_notice = done.spell_level_notice;
        }
        if done.art_learned_notice.is_some() {
            self.art_learned_notice = done.art_learned_notice;
        }
        match done.handoff {
            SubsessionHandoff::Applied => {}
            // The outcome names the card port, the grid names the block.
            // Persisting it is the host's: `BootSession` has no save
            // backend, so the pick is latched for the caller.
            SubsessionHandoff::Save(s) => self.last_save_commit = self.save_flow.commit(&s),
            SubsessionHandoff::Options(state) => self.options_state = state,
        }
        if let Some(menu) = self.field_menu.as_mut() {
            let _ = menu.resume(false);
        }
    }

    /// Start a **global-pool** `music_01` track (`bgm_id >= 2000`) through the
    /// BGM director: resolve the bank entry, upload its own VAB, and play its
    /// SEQ. This is how a minigame (or any caller with a disc-pinned track id)
    /// starts music that doesn't live in the current scene's sound bank -
    /// the dance overlay's chart loops, the Baka Fighter overture, the Muscle
    /// Dome battle theme. Returns `false` when audio is off, the id isn't a
    /// bank slot, or the entry doesn't decode. The slot machine + fishing
    /// deliberately don't call this: retail inherits the host scene's BGM.
    pub fn start_global_bgm(&mut self, bgm_id: u16) -> bool {
        let Ok(Some(entry)) = self.host.music_bank_entry_bytes(bgm_id) else {
            return false;
        };
        let Some(bgm) = self.bgm.as_mut() else {
            return false;
        };
        bgm.start_owned_vab(bgm_id, &entry);
        true
    }

    /// Restart the field scene's BGM after a minigame that took over the
    /// director with its own global track (dance / Baka Fighter / Muscle
    /// Dome). Re-plays whatever op-`0x35` track the scene had running
    /// ([`legaia_engine_core::world::AudioState::current_bgm`](legaia_engine_core::world::AudioState::current_bgm)),
    /// re-uploading its VAB. No-op when the scene had no track or it isn't a
    /// global-pool id. The slot machine + fishing don't need this: they never
    /// replaced the director's bank.
    pub fn restore_field_bgm(&mut self) {
        if let Some(id) = self.host.world.audio.current_bgm {
            self.start_global_bgm(id);
        }
    }

    /// One per-frame step: tick the world, route field-VM camera + BGM
    /// events, advance the camera follow, return the [`SceneTickEvent`] for
    /// engines that want to react to scene transitions.
    /// Hand this tick's SFX ring producer calls (field-VM op `0x36` sub
    /// `0`/`4`, the ambient motion VM's op `0x09`) to the director's retail
    /// ring, and keep the director's field-side SFX sources - the scene's
    /// runtime descriptor rows and the side-band bank - in step with the
    /// world. The director's own per-frame [`AudioBgmDirector::tick_sfx_frame`]
    /// then plays whatever came due. With no audio the calls are dropped, as
    /// every other cue is.
    fn route_field_sfx(&mut self) {
        let ops = self.host.world.take_sfx_ring_ops();
        // The field's CD-XA one-shots (op `0x36`'s XA arm, the scripted-scene
        // voice leg) - drained every tick so none outlives its frame.
        let field_xa = self.host.world.drain_field_xa_cues();
        // The scene's CD-XA prestage list is for a host that decodes clips
        // asynchronously (the browser page stages it); this director reads a
        // clip's span synchronously on first use, so the list is dropped.
        let _ = self.host.world.drain_field_xa_prestage();
        let Some(bgm) = self.bgm.as_mut() else {
            return;
        };
        for xa in &field_xa {
            let fired = bgm.play_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
            log::debug!(
                "field XA clip slot {} ch {} dur {} -> {}",
                xa.clip,
                xa.channel,
                xa.duration_sectors,
                if fired { "playing" } else { "not staged" }
            );
        }
        let world = &self.host.world;
        // One `World::tick` is one vsync, and the director's scheduler ticks
        // once per `World::tick`, so the ring ages by the vsyncs one tick
        // spans (`display_frame_step`, always 1) - not by the game-tick
        // cadence `frame_step`, which retail applies once per *game tick* of
        // that many vsyncs. The two schedules are the same in wall time.
        bgm.apply_sfx_ring_ops(&ops, world.clock.display_frame_step.clamp(1, 255) as u8);
        let field_family = matches!(
            world.mode,
            legaia_engine_core::world::SceneMode::Field
                | legaia_engine_core::world::SceneMode::WorldMap
        );
        let side_band = field_family.then(|| world.side_band_bank()).flatten();
        let index = &self.host.index;
        // A slot-6 side-band bank is not a tail borrower: retail streams it
        // over the field bank in the shared region, and the residency below
        // carries it.
        let side_band = side_band.filter(|b| b.slot != 6);
        bgm.sync_field_sfx(
            world.runtime_sfx_bundle(),
            field_family,
            side_band,
            |entry| index.entry_bytes_extended(entry).ok(),
        );
        // The slot-2 / slot-6 region follows the mode: the field bank in the
        // field, the class-2 bank in battle, a minigame's own in its mode.
        let shared = self.host.world.sync_sfx_residency();
        bgm.sync_shared_region(shared, |entry| index.entry_bytes_extended(entry).ok());
        // The battle's two monster.snd banks (VAB slots 7 / 8).
        let monster_banks = self.host.world.battle_monster_sound_banks();
        bgm.sync_battle_monster_banks(&monster_banks, || {
            index
                .entry_bytes_extended(legaia_asset::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
                .ok()
        });
        bgm.stop_sfx_voices(&self.host.world.take_sfx_voice_stops());
        // A minigame's directly keyed voices (the slot machine's reel motor),
        // after the stops so a release and a re-key in one tick end keyed.
        for k in self.host.world.take_sfx_voice_keys() {
            let keyed = bgm.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                k.voice,
                k.vab_program_tone,
                k.note_and_fine,
                k.volume,
            ));
            log::debug!(
                "direct voice {:#04x} {:?} keyed: {keyed}",
                k.voice,
                k.vab_program_tone
            );
        }
    }

    /// The session-side half of a scene swap under the host: the camera
    /// globals reset and the SFX queue dropped (no bank is staged).
    /// A door (`SceneTickEvent::SceneEntered`) and the post-FMV hand-off
    /// ([`Self::apply_pending_fmv_handoff`]) both swap the scene, and only the
    /// first used to reach this - so a movie that handed off into a new scene
    /// kept the trigger scene's camera shot and VAB bank.
    fn after_scene_swap(&mut self) {
        // The camera globals' reset (`FUN_80025C24`) is not here: a door's
        // runs inside `frame_step::camera_after_world_tick`, the tick order
        // both hosts share, and the post-FMV hand-off runs it itself.
        if let Some(bgm) = self.bgm.as_mut() {
            // New scene -> drop any SFX cues queued against the previous
            // one. No bank is staged: retail's field init loads no scene
            // bank (its only bank loads are slot 6 and the ending arm), and
            // the BGM slot changes only with the track, so a track carried
            // across the door keeps its samples.
            bgm.clear_sfx();
            // Nothing is flushed here. Op-`0x35` sub-op 9 - the op a cutscene
            // changes music with - is a *start* behind a load barrier, not a
            // queue for the next scene; it plays the moment
            // `route_bgm_events` hands it over. Deferring it to this point is
            // what left every Biron Monastery cutscene silent and then
            // started its score over the next scene.
        }
    }

    /// Run retail's post-FMV control transfer
    /// ([`SceneHost::apply_pending_fmv_handoff`]) and, when it entered a new
    /// scene, the same session-side swap a door runs. Every engine-shell host
    /// calls this rather than the bare host kernel; the render-side rebuild
    /// stays with the caller, keyed on [`FmvHandoffOutcome::Entered`].
    ///
    /// [`FmvHandoffOutcome::Entered`]: legaia_engine_core::scene::FmvHandoffOutcome::Entered
    pub fn apply_pending_fmv_handoff(
        &mut self,
    ) -> Option<legaia_engine_core::scene::FmvHandoffOutcome> {
        let outcome = self.host.apply_pending_fmv_handoff()?;
        if matches!(
            outcome,
            legaia_engine_core::scene::FmvHandoffOutcome::Entered { .. }
        ) {
            // Field entry resets the camera globals (`FUN_80025C24`) and
            // kills any mover in flight, so the movie's trigger scene cannot
            // leak its shot into the next one - as on a door.
            self.camera.reset_globals_for_scene_entry();
            self.after_scene_swap();
        }
        Some(outcome)
    }

    /// The world-side half of the per-tick tail both play hosts run after the
    /// scene tick ([`legaia_engine_core::world::World::step_world_frame_tail`]
    /// plus [`legaia_engine_core::world::World::step_field_vram_effects`] over
    /// the scene's own VRAM image, as the browser page steps it). What the
    /// tail hands back for drawing is dropped - a headless session draws
    /// nothing - except the spawned move's sound cue, which goes to the
    /// director when one is attached, the way both hosts route it.
    fn run_world_frame_tail(&mut self) {
        // The summon spawn request's seat: both play hosts bind the creature
        // mesh and seat it here, and a directed module's walk arm reads the
        // seat, so a headless run seats it unrendered.
        self.host.world.seat_summon_creature_unrendered();
        let tail = self.host.world.step_world_frame_tail(None, None, |_| None);
        if let (Some(cue), Some(bgm)) = (tail.move_fx_cue, self.bgm.as_mut())
            && let legaia_engine_audio::CueDispatch::Ring { ring_value, .. } =
                legaia_engine_audio::classify_cue(u32::from(cue))
        {
            bgm.enqueue_sfx(ring_value, 0, 0, 0);
        }
        if let Some(res) = self.host.resources.as_mut() {
            let _ = self
                .host
                .world
                .step_field_vram_effects(&mut res.vram, false);
        }
    }

    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        // The hosts' per-tick queue duty, for a caller that does not perform
        // it: what the previous tick queued and nobody took is gone before
        // this one runs, as it is in both play hosts.
        if !self.host_drains_queues {
            std::mem::take(&mut self.queue_marks).drop_stale(&mut self.host.world);
        }
        // The naming prompt (field-VM op `0x49`, the opening's pc `0x02C6`) is
        // modal: the field is frozen under it and every pad edge drives the
        // entry SM. The edge is the one the caller's `set_pad` just made.
        // Before this arm only the two window hosts routed the prompt, so any
        // driver that ticks the session - every headless run - parked on it
        // forever. The native window still takes its own arm first (it also
        // skips its per-frame tail); both call the same kernel.
        let input = &self.host.world.input;
        let edge = input.pad() & !input.pad_prev();
        if self.host.world.step_name_entry_frame(edge) {
            return Ok(SceneTickEvent::Stepped);
        }
        // The mode table's outer level, once per frame, ahead of everything
        // else - retail's `main` (`FUN_80015E90`, `0x8001615C..0x8001620C`)
        // takes any pending mode-change edge before it dispatches the new
        // mode's handler. The edge is not bookkeeping: it swallows the pad
        // edges (`gp+0x538` / `gp+0x55C`) so the button that caused the
        // transition is not re-delivered to the mode it opened.
        let mode_frame = self.mode_seat.frame(&mut self.host.world);
        if let Some(edge) = mode_frame.edge {
            log::debug!(
                "mode {:?} -> {:?} ({})",
                edge.from,
                edge.to,
                self.mode_seat.mode_name()
            );
        }
        // Pause menu (retail CARD pair, game_mode 0x17). Drive the Start edge
        // through the shared predicate rather than a local mode test: retail's
        // accept is a leg of the locomotion controller `FUN_801D01B0`, so the
        // menu opens wherever that controller walks the player - towns,
        // fields *and* the three kingdom overworlds, which are ordinary
        // `game_mode 0x03` field-run scenes in retail even though the port
        // models them as `SceneMode::WorldMap`. Spelling `SceneMode::Field`
        // here is what previously made the Save row - legal only on those
        // overworlds - unreachable by pad anywhere in the port.
        //
        // The windowed host never reaches this auto-path (it handles the
        // Start edge itself and skips `tick` while its boot-UI owns the
        // frame), so the two hosts can't double-drive the session.
        //
        // REF: FUN_801D01B0 (`0x801D0250`, the menu-open accept)
        // A script's op-`0x49` save point / ready check is a menu-button press
        // of its own (`World::scripted_menu_open_pending`): it opens the menu
        // with no Start edge and past the engagement gate the Start path
        // keeps, exactly once per arm.
        let scripted_menu =
            self.field_menu.is_none() && self.host.world.scripted_menu_open_pending();
        let menu_opened_this_tick = if scripted_menu
            || (self.field_menu.is_none()
                && self.host.world.field_menu_open_allowed()
                && self.host.world.input.just_pressed(PadButton::Start))
        {
            self.open_field_menu();
            if scripted_menu && self.field_menu.is_some() {
                self.host.world.note_scripted_menu_opened();
            }
            true
        } else {
            false
        };
        if !menu_opened_this_tick && self.field_menu.is_some() {
            let close = self.tick_field_menu();
            if close {
                self.close_field_menu();
            }
        }
        // The camera's half before the world tick (shared with the browser
        // page, `frame_step::camera_before_world_tick`): snap a free-roam
        // camera back to the follow default - a cutscene's op-0x45 events
        // leave it Cinematic at the shot's yaw, and the stale yaw would rotate
        // the d-pad remap ~180deg off the on-screen camera - then publish the
        // compass azimuth (scripted yaw + the user's drag-orbit + the host
        // framing bias) the field controller reads THIS tick.
        legaia_engine_core::frame_step::camera_before_world_tick(
            &mut self.camera,
            &mut self.host.world,
            None,
        );
        let event = self.host.tick()?;
        if let Some(bgm) = self.bgm.as_mut() {
            // SceneHost::route_bgm_events drains the world's pending BGM
            // events and dispatches into the director.
            let _ = self.host.route_bgm_events(bgm)?;
        }
        // The camera's half after it: route this tick's op-0x45 events,
        // advance the globals, and reset them on a scene entry
        // (`frame_step::camera_after_world_tick`).
        let scene_entered = matches!(event, SceneTickEvent::SceneEntered { .. });
        legaia_engine_core::frame_step::camera_after_world_tick(
            &mut self.camera,
            &mut self.host.world,
            scene_entered,
        );
        if scene_entered {
            self.after_scene_swap();
        }
        self.route_field_sfx();
        if !self.host_drains_queues {
            // The world-side half of the hosts' frame tail - effect
            // scene-graphs, move-FX / effect-script spawns, ANIMATE cues and
            // the scripted VRAM effects - which the play window runs itself.
            self.run_world_frame_tail();
        }
        // Reconcile the word with wherever the scene sessions left the world.
        // The seat owns the word; the sessions own the scene, and this is the
        // one join between them (see `ModeSeat`'s "what owns what").
        self.mode_seat.adopt_world_mode(&self.host.world);
        if !self.host_drains_queues {
            // What this tick queued stays readable until the next one starts.
            self.queue_marks = HostQueueMarks::record(&self.host.world);
        }
        self.frames += 1;
        Ok(event)
    }

    /// Drop the world into a live field scene: run the scene's event-script
    /// record 0 (the init prologue) so the field VM actually ticks, install
    /// the per-scene encounter table, and arm the live gameplay loop per
    /// `opts`.
    ///
    /// [`BootSession::open`] only calls `load_scene`, which leaves the world
    /// in [`SceneMode::Title`] with no field events firing. This is the
    /// reusable core of the windowed host's `--live-loop` setup, shared so the
    /// v0.1 oracle and headless drivers reach Field/Battle the same way the
    /// window does.
    ///
    /// Soft-fails the same way the window does: a scene with no event script
    /// logs and continues (the world stays in whatever mode it was in).
    /// Returns the active [`SceneMode`] after the attempt.
    pub fn enter_field_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        // Retail reaches the field through the mode table, not through a call:
        // whoever wants the field stores `MAIN INIT` (2) - the title
        // dispatcher does it at `0x801DFC00` - and mode 2's handler
        // `FUN_80025B64` stages the field overlay and calls the per-scene
        // initializer `FUN_801D6704` before handing the word to `MAIN MODE`
        // at `0x80025E50`. The port replaces the overlay load with native
        // scene entry, so the INIT column here is the plan and the body
        // below is the staging it names.
        {
            use legaia_engine_core::mode::{GameMode, ModeInitPlan};
            let plan = self
                .mode_seat
                .enter(GameMode::MainInit, &mut self.host.world);
            debug_assert!(
                matches!(plan, Some(ModeInitPlan::Stage(st))
                    if st.overlay_entry == legaia_engine_vm::title_overlay::FIELD_SCENE_INIT_PC),
                "MAIN INIT's plan should name the per-scene initializer"
            );
        }
        match self.host.enter_field_scene(scene, 0) {
            Ok(()) => log::info!("entered field scene '{scene}' record 0 (field VM live)"),
            Err(e) => log::warn!(
                "enter_field_scene('{scene}', 0) failed ({e:#}); staying on the load_scene-only \
                 path (field VM will not tick)"
            ),
        }
        // A direct entry (dev warp, load from a save) is not a `SceneEntered`
        // tick event, so the camera-side reset that event drives must run
        // here: an interrupted cutscene's shot otherwise frames the new scene
        // (`Camera::reset_for_scene_entry`). The browser play page's
        // `enter_field` is the paired site.
        self.camera.reset_for_scene_entry();

        let world = &mut self.host.world;

        // Install the equipment / spell / item catalogs unconditionally so
        // every consumer - not just the battle loop - sees real data. The
        // field pause-menu (Equip / Magic / Items screens) reads these off
        // the world; before they were flag-gated and the menu fell back to
        // throwaway vanilla()/new() placeholders that ignored disc data.
        // Each prefers the disc-accurate real-id table and falls back to the
        // fabricated-id vanilla catalog on disc-free builds.
        world.set_equipment_table(self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table()
        }));
        if let Some(stats) = self.equip_stats.clone() {
            world.set_equip_stats(stats);
        }
        world.set_spell_catalog(
            self.spell_catalog
                .clone()
                .unwrap_or_else(legaia_engine_core::retail_magic::retail_seru_magic_catalog),
        );
        world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
        if let Some(steal) = self.steal_table.clone() {
            world.set_steal_table(steal);
        }

        // Scene label + encounter fallback + the loop / player-battle / BGM
        // arming are the browser host's business too, so they live in one
        // kernel both hosts call (`World::arm_live_loop`). Only the catalogs
        // above stay here - they are disc-derived on native.
        world.arm_live_loop(scene, &opts.to_live_loop_opts());

        self.restage_audio_for_direct_entry();
        Ok(self.host.world.mode)
    }

    /// The audio half of a **direct** scene entry (dev warp, prologue skip,
    /// save load) - the browser play page's `enter_field` makes the same
    /// three moves. A deliberate scene boot restages BGM from scratch: SFX
    /// cues queued against the old scene are dropped, the dedupe latch is
    /// cleared so the scene's own op-`0x35` start is honoured even when it
    /// names the track already playing (which re-stages that track's bank).
    /// A door does not come through here; [`Self::after_scene_swap`] keeps
    /// the latch so a carried track keeps its playhead. No-op until audio is
    /// up.
    fn restage_audio_for_direct_entry(&mut self) {
        if let Some(bgm) = self.bgm.as_mut() {
            bgm.clear_sfx();
            bgm.last_started = None;
        }
    }

    /// Enter a world-map scene live: load the scene's resources, route its
    /// region-keyed encounter table onto the overworld, install the player
    /// actor, and switch into [`SceneMode::WorldMap`].
    ///
    /// The window's `--world-map` flag used to call [`World::enter_world_map`]
    /// directly, which only installs the camera controller (a camera-only
    /// debug viewer). This is the playable counterpart to [`Self::enter_field_live`]:
    /// it loads the scene through [`SceneHost::enter_field_scene`] (which seeds
    /// the formation table + monster catalog from the MAN, so overworld
    /// encounters resolve to real monsters), builds the
    /// [`RegionEncounterTable`](legaia_engine_core::region_encounter::RegionEncounterTable)
    /// from the same MAN, routes it via
    /// [`World::set_world_map_regions`], installs the field player so
    /// `tick_world_map`'s locomotion + per-tile encounter roll run, and enters
    /// world-map mode with the live loop armed.
    ///
    /// Soft-fails like [`Self::enter_field_live`]: a scene that fails to load
    /// logs and continues into world-map mode without a region table (camera
    /// only). Returns the active [`SceneMode`].
    pub fn enter_world_map_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        // The scene load + region routing + world-map mode now live in
        // `SceneHost::enter_world_map_scene`, so the natural boot/transition
        // path (`SceneHost::tick` auto-routing an overworld scene) and this
        // explicit `--world-map` entry seed the overworld identically. This
        // wrapper only layers the live-loop / battle options on top.
        // Same camera-side reset as the field entry above: an overworld
        // entered from the picker after an interrupted scene keeps no shot.
        self.camera.reset_for_scene_entry();
        match self.host.enter_world_map_scene(scene) {
            Ok(()) => log::info!("entered world-map scene '{scene}' (overworld seeded)"),
            Err(e) => {
                log::warn!(
                    "enter_world_map_scene('{scene}') failed ({e:#}); world map camera-only"
                );
                // Still switch into world-map mode so the window has a camera.
                self.host.world.set_active_scene_label(scene);
                self.host.world.enter_world_map();
            }
        }

        let equip_table = self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table()
        });
        let world = &mut self.host.world;
        world.set_equipment_table(equip_table);
        if opts.player_battle {
            world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
            world.set_spell_catalog(
                self.spell_catalog
                    .clone()
                    .unwrap_or_else(legaia_engine_core::retail_magic::retail_seru_magic_catalog),
            );
        }
        // The overworld always arms the loop (it is the encounter surface),
        // through the same shared kernel the field path uses.
        let mut live_opts = opts.to_live_loop_opts();
        live_opts.live_loop = true;
        world.arm_live_loop(scene, &live_opts);
        // A direct overworld entry (picker, save load) is a deliberate scene
        // boot like the field entry above, and gets the same audio restage;
        // the browser page's `enter_field` makes it for both kinds.
        self.restage_audio_for_direct_entry();
        Ok(self.host.world.mode)
    }

    /// Enter `scene` live through whichever entry the scene's own label
    /// names: an overworld label ([`legaia_engine_core::scene::is_world_map_scene`])
    /// through [`Self::enter_world_map_live`], anything else through
    /// [`Self::enter_field_live`]. The same one-predicate branch the browser
    /// play page's `enter_field` and the in-world door transition take.
    pub fn enter_scene_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        if legaia_engine_core::scene::is_world_map_scene(scene) {
            self.enter_world_map_live(scene, opts)
        } else {
            self.enter_field_live(scene, opts)
        }
    }

    /// Enter a scene live, then seed the world from a saved game.
    ///
    /// The entry goes through [`Self::enter_scene_live`], so a save written
    /// on a kingdom overworld (`mapNN` - where most saves are written)
    /// resumes in world-map mode. It used to call [`Self::enter_field_live`]
    /// unconditionally, which loaded the overworld as a plain field scene
    /// with no region table and no overworld controller, while the browser
    /// page's card Load routed the same label through the world-map entry.
    ///
    /// [`Self::enter_field_live`] cold-boots the scene at record 0 (a fresh
    /// party, no story progress). This variant runs that path and then
    /// hydrates the world from `save` via [`legaia_engine_core::World::load_full`]
    /// (party records, story flags, money, inventory) so the field VM sees the
    /// saved story state on its first tick. It is the building block for
    /// "continue a saved game" and for the story-gated paths that a cold boot
    /// into record 0 can't reach, such as a scripted-encounter trigger armed
    /// by story state.
    ///
    /// The save is applied *after* the scene is entered, so the scene record
    /// is still 0; selecting the story-appropriate record from the seeded
    /// flags is a separate concern (the field VM's record picker).
    ///
    /// To seed from a retail memory-card SC block, parse it first with
    /// [`legaia_save::SaveFile::from_retail_sc_block`].
    pub fn enter_field_live_from_save(
        &mut self,
        scene: &str,
        opts: &FieldLiveOpts,
        save: legaia_save::SaveFile,
    ) -> Result<SceneMode> {
        self.enter_scene_live(scene, opts)?;
        self.host.world.load_full(save);
        self.host.refresh_party_battle_inputs();
        log::info!("seeded world from save ({} party records)", {
            self.host.world.party.party_count
        });
        Ok(self.host.world.mode)
    }

    /// Resume a loaded save the way both hosts resume one,
    /// [`legaia_engine_core::resume::resume_card_load`]: seed the saved
    /// story flags, land it through
    /// [`legaia_engine_core::resume::land_save`] (the save's own scene, else
    /// the scene already running, else the opening town - never a New Game),
    /// entering scenes through [`Self::enter_scene_live`], then hydrate the
    /// whole save over the landing.
    ///
    /// `save_scene` is the save's resume label ([`legaia_save::SaveResume::scene`],
    /// empty for a file that carries none). The caller rebuilds its
    /// render-side scene state when [`ResumeLanding::entered_scene`] is set.
    /// The browser play page's `play_resume_save` is the paired entry.
    ///
    /// [`ResumeLanding::entered_scene`]: legaia_engine_core::resume::ResumeLanding::entered_scene
    pub fn resume_save(
        &mut self,
        save: legaia_save::SaveFile,
        save_scene: &str,
        opts: &FieldLiveOpts,
    ) -> legaia_engine_core::resume::ResumeLanding {
        // The order (story flags, landing, whole save) is the shared
        // kernel's - `resume_card_load`, which the browser page runs too.
        let landing = legaia_engine_core::resume::resume_card_load(
            &mut NativeCardLoad {
                session: self,
                opts,
            },
            save,
            save_scene,
        );
        log::info!(
            "resume: landed {} ({:?}); seeded world from save ({} party records)",
            landing.kind(),
            landing.scene(),
            self.host.world.party.party_count
        );
        landing
    }

    /// Where a save written now would resume
    /// ([`legaia_engine_core::scene::SceneHost::current_resume`], the one
    /// derivation the page's card and LGSF writers call too).
    pub fn current_resume(&self) -> legaia_save::SaveResume {
        self.host.current_resume()
    }

    /// Shut down the audio stream and clear the scene. Idempotent.
    pub fn shutdown(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.detach_sequencer();
        }
        self.bgm = None;
    }
}

impl Drop for BootSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The native window's half of a card load
/// ([`legaia_engine_core::resume::resume_card_load`]).
struct NativeCardLoad<'a> {
    session: &'a mut BootSession,
    opts: &'a FieldLiveOpts,
}

impl legaia_engine_core::resume::CardLoadHost for NativeCardLoad<'_> {
    fn card_load_world(&mut self) -> &mut legaia_engine_core::world::World {
        &mut self.session.host.world
    }

    fn card_load_running_scene(&self) -> Option<String> {
        self.session.host.scene.as_ref().map(|s| s.name.clone())
    }

    fn card_load_enter(
        &mut self,
        scene: &str,
        save: &legaia_save::SaveFile,
        save_scene: &str,
    ) -> Result<(), String> {
        // The saved scene is entered at the save's own position, as retail's
        // card load seats it (`SceneHost::arm_resume_seat`).
        let armed = self.session.host.arm_resume_seat(save, save_scene, scene);
        let entered = self
            .session
            .enter_scene_live(scene, self.opts)
            .and_then(|_| self.session.confirm_scene_landed(scene));
        if entered.is_err() && armed {
            self.session.host.disarm_entry_seat();
        }
        entered.map(|_| ()).map_err(|e| format!("{e:#}"))
    }

    fn card_load_hydrated(&mut self) {
        // The save's equipment prices the arts input, as retail's card load
        // (hydrate, then scene load) selects it.
        self.session.host.refresh_party_battle_inputs();
    }
}

/// Stage the reserved SFX region the way retail's SPU map lays it out:
/// slot `0` = PROT 0868 (the system bank the 16 category-`0` shared UI cues
/// key) resident from boot at the region's bottom, and above it the region
/// VAB slots `2` and `6` share (`FUN_800265E8` gives both `0x33010`).
///
/// The shared region is seeded with the class-2 bank (PROT 0869, slot 2) so a
/// cue fired before the first world tick has somewhere to sound; from then on
/// [`BootSession::route_field_sfx`] refills it with whatever the world's
/// residency names for the current mode - PROT 0876 in slot 6 in the field,
/// PROT 0869 in slot 2 in battle ([`AudioBgmDirector::sync_shared_region`]).
///
/// Each entry is a scene-VAB-style stream (`[u32 chunk header][VAB]...`), so
/// the VAB starts at `+4` (with a `+0` fallback for a bare bank).
fn stage_sfx_vab(director: &mut AudioBgmDirector, host: &SceneHost) -> Result<()> {
    use legaia_asset::sfx_table::SLOT0_SYSTEM_BANK_PROT_INDEX;
    use legaia_engine_core::world::SharedRegionBank;

    let bytes = host
        .index
        .entry_bytes_extended(SLOT0_SYSTEM_BANK_PROT_INDEX)
        .context("read the slot-0 system bank")?;
    // At the SFX region's floor, through the layout kernel the page shares;
    // the director keeps the bytes to re-stage the bank after a track that
    // overran the region (the ending theme's) lets it go.
    if !director.stage_resident_slot0(bytes) {
        anyhow::bail!("no VAB header at +4 or +0 in PROT 0868");
    }
    if !director.sync_shared_region(Some(SharedRegionBank::CLASS2), |e| {
        host.index.entry_bytes_extended(e).ok()
    }) {
        log::warn!("class-2 SFX bank not staged in the shared region");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_boot_config_uses_town01() {
        let c = BootConfig::default();
        assert_eq!(c.scene, "town01");
        assert!(c.enable_audio);
    }
}
