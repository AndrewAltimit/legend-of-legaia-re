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
//! Not sampled, on purpose: the play clock (each host advances it off its
//! own wall clock), anything a renderer owns, and the audio director (a
//! headless native session runs with audio off; the page builds a director
//! off wasm on first use).

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
    }
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
