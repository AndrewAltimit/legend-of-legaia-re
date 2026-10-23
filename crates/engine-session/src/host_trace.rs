//! One host-agnostic sample of a session's simulation state, for comparing
//! two hosts tick by tick.
//!
//! Both play hosts run a [`BootSession`] (the native window over the cpal
//! output, the browser page over its own sink), so a sample taken off the
//! session reads the same fields on either side. What a host wraps around
//! the session tick - its step order, its modal arms, the pad word it hands
//! the world - shows up here only through what it does to the world, which
//! is the point: two hosts fed one pad script must leave the same state.
//!
//! Each [`HostTraceFrame`] field is one **channel**. [`first_divergences`]
//! reports, per channel, the first frame two traces disagree on, so a
//! report reads "the camera parted at tick 12, the RNG at tick 40" rather
//! than one opaque mismatch. The cheap structured channels (positions, the
//! camera words, the RNG) are spelled out; the wide ones (flag banks, the
//! roster, the bag) are folded to a 64-bit digest, where only "equal or not"
//! is meaningful.
//!
//! The audio channel is the director's command record - what the host's
//! routing asked for, not what an output sounded - so it compares across
//! two different outputs; a session with no director reports `None`.
//!
//! Not sampled, on purpose: the play clock (each host advances it off its
//! own wall clock) and anything a renderer owns.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use legaia_engine_audio::AudioSink;

use crate::BootSession;

/// One active actor slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorTrace {
    /// Slot index in `World::actors`.
    pub slot: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// 12-bit heading (`render_26`).
    pub heading: u16,
}

/// One sample. See the module docs for what each channel covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTraceFrame {
    /// The session's frame counter after the sample's tick.
    pub frames: u64,
    /// Loaded scene label.
    pub scene: Option<String>,
    /// `World::mode`, as its debug name.
    pub mode: String,
    /// The retail mode word the session's seat holds (`_DAT_8007B83C`).
    pub seat: usize,
    /// Mode-change edges the seat has taken.
    pub seat_edges: u64,
    /// `World::frame`.
    pub world_frame: u64,
    /// The world `rand()` stream state.
    pub rng: u32,
    /// Field-VM program counter.
    pub field_pc: usize,
    /// Every active actor slot.
    pub actors: Vec<ActorTrace>,
    /// The field NPCs' live positions (`FieldNpcState::positions`), by
    /// placement slot: villagers are not `World::actors` slots.
    pub npcs: Vec<(u8, i16, i16)>,
    /// The retail camera globals: pitch / yaw / roll (12-bit), eye-space
    /// translation, stored focus.
    pub cam: ([i32; 3], [i32; 3], [i32; 3]),
    /// System flags, the story bitmap, the scratch flag word and the region
    /// mask.
    pub flags: u64,
    /// Roster records, active party, leader and money.
    pub party: u64,
    /// The item bag.
    pub bag: u64,
    /// The open dialog request's text id, whether an inline dialogue runner
    /// is live, and the placement slot an inline talk runs for.
    pub dialog: (Option<u16>, bool, Option<u8>),
    /// `World::battle_ctx` (the action SM's context).
    pub battle: u64,
    /// `World::field_ctx` (the field VM's context).
    pub field_ctx: u64,
    /// Whether the pause menu is open, and where its field wipe stands.
    pub menu: (bool, legaia_engine_system::pause_wipe::PauseWipe),
    /// The open menu-overlay screen (shop, prize exchange, inn), which each
    /// host holds beside the session rather than in it: whether it is open,
    /// its UI frame word, the shop's opening fade, its state byte and cursor,
    /// and the shop screen it shows. `None` from [`sample`], which sees only
    /// the session; [`sample_with_overlay`] fills it.
    pub overlay: Option<OverlayTrace>,
    /// The op-`0x49` merchant gate (`ShopState::shop_armed`), whether a shop
    /// is open (`shop_open`) and whether one is staged for the hosts' drain.
    pub shop: (bool, bool, bool),
    /// The naming prompt (`PartyState::name_entry`), as its debug form:
    /// cursor, entered glyphs and confirm state.
    pub naming: Option<u64>,
    /// What the scene's actors are bound to: which slots carry a model, the
    /// idle clip each plays and the pose it holds, the lead's locomotion
    /// clip (its slot, frame and walking flag), and every placed NPC's clip
    /// cursor. A host that binds these from its mesh upload rather than
    /// from the scene parts here (`scene_actors::bind_scene_actors`).
    pub bindings: u64,
    /// What a battle's stage build leaves on the world: the stage class
    /// (`battle.stage_outdoor`), the backdrop depth-cue weight it ramps, the
    /// fog's stage luminance cap and each actor's battle texture slot. A
    /// host that writes these from its battle mesh build parts here.
    pub battle_stage: u64,
    /// The audio director's command record
    /// ([`crate::audio_director::AudioCommandLog`]): how many commands the
    /// host's routing has issued and their digest. `None` with no director
    /// attached.
    pub audio: Option<(u64, u64)>,
    /// The switches a launch sets on the world - the native window's
    /// command-line switches, the page's settings: the behaviour toggles,
    /// the locomotion rules (terrain follow, leading-edge walls, precise
    /// movement, run default), solid and live NPCs, the frame grab and the
    /// camera distance. Two hosts at their defaults must agree here; a
    /// switch one host sets and the other ignores parts this channel.
    pub switches: u64,
}

/// The host-held menu-overlay screen's state ([`HostTraceFrame::overlay`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayTrace {
    pub open: bool,
    pub ui_frame: u32,
    pub fade: Option<u8>,
    pub state: u8,
    pub cursor: u8,
    pub phase: Option<legaia_engine_menus::shop::ShopScreenPhase>,
}

/// [`sample`], plus the host's menu-overlay screen.
pub fn sample_with_overlay<S: AudioSink>(
    session: &BootSession<S>,
    menu: &legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
) -> HostTraceFrame {
    let mut f = sample(session);
    f.overlay = Some(OverlayTrace {
        open: menu.is_open(),
        ui_frame: menu.ui_frame(),
        fade: menu.shop_fade_level(),
        state: menu.ctx_state(),
        cursor: menu.cursor(),
        phase: menu.shop_screen_phase(),
    });
    f
}

fn digest<T: Hash + ?Sized>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn debug_digest<T: std::fmt::Debug + ?Sized>(v: &T) -> u64 {
    digest(format!("{v:?}").as_str())
}

/// Sample `session` now.
pub fn sample<S: AudioSink>(session: &BootSession<S>) -> HostTraceFrame {
    let world = &session.host.world;
    let g = &session.camera.globals;
    let angles = g.angles();
    let actors = world
        .actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active)
        .map(|(i, a)| ActorTrace {
            slot: i as u8,
            x: i32::from(a.move_state.world_x),
            y: i32::from(a.move_state.world_y),
            z: i32::from(a.move_state.world_z),
            heading: (a.move_state.render_26 as u16) & 0xFFF,
        })
        .collect();
    let mut npcs: Vec<(u8, i16, i16)> = world
        .npcs
        .positions
        .iter()
        .map(|(&slot, &(x, z))| (slot, x, z))
        .collect();
    npcs.sort_unstable();
    let flags = {
        let f = &world.flags;
        digest(&(
            f.system_flags.as_slice(),
            f.story_flag_bits.as_slice(),
            f.story_flags,
            f.extra_flags,
        ))
    };
    let party = {
        let p = &world.party;
        debug_digest(&(&p.roster, &p.active_party, p.party_leader_slot, p.money))
    };
    HostTraceFrame {
        frames: session.frames,
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: format!("{:?}", world.mode),
        seat: session.mode_seat.game_mode().as_index(),
        seat_edges: session.mode_seat.edges(),
        world_frame: world.frame,
        rng: world.rng_state,
        field_pc: world.field_pc,
        actors,
        npcs,
        cam: (
            [angles[0] & 0xFFF, angles[1] & 0xFFF, angles[2] & 0xFFF],
            g.tr_eye(),
            g.focus_stored(),
        ),
        flags,
        party,
        bag: debug_digest(&world.party.inventory),
        dialog: (
            world.dialog.current.as_ref().map(|d| d.text_id),
            world.dialog.inline.is_some(),
            world.dialog.active_inline_slot,
        ),
        battle: debug_digest(&world.battle_ctx),
        field_ctx: debug_digest(&world.field_ctx),
        menu: (session.field_menu_is_open(), *session.pause_wipe()),
        overlay: None,
        shop: (
            world.shops.shop_armed,
            world.shops.shop_open,
            world.shops.pending_shop.is_some(),
        ),
        naming: world.party.name_entry.as_ref().map(debug_digest),
        bindings: bindings_digest(world),
        switches: {
            let t = &world.toggles;
            let l = &world.locomotion;
            debug_digest(&(
                (
                    t.entry_pulse_enabled,
                    t.volumetric_fog,
                    t.live_gameplay_loop,
                    t.smarter_monster_targeting,
                    t.use_vm_dialogue,
                    t.use_damage_finish,
                    t.battle_command,
                    t.battle_camera,
                    t.overworld_marker_overlay,
                    t.battle_pass_rand_draw,
                ),
                (
                    l.follow_terrain_height,
                    l.leading_edge_wall_probes,
                    l.precise_movement,
                    l.run_default,
                ),
                (world.npcs.solid, world.npcs.animate),
                crate::session_switches::GameplayTweaks::from_toggles(t),
                world.ambient.frame_grab.enabled,
                session.camera.distance,
            ))
        },
        audio: session
            .bgm
            .as_ref()
            .map(|d| (d.commands().count(), d.commands().digest())),
        battle_stage: {
            let tex: Vec<(usize, u8)> = world
                .actors
                .iter()
                .enumerate()
                .filter_map(|(i, a)| a.battle_tex_slot.map(|t| (i, t)))
                .collect();
            debug_digest(&(
                world.battle.stage_outdoor,
                world.battle_backdrop_cue().map(f32::to_bits),
                world.fog_volume.battle_luma.map(f32::to_bits),
                tex,
            ))
        },
    }
}

fn bindings_digest(world: &legaia_engine_core::world::World) -> u64 {
    let slots: Vec<String> = world
        .actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.tmd_binding.is_some() || a.active_animation.is_some())
        .map(|(i, a)| {
            format!(
                "{i}:{}:{}:{:?}",
                a.tmd_binding.is_some(),
                a.active_animation.is_some(),
                a.pose_frame
            )
        })
        .collect();
    let lead = world.locomotion.player_anim.as_ref().map(|p| {
        (
            p.walking,
            p.leader,
            p.retail_slot(),
            p.scene_record(),
            p.active_frame_count(),
            p.idle.cursor(),
            p.walk.cursor(),
        )
    });
    debug_digest(&(slots, lead, &world.npcs.clip_cursors))
}

/// The channel names [`first_divergences`] reports, in field order.
pub const CHANNELS: &[&str] = &[
    "scene",
    "mode",
    "seat",
    "seat_edges",
    "world_frame",
    "rng",
    "field_pc",
    "actors",
    "npcs",
    "cam",
    "flags",
    "party",
    "bag",
    "dialog",
    "battle",
    "field_ctx",
    "menu",
    "overlay",
    "shop",
    "naming",
    "bindings",
    "battle_stage",
    "audio",
    "switches",
];

fn channel_equal(a: &HostTraceFrame, b: &HostTraceFrame, ch: &str) -> bool {
    match ch {
        "scene" => a.scene == b.scene,
        "mode" => a.mode == b.mode,
        "seat" => a.seat == b.seat,
        "seat_edges" => a.seat_edges == b.seat_edges,
        "world_frame" => a.world_frame == b.world_frame,
        "rng" => a.rng == b.rng,
        "field_pc" => a.field_pc == b.field_pc,
        "actors" => a.actors == b.actors,
        "npcs" => a.npcs == b.npcs,
        "cam" => a.cam == b.cam,
        "flags" => a.flags == b.flags,
        "party" => a.party == b.party,
        "bag" => a.bag == b.bag,
        "dialog" => a.dialog == b.dialog,
        "battle" => a.battle == b.battle,
        "field_ctx" => a.field_ctx == b.field_ctx,
        "menu" => a.menu == b.menu,
        "overlay" => a.overlay == b.overlay,
        "shop" => a.shop == b.shop,
        "naming" => a.naming == b.naming,
        "bindings" => a.bindings == b.bindings,
        "battle_stage" => a.battle_stage == b.battle_stage,
        "audio" => a.audio == b.audio,
        "switches" => a.switches == b.switches,
        _ => true,
    }
}

/// Per channel, the index of the first sample the two traces disagree on
/// (`None` = equal over the common length).
pub fn first_divergences(
    a: &[HostTraceFrame],
    b: &[HostTraceFrame],
) -> Vec<(&'static str, Option<usize>)> {
    CHANNELS
        .iter()
        .map(|&ch| {
            let at = a.iter().zip(b).position(|(x, y)| !channel_equal(x, y, ch));
            (ch, at)
        })
        .collect()
}
