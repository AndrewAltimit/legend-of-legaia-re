//! Top-level engine boot session.
//!
//! Composes the per-crate primitives ([`legaia_engine_scene_host::scene_host::SceneHost`],
//! [`legaia_engine_field::camera::Camera`], the BGM director from
//! [`crate::audio_director::AudioDirector`]) into one struct the binary drives per
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

use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::AudioSink;
use legaia_engine_battle::magic_xp::SpellLevelNotice;
use legaia_engine_field::camera::Camera;
use legaia_engine_menu_runtime::field_menu_dispatch::{
    FieldMenuSubsession, SubsessionHandoff, finish_subsession, tick_open_subsession, tick_root_list,
};
use legaia_engine_menus::field_menu::{FieldMenuGate, FieldMenuSession};
use legaia_engine_menus::options::OptionsState;
use legaia_engine_menus::save_screen::{SaveCommit, SaveScreenFlow};
use legaia_engine_menus::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::DefaultMapIdResolver;
use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;

use crate::audio_director::AudioDirector;

mod construct;
mod disc_tables;
mod field_menu;
mod frame_steps;
mod frame_tick;
mod scene_entry;
mod tick_owner;

use disc_tables::*;
pub use disc_tables::{BATTLE_XA_CLIP_SLOTS, attach_escape_icons};
pub use frame_steps::{CUE_FOOTSTEP, step_npc_clip_players};
pub use scene_entry::{NewGameHost, start_new_game_on};
pub use tick_owner::{TickClaims, TickOwner};

/// What one menu-button press did ([`BootSession::press_field_menu`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenuPress {
    /// The pause menu opened. `scripted` = a script's op-`0x49` press, which
    /// plays no confirm blip (retail's cue `0x20` is the pad controller's,
    /// not the subsystem actor's).
    Opened { scripted: bool },
    /// The menu lock refused the press and queued the deny buzz.
    Denied,
    /// Nothing happened: no press, a menu already open, or a silent refusal.
    None,
}

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
pub const SPU_RAM_BYTES: u32 = 512 * 1024;
/// Byte offset reserved for voice-0 / scratchpad - banks are allocated
/// above this. Mirrors the asset-viewer SEQ playback path.
pub const SPU_RESERVED_BYTES: u32 = legaia_engine_audio::spu_layout::SPU_RESERVED_BYTES;
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
    /// Starting scene name (CDNAME label). Empty boots no scene.
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

/// Per-frame session bundle. The binary owns one of these and calls
/// [`tick`](Self::tick) every frame.
pub struct BootSession<S: AudioSink> {
    pub host: SceneHost,
    pub camera: Camera,
    pub audio: Option<Arc<S>>,
    pub bgm: Option<AudioDirector<S>>,
    /// Wall-clock frame counter, separate from `host.world.frame` (which
    /// includes pause-time skips when those land).
    pub frames: u64,
    /// New-game starting-party template parsed from the boot source's
    /// `SCUS_942.54`, if present. Used by [`BootSession::begin_new_game`] to
    /// seed a faithful starting roster; `None` when the executable couldn't be
    /// read (e.g. a raw PROT.DAT-only source), in which case New Game keeps the
    /// world's default scaffold party.
    pub starting_party: Option<legaia_game_tables::new_game::StartingParty>,
    /// New-game starting inventory decoded from the boot source's `SCUS_942.54`
    /// seed code (`FUN_80034A6C`), if present. Vanilla retail is Healing Leaf
    /// ×5; the starting-item randomizer rewrites it. Used by
    /// [`BootSession::begin_new_game`] to seed the opening bag faithfully.
    pub starting_inventory: Option<legaia_game_tables::new_game::StartingInventory>,
    /// Disc-accurate equipment modifier table keyed by real item ids, parsed
    /// from the boot source's `SCUS_942.54` ([`legaia_game_tables::equip_stats`]).
    /// Preferred over the fabricated-id vanilla catalog when installing the
    /// battle-stat equipment table; `None` on disc-free builds.
    pub equip_modifier_table: Option<legaia_engine_battle::battle_stats::EquipmentTable>,
    /// Disc-pinned equip restrictions (character mask `+6` + slot category
    /// `+7`) keyed by real item ids, parsed from the same equipment stat-bonus
    /// table ([`legaia_game_tables::equip_stats`]). Drives the equip screen's
    /// per-character item gate
    /// ([`legaia_engine_menus::equip_session::EquipSession::new_with_restrictions`]);
    /// `None` on disc-free builds.
    pub equip_restrictions: Option<legaia_engine_menus::equipment::DiscEquipInfo>,
    /// The **raw** equipment stat-bonus records (`DAT_80074F68`) behind the two
    /// derived tables above. The Items screen's Throw Out list reads each
    /// record's `+7` flags byte, which neither derived form keeps; `None` on
    /// disc-free builds, where no row reports the no-discard bit.
    pub equip_stats: Option<legaia_game_tables::equip_stats::EquipStatTable>,
    /// Player Seru-magic catalog with MP cost + target shape read from the
    /// boot source's `SCUS_942.54` spell table ([`legaia_game_tables::spell_names`]).
    /// Preferred over the pinned `retail_seru_magic_catalog` when installing the
    /// battle spell catalog, so a randomized / translated disc is honoured;
    /// `None` on disc-free builds.
    pub spell_catalog: Option<legaia_engine_battle::spells::SpellCatalog>,
    /// Static per-monster **steal** table (`DAT_80077828`, fields
    /// `[chance, item]`), parsed from the same `SCUS_942.54`
    /// ([`legaia_game_tables::steal_table`]). Installed on the world beside the
    /// equipment / spell / item catalogs so PROT 0941's Steal can resolve a
    /// monster-seat victim; `None` on disc-free builds, which leaves that leg
    /// drawing no roll at all.
    pub steal_table: Option<legaia_game_tables::steal_table::StealTable>,
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
    /// `_DAT_8007B83C` ([`legaia_engine_mode::mode::ModeSeat`]).
    ///
    /// [`Self::tick`] drives it once per frame, and the session's own
    /// transitions write it where retail's code stores the word - field entry
    /// through `MAIN INIT`, the pause menu through `CARD INIT`. Two things
    /// come out of it that nothing else in the port produces: the mode-change
    /// edge (which swallows the pad edge that caused the transition, the way
    /// `0x800161EC` / `0x800161F8` do) and a real `game_mode` for every frame
    /// the mode-trace oracle samples.
    pub mode_seat: legaia_engine_mode::mode::ModeSeat,
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
    /// The field's fade to black around the pause menu (`FUN_801ED308`'s
    /// `_DAT_8007B440`) - see [`legaia_engine_system::pause_wipe`].
    pause_wipe: legaia_engine_system::pause_wipe::PauseWipe,
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
    pub art_learned_notice: Option<legaia_engine_menus::pause_screens::ArtLearnedNotice>,
    /// Scene mode the world ran before the pause menu opened, restored by
    /// [`BootSession::close_field_menu`].
    field_menu_resume: SceneMode,
    /// `true` when the caller drains the world's per-tick presentation queues
    /// itself after every [`Self::tick`] - the play window does. Otherwise
    /// the session does it ([`HostQueueMarks`]).
    host_drains_queues: bool,
    /// Queue lengths the last full [`Self::tick`] left behind.
    queue_marks: HostQueueMarks,
    /// A compass azimuth the host's own camera input asks for this tick (the
    /// browser page's VR / drag yaw), consumed by the next [`Self::tick`]'s
    /// `camera_before_world_tick`. `None` publishes the camera's own.
    pub camera_azimuth_override: Option<u16>,
    /// `true` when the host opens and drives the pause menu itself (the
    /// browser page's menu is opened from its own input path), so
    /// [`Self::tick`] neither auto-opens [`Self::field_menu`] on Start nor
    /// ticks it.
    host_owns_pause_menu: bool,
    /// The footstep / ambient cadence (`FUN_80018db0`), stepped once per sim
    /// tick by [`Self::step_footstep_cadence`].
    footstep: legaia_engine_audio::footstep::FootstepCadence,
    /// Last tick's player XZ, for the movement magnitude the cadence reads.
    footstep_prev_pos: Option<(i32, i32)>,
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

impl<S: AudioSink> BootSession<S> {
    /// Shut down the audio stream and clear the scene. Idempotent.
    pub fn shutdown(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.detach_sequencer();
        }
        self.bgm = None;
    }
}

impl<S: AudioSink> Drop for BootSession<S> {
    fn drop(&mut self) {
        self.shutdown();
    }
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

    /// A PROT image with a handful of empty entries - enough for a
    /// [`SceneHost`] and nothing to load.
    fn synthetic_prot() -> Vec<u8> {
        const SECTOR: usize = 2048;
        let mut img = vec![0u8; 8 * SECTOR];
        img[4..8].copy_from_slice(&3u32.to_le_bytes());
        img[8..12].copy_from_slice(&1u32.to_le_bytes());
        for (i, l) in [1u32, 3, 5, 7, 8].iter().enumerate() {
            let o = 8 + (i + 2) * 4;
            img[o..o + 4].copy_from_slice(&l.to_le_bytes());
        }
        img
    }

    type HeadlessSession = BootSession<legaia_engine_audio::TestAudioSink>;

    /// A session over a host with no scene, no audio and no disc tables.
    fn headless_session() -> HeadlessSession {
        let host = SceneHost::from_prot_bytes(synthetic_prot(), None).expect("synthetic host");
        let cfg = BootConfig {
            scene: String::new(),
            enable_audio: false,
        };
        BootSession::from_host(host, None, &cfg, || {
            Ok(legaia_engine_audio::TestAudioSink::new(
                legaia_engine_audio::SPU_INTERNAL_RATE,
            ))
        })
        .expect("an empty scene name boots no scene")
    }

    /// The camera azimuth a host hands in is consumed by the next tick.
    /// The press rule every host routes through: no press does nothing, a
    /// Start edge opens exactly when the engine predicate allows it, a press
    /// with the menu already up is not a second open, and the close restores
    /// the suspended mode.
    #[test]
    fn a_menu_press_opens_once_and_only_where_allowed() {
        let mut s = headless_session();
        s.host.world.mode = SceneMode::Field;
        assert_eq!(s.press_field_menu(false), PauseMenuPress::None);
        assert!(!s.field_menu_is_open());
        assert!(s.host.world.field_menu_open_allowed());
        assert_eq!(
            s.press_field_menu(true),
            PauseMenuPress::Opened { scripted: false }
        );
        assert!(s.field_menu_is_open());
        assert_eq!(s.host.world.mode, SceneMode::Menu);
        assert_eq!(s.press_field_menu(true), PauseMenuPress::None);
        s.close_field_menu();
        assert_eq!(s.host.world.mode, SceneMode::Field);
        // Off the field-run modes the press is refused silently.
        s.host.world.mode = SceneMode::Battle;
        assert_eq!(s.press_field_menu(true), PauseMenuPress::None);
        assert!(!s.field_menu_is_open());
    }

    #[test]
    fn the_azimuth_override_lasts_one_tick() {
        let mut s = headless_session();
        s.camera_azimuth_override = Some(0x400);
        s.tick().expect("tick");
        assert_eq!(s.camera_azimuth_override, None);
    }

    /// The one tick order both play hosts share: a host screen first, the
    /// naming prompt before any pause press, a press refused while an
    /// overlay screen owns the pad, a suspending overlay taking the tick and
    /// the field's pad neutral under any overlay.
    #[test]
    fn the_tick_claim_hands_each_tick_to_one_owner_in_order() {
        let start = PadButton::Start.mask();
        let ready = TickClaims {
            pause_menu_ready: true,
            ..TickClaims::default()
        };
        let mut s = headless_session();
        s.host.world.mode = SceneMode::Field;
        // A host screen wins over a Start edge: nothing opens.
        let host = TickClaims {
            host_screen: true,
            ..ready
        };
        assert_eq!(s.claim_tick(host, start), TickOwner::HostScreen);
        assert!(!s.field_menu_is_open());
        // An overlay screen owns the pad: the press is not answered, and the
        // field ticks on a neutral pad (an inn) or not at all (a shop).
        let inn = TickClaims {
            overlay_open: true,
            ..ready
        };
        assert_eq!(s.claim_tick(inn, start), TickOwner::Field(true));
        let shop = TickClaims {
            overlay_suspends: true,
            ..inn
        };
        assert_eq!(s.claim_tick(shop, start), TickOwner::OverlayScreen);
        assert!(!s.field_menu_is_open());
        // A host that cannot draw the menu yet does not open it.
        assert_eq!(
            s.claim_tick(TickClaims::default(), start),
            TickOwner::Field(false)
        );
        // No owner and no press: the field, on the player's pad.
        assert_eq!(s.claim_tick(ready, 0), TickOwner::Field(false));
        // The press opens the menu and takes the tick; while the wipe runs
        // the field ticks under it (the world in `Menu`).
        assert_eq!(
            s.claim_tick(ready, start),
            TickOwner::PauseMenuOpened(false)
        );
        assert!(s.field_menu_is_open());
        assert!(!s.pause_wipe().menu_spawned());
        assert!(matches!(s.claim_tick(ready, 0), TickOwner::Field(_)));
        while !s.pause_wipe().menu_spawned() {
            s.step_pause_wipe();
        }
        assert_eq!(s.claim_tick(ready, 0), TickOwner::PauseMenuSpawned);
        s.close_field_menu();
        // The naming prompt is asked before the pause press.
        s.host.world.open_name_entry(0);
        assert!(s.host.world.name_entry_active());
        assert!(matches!(
            s.claim_tick(ready, start),
            TickOwner::NameEntry(_)
        ));
        assert!(!s.field_menu_is_open());
    }

    /// The session's tick drains the field CD-XA queues for every host: a
    /// cue never outlives its tick, with or without a director.
    #[test]
    fn the_tick_drains_the_field_xa_queues() {
        let mut s = headless_session();
        s.host.world.push_field_xa_cue(6, 4, 120);
        s.host
            .world
            .queue_xa_prestage([legaia_engine_system::sound_state::XaVoiceClip {
                clip: 6,
                channel: 4,
                duration_sectors: 120,
            }]);
        s.tick().expect("tick");
        assert!(s.host.world.drain_field_xa_cues().is_empty());
        assert!(s.host.world.drain_field_xa_prestage().is_empty());
    }
}
