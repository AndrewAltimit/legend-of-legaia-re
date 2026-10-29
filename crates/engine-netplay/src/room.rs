//! [`NetPlay`]: one player's connection to a room, driven once per frame
//! by a play host.
//!
//! The controller owns the transport and everything the room has told this
//! player (the lobby, chat, who hosts), and moves between three phases:
//!
//! - **Lobby** - players roam their own worlds and see each other
//!   ([`crate::roam`]); the room screen picks a game, readies and starts.
//! - **Lockstep mode** - golf, racing, prop hunt: a [`LockstepSession`]
//!   ticks the world's mode seat from every seat's pad.
//! - **View-stream mode** - poker, Grid Duel: the host's [`ViewHost`] ticks
//!   the session, guests draw its frames ([`ViewGuest`]).
//!
//! A mode runs in the world's own mode seat ([`PackModeSeat::networked`]),
//! so it draws through the same screens as an offline one. Leaving it (or
//! a desync, or another player ending it) returns the room to its lobby.
//!
//! [`PackModeSeat::networked`]: legaia_engine_expansion::modes::PackModeSeat::networked

use legaia_engine_core::world::World;
use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{ModeEntry, ModeKind};
use legaia_engine_modes::session::{AiTier, SeatKind};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_netplay::lockstep::{LockstepSession, Step};
use legaia_netplay::protocol::{
    AvatarState, CloseReason, Hello, LobbyState, Message, PROTOCOL_VERSION, PackIdentity, PlayerId,
    RejectReason, RoomCode, RoomRequest, StartGame,
};
use legaia_netplay::transport::Transport;

use crate::mode_net::{ModeSim, NetMode, model_for};
use crate::roam::{NO_SCENE, Roam};
use crate::view_stream::{ViewGuest, ViewHost};

/// Pad seats a hosted room offers.
pub const ROOM_SEATS: u8 = 4;
/// Chat and notice lines kept.
const CHAT_LINES: usize = 32;
/// Lockstep frames a host frame may run to catch up after a stall.
const CATCH_UP: usize = 2;

/// The engine build id every peer in a room must share: the crate version,
/// which the native and browser builds of one release have in common.
pub fn engine_build() -> String {
    format!("legaia-engine/{}", env!("CARGO_PKG_VERSION"))
}

/// `pack`'s identity for the hello: its id and the SHA-256 of its
/// canonical bundle. `None` for no pack (retail content only).
pub fn pack_identity(pack: Option<&ExpansionPack>) -> Option<PackIdentity> {
    use sha2::{Digest, Sha256};
    let pack = pack?;
    let bundle = pack
        .to_files()
        .and_then(|f| legaia_expansion_format::bundle::write_bundle(&f))
        .unwrap_or_default();
    Some(PackIdentity {
        id: pack.manifest.id.clone(),
        content_hash: Sha256::digest(&bundle).into(),
    })
}

/// The quick-chat lines the room screen offers without a keyboard.
pub const QUICK_CHAT: &[&str] = &["Hi!", "Ready?", "Let's play", "One more?", "Good game"];

/// The emotes, as the chat line shows them ("Vahn waves").
pub const EMOTES: &[&str] = &["waves", "cheers", "bows", "dances", "laughs"];
/// The chat prefix an emote travels with.
const EMOTE_PREFIX: &str = "/me ";

/// A field scene's id as the avatars carry it: its first PROT entry, which
/// two players on one disc agree on.
pub fn scene_id(host: &SceneHost) -> u16 {
    host.scene
        .as_ref()
        .map_or(NO_SCENE, |s| u16::try_from(s.start).unwrap_or(NO_SCENE))
}

/// This player's avatar off `world`, in `scene`; [`NO_SCENE`] outside the
/// field.
pub fn own_avatar(world: &World, me: PlayerId, scene: u16) -> AvatarState {
    let hidden = AvatarState {
        player: me,
        scene: NO_SCENE,
        ..AvatarState::default()
    };
    if world.mode != SceneMode::Field {
        return hidden;
    }
    let Some(actor) = world
        .player_actor_slot
        .and_then(|s| world.actors.get(usize::from(s)))
    else {
        return hidden;
    };
    let m = &actor.move_state;
    let (anim, anim_frame) = match &world.locomotion.player_anim {
        Some(a) if a.walking => (1, a.walk.frame() as u16),
        Some(a) => (0, a.idle.frame() as u16),
        None => (0, 0),
    };
    AvatarState {
        player: me,
        scene,
        pos: [
            i32::from(m.world_x),
            i32::from(m.world_y),
            i32::from(m.world_z),
        ],
        facing: (m.render_26 as u16) & 0x0FFF,
        anim,
        anim_frame,
    }
}

/// A lockstep mode in play. The session's simulation holds a stand-in
/// between frames; the real session sits in the world's mode seat and is
/// swapped in around each advance, so the screens draw it.
struct LockstepRun {
    session: LockstepSession<ModeSim>,
}

enum Phase {
    /// Hello sent, no welcome yet.
    Joining,
    Lobby,
    Lockstep(Box<LockstepRun>),
    ViewHost(Box<ViewHost>),
    ViewGuest(Box<ViewGuest>),
}

/// The room screen's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Ready,
    Game,
    Start,
    Emote,
    QuickChat,
    Leave,
    Close,
}

/// One player's netplay controller.
pub struct NetPlay {
    link: Box<dyn Transport>,
    hello: Option<Hello>,
    pub me: Option<PlayerId>,
    pub room: Option<RoomCode>,
    pub lobby: Option<LobbyState>,
    /// Chat and room notices, newest last.
    pub chat: Vec<String>,
    /// The last error or notice, shown until the screen closes.
    pub notice: Option<String>,
    /// Where others join (set by a host that runs the relay).
    pub address: Option<String>,
    /// Hosting: the hello created the room.
    creating: bool,
    ready: bool,
    phase: Phase,
    roam: Roam,
    /// The room screen is open (it takes the pad).
    pub screen_open: bool,
    cursor: usize,
    game_pick: usize,
    emote_pick: usize,
    quick_pick: usize,
    /// A chat line being typed.
    pub draft: Option<String>,
    last_pad: u16,
    /// The connection is gone; the host should drop the controller.
    pub closed: bool,
    /// The scene id this frame (for drawing remote avatars).
    scene: u16,
}

impl NetPlay {
    fn new(link: Box<dyn Transport>, hello: Hello, creating: bool) -> Self {
        Self {
            link,
            hello: Some(hello),
            me: None,
            room: None,
            lobby: None,
            chat: Vec::new(),
            notice: None,
            address: None,
            creating,
            ready: false,
            phase: Phase::Joining,
            roam: Roam::new(),
            screen_open: true,
            cursor: 0,
            game_pick: 0,
            emote_pick: 0,
            quick_pick: 0,
            draft: None,
            last_pad: 0,
            closed: false,
            scene: NO_SCENE,
        }
    }

    /// Create a room over `link` (already connected to a relay).
    pub fn host(link: Box<dyn Transport>, name: &str, pack: Option<PackIdentity>) -> Self {
        let hello = Hello {
            protocol: PROTOCOL_VERSION,
            engine_build: engine_build(),
            player_name: name.to_string(),
            pack,
            room: RoomRequest::Create { seats: ROOM_SEATS },
        };
        Self::new(link, hello, true)
    }

    /// Join room `code` over `link`.
    pub fn join(
        link: Box<dyn Transport>,
        name: &str,
        code: RoomCode,
        pack: Option<PackIdentity>,
    ) -> Self {
        let hello = Hello {
            protocol: PROTOCOL_VERSION,
            engine_build: engine_build(),
            player_name: name.to_string(),
            pack,
            room: RoomRequest::Join(code),
        };
        Self::new(link, hello, false)
    }

    /// Whether this player hosts the room.
    pub fn is_host(&self) -> bool {
        match (&self.lobby, self.me) {
            (Some(l), Some(me)) => l.host == me,
            _ => self.creating,
        }
    }

    /// Whether a networked mode is in play.
    pub fn in_mode(&self) -> bool {
        matches!(
            self.phase,
            Phase::Lockstep(_) | Phase::ViewHost(_) | Phase::ViewGuest(_)
        )
    }

    /// Whether the controller has a lobby (welcomed, not in a mode).
    pub fn in_lobby(&self) -> bool {
        matches!(self.phase, Phase::Lobby)
    }

    /// The room line the screens title with.
    pub fn title(&self) -> String {
        match self.room {
            Some(code) => format!("Room {code}"),
            None => "Connecting...".to_string(),
        }
    }

    /// A player's display name.
    pub fn name_of(&self, id: PlayerId) -> String {
        self.lobby
            .as_ref()
            .and_then(|l| l.players.iter().find(|p| p.id == id))
            .map_or_else(|| id.to_string(), |p| p.name.clone())
    }

    fn note(&mut self, line: String) {
        self.chat.push(line);
        let excess = self.chat.len().saturating_sub(CHAT_LINES);
        self.chat.drain(..excess);
    }

    fn send(&mut self, msg: Message) {
        if self.link.send(&msg).is_err() {
            self.closed = true;
        }
    }

    /// Open or close the room screen (not while a mode is in play).
    pub fn toggle_screen(&mut self) {
        if self.in_mode() {
            return;
        }
        self.screen_open = !self.screen_open;
        self.draft = None;
        if !self.screen_open {
            self.notice = None;
        }
    }

    // --- chat ---

    /// Start typing a chat line.
    pub fn begin_chat(&mut self) {
        self.draft.get_or_insert_with(String::new);
    }

    /// Typed characters (printable ones only, up to the protocol's cap).
    pub fn type_chat(&mut self, text: &str) {
        if let Some(d) = self.draft.as_mut() {
            for c in text.chars().filter(|c| !c.is_control()) {
                if d.len() + c.len_utf8() <= legaia_netplay::protocol::MAX_CHAT_LEN {
                    d.push(c);
                }
            }
        }
    }

    pub fn chat_backspace(&mut self) {
        if let Some(d) = self.draft.as_mut() {
            d.pop();
        }
    }

    /// Send the typed line.
    pub fn submit_chat(&mut self) {
        if let Some(text) = self.draft.take()
            && !text.trim().is_empty()
        {
            self.say(&text);
        }
    }

    pub fn cancel_chat(&mut self) {
        self.draft = None;
    }

    /// Send one chat line now.
    pub fn say(&mut self, text: &str) {
        let me = self.me.unwrap_or_default();
        self.send(Message::Chat {
            from: me,
            text: text.to_string(),
        });
    }

    /// Send emote `i` of [`EMOTES`].
    pub fn emote(&mut self, i: usize) {
        if let Some(e) = EMOTES.get(i) {
            self.say(&format!("{EMOTE_PREFIX}{e}"));
        }
    }

    // --- the frame ---

    /// One frame, before the world's tick. `pad` is this frame's raw pad
    /// word; the return is the pad the world should see (zero while the room
    /// screen or a chat draft holds it).
    pub fn frame_host(&mut self, host: &mut SceneHost, pad: u16) -> u16 {
        let scene = scene_id(host);
        self.frame(&mut host.world, scene, pad)
    }

    /// [`Self::frame_host`] over a bare world standing in field scene
    /// `scene` (an id two players agree on; [`scene_id`] for a scene host).
    pub fn frame(&mut self, world: &mut World, scene: u16, pad: u16) -> u16 {
        if let Some(h) = self.hello.take() {
            self.send(Message::Hello(h));
        }
        self.scene = scene;
        loop {
            match self.link.try_recv() {
                Ok(Some(m)) => self.handle(world, m),
                Ok(None) => break,
                Err(_) => {
                    if !self.closed {
                        self.note("Connection lost".into());
                        self.notice = Some("Connection lost".into());
                    }
                    self.closed = true;
                    self.leave_mode(world, false);
                    break;
                }
            }
        }
        let pressed = pad & !self.last_pad;
        self.last_pad = pad;
        let owns_pad = (self.screen_open || self.draft.is_some()) && !self.in_mode();
        if owns_pad && self.draft.is_none() {
            self.screen_input(world, pressed);
        }
        if !self.closed {
            self.drive(world, pad);
        }
        if owns_pad { 0 } else { pad }
    }

    fn drive(&mut self, world: &mut World, pad: u16) {
        // A mode whose seat the player has left (or the world lost) is over
        // for this player; tell the room.
        if self.in_mode() {
            let seated = world.mode == SceneMode::Expansion
                && world
                    .expansion
                    .mode_seat
                    .as_ref()
                    .is_some_and(|s| s.networked);
            if !seated {
                self.send(Message::EndSession {
                    by: self.me.unwrap_or_default(),
                });
                self.phase = Phase::Lobby;
                return;
            }
        }
        let mut outbox = Vec::new();
        let mut desync = None;
        match &mut self.phase {
            Phase::Joining => {}
            Phase::Lobby => {
                if let Some(me) = self.me {
                    let own = own_avatar(world, me, self.scene);
                    let hosting = self.lobby.as_ref().is_some_and(|l| l.host == me);
                    outbox = self.roam.frame(own, hosting);
                }
            }
            Phase::Lockstep(run) => {
                let Some(seat) = world.expansion.mode_seat.as_deref_mut() else {
                    return;
                };
                if matches!(seat.stage, ModeStage::Results { .. }) {
                    return;
                }
                if run.session.wants_local_input() {
                    let local = seat.local_pad(pad);
                    run.session.push_local_input(local);
                }
                let Some(mode) = seat.active_mut() else {
                    return;
                };
                std::mem::swap(&mut run.session.sim_mut().mode, mode);
                for _ in 0..CATCH_UP {
                    match run.session.advance() {
                        Step::Advanced { .. } => {}
                        Step::Desynced(d) => {
                            desync = Some(d);
                            break;
                        }
                        _ => break,
                    }
                    if run.session.sim().mode.is_finished() {
                        break;
                    }
                }
                std::mem::swap(&mut run.session.sim_mut().mode, mode);
                let lines = std::mem::take(&mut run.session.sim_mut().log);
                seat.push_log(lines);
                outbox = run.session.drain_outbox();
            }
            Phase::ViewHost(vh) => {
                let Some(seat) = world.expansion.mode_seat.as_deref_mut() else {
                    return;
                };
                if matches!(seat.stage, ModeStage::Results { .. }) {
                    return;
                }
                let local = seat.local_pad(pad);
                let Some(mode) = seat.active_mut() else {
                    return;
                };
                let lines = vh.tick(mode, local);
                seat.push_log(lines);
                outbox = vh.drain_outbox();
            }
            Phase::ViewGuest(vg) => {
                if let Some(seat) = world.expansion.mode_seat.as_deref_mut() {
                    let local = seat.local_pad(pad);
                    vg.push_pad(local);
                }
                outbox = vg.drain_outbox();
            }
        }
        for m in outbox {
            self.send(m);
        }
        if let Some(d) = desync {
            let d: legaia_netplay::lockstep::DesyncReport = d;
            let text = format!(
                "Desync at frame {} (seat {}): back to the lobby",
                d.frame,
                d.remote_seat + 1
            );
            self.notice = Some(text.clone());
            self.note(text);
            self.leave_mode(world, true);
            self.screen_open = true;
        }
    }

    /// Close the world's networked seat and return to the lobby; with
    /// `announce`, tell the room the session is over.
    fn leave_mode(&mut self, world: &mut World, announce: bool) {
        if !self.in_mode() {
            return;
        }
        if world
            .expansion
            .mode_seat
            .as_ref()
            .is_some_and(|s| s.networked)
        {
            world.exit_pack_mode();
        }
        if announce {
            self.send(Message::EndSession {
                by: self.me.unwrap_or_default(),
            });
        }
        self.phase = Phase::Lobby;
    }

    fn handle(&mut self, world: &mut World, m: Message) {
        match m {
            Message::Welcome { room, you } => {
                self.me = Some(you);
                self.room = Some(room);
                self.phase = Phase::Lobby;
                self.note(format!("Joined room {room}"));
            }
            // Traffic that crossed a phase change (a roaming report landing
            // after a start, a second peer's end of a session already over)
            // is refused as unexpected; nothing the player did.
            Message::Reject(RejectReason::Unexpected) => {}
            Message::Reject(r) => {
                let text = reject_text(r).to_string();
                self.notice = Some(text.clone());
                self.note(text);
                if r.is_fatal() || matches!(self.phase, Phase::Joining) {
                    self.closed = r.is_fatal() || self.closed;
                }
            }
            Message::Lobby(l) => {
                if let Some(me) = self.me {
                    self.ready = l.players.iter().any(|p| p.id == me && p.ready);
                }
                self.lobby = Some(l);
            }
            Message::Chat { from, text } => {
                let name = self.name_of(from);
                let line = match text.strip_prefix(EMOTE_PREFIX) {
                    Some(action) => format!("* {name} {action}"),
                    None => format!("{name}: {text}"),
                };
                self.note(line);
            }
            Message::PlayerLeft { player, .. } => {
                let name = self.name_of(player);
                self.note(format!("{name} left"));
                self.roam.forget(player);
                let dropped = match &mut self.phase {
                    Phase::ViewHost(vh) => vh.drop_player(player),
                    _ => None,
                };
                if let Some(seat) = dropped
                    && let Some(mode) = world
                        .expansion
                        .mode_seat
                        .as_deref_mut()
                        .and_then(|s| s.active_mut())
                {
                    mode.set_seat_kind(seat, SeatKind::Ai(AiTier::Regular));
                    self.note(format!("The AI takes seat {}", seat + 1));
                }
            }
            Message::HostChanged { host } => {
                let name = self.name_of(host);
                self.note(format!("{name} hosts now"));
            }
            Message::RoomClosed(reason) => {
                let text = match reason {
                    CloseReason::HostLeft => "The host left; the room is closed",
                    CloseReason::ServerShutdown => "The server shut down",
                };
                self.notice = Some(text.into());
                self.note(text.into());
                self.leave_mode(world, false);
                self.closed = true;
            }
            Message::Start(start) => self.start(world, start),
            Message::EndSession { by } => {
                if self.in_mode() {
                    let finished = world
                        .expansion
                        .mode_seat
                        .as_ref()
                        .is_some_and(|s| matches!(s.stage, ModeStage::Results { .. }));
                    if !finished {
                        let name = self.name_of(by);
                        self.note(format!("{name} ended the game"));
                        self.leave_mode(world, false);
                    }
                    self.phase = Phase::Lobby;
                }
            }
            Message::Snapshot { tick, avatars } => self.roam.receive_snapshot(tick, &avatars),
            Message::Action { from, payload } => match &mut self.phase {
                Phase::Lobby => {
                    self.roam.receive_report(from, &payload);
                }
                Phase::ViewHost(vh) => {
                    vh.receive_action(from, &payload);
                }
                _ => {}
            },
            Message::StateBlob { seq, bytes, .. } => {
                if let Phase::ViewGuest(vg) = &mut self.phase
                    && let Some(seat) = world.expansion.mode_seat.as_deref_mut()
                    && let Some(mode) = seat.active_mut()
                    && let Some(lines) = vg.receive(mode, seq, &bytes)
                {
                    seat.push_log(lines);
                }
            }
            Message::SeatController { seat, .. } => {
                if let Phase::Lockstep(run) = &mut self.phase {
                    run.session.receive(&m);
                    self.note(format!("The AI takes seat {}", seat + 1));
                }
            }
            Message::Input(_)
            | Message::Checkpoint { .. }
            | Message::Pause { .. }
            | Message::Resume { .. } => {
                if let Phase::Lockstep(run) = &mut self.phase {
                    run.session.receive(&m);
                }
            }
            Message::Ping { nonce } => self.send(Message::Pong { nonce }),
            _ => {}
        }
    }

    /// The room's start order: build the instance and seat it.
    fn start(&mut self, world: &mut World, start: StartGame) {
        let (Some(me), Some(host)) = (self.me, self.lobby.as_ref().map(|l| l.host)) else {
            return;
        };
        let (modes, cards) = world.pack_mode_sections();
        let net = match NetMode::build(modes, cards, &start, me, host) {
            Ok(n) => n,
            Err(e) => {
                let text = format!("Could not start {}: {e}", start.mode);
                self.notice = Some(text.clone());
                self.note(text);
                if me == host {
                    self.send(Message::EndSession { by: me });
                }
                return;
            }
        };
        let name = net.mode.name.clone();
        let phase = if let Some(cfg) = net.lockstep_config() {
            let session = LockstepSession::new(ModeSim::new(net.mode.clone()), cfg);
            Phase::Lockstep(Box::new(LockstepRun { session }))
        } else if me == host {
            Phase::ViewHost(Box::new(ViewHost::new(&net.seats, me)))
        } else if net.local_seat.is_some() {
            Phase::ViewGuest(Box::new(ViewGuest::new()))
        } else {
            self.note(format!("{name} started; the table is full, so you roam"));
            return;
        };
        self.note(format!("{name} started"));
        world.enter_networked_pack_mode(net.mode);
        self.phase = phase;
        self.screen_open = false;
        self.draft = None;
        self.notice = None;
    }

    /// Ask the room to start mode `entry` (host only; the relay checks).
    pub fn request_start(&mut self, world: &World, entry: &ModeEntry) {
        let seed = world.frame.rotate_left(29) ^ u64::from(world.rng_state) ^ 0x4C47_4E50;
        self.send(Message::Start(StartGame {
            seed,
            mode: entry.id.clone(),
            config: Vec::new(),
            model: model_for(entry.kind),
            seats: Vec::new(),
        }));
    }

    /// Mark this player ready (or not) to start.
    pub fn set_ready(&mut self, ready: bool) {
        self.send(Message::SetReady(ready));
    }

    /// Whether the room lists this player ready.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// The lockstep frame the running mode has simulated to, if one runs.
    pub fn mode_frame(&self) -> Option<u32> {
        match &self.phase {
            Phase::Lockstep(run) => Some(run.session.frame()),
            _ => None,
        }
    }

    /// The running lockstep mode's verified checkpoints (remote hashes that
    /// matched this peer's).
    pub fn verified_checkpoints(&self) -> u32 {
        match &self.phase {
            Phase::Lockstep(run) => run.session.verified_checkpoints(),
            _ => 0,
        }
    }

    /// Leave the room (the connection closes).
    pub fn leave(&mut self, world: &mut World) {
        self.leave_mode(world, false);
        self.send(Message::Leave);
        self.link.close();
        self.closed = true;
    }

    // --- the room screen ---

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.is_host() {
            rows.extend([Row::Game, Row::Start]);
        } else {
            rows.push(Row::Ready);
        }
        rows.extend([Row::Emote, Row::QuickChat, Row::Leave, Row::Close]);
        rows
    }

    fn screen_input(&mut self, world: &World, pressed: u16) {
        let hit = |b: PadButton| pressed & b.mask() != 0;
        let rows = self.rows();
        self.cursor = self.cursor.min(rows.len() - 1);
        if hit(PadButton::Circle) || hit(PadButton::Triangle) {
            self.toggle_screen();
            return;
        }
        if hit(PadButton::Up) {
            self.cursor = (self.cursor + rows.len() - 1) % rows.len();
        }
        if hit(PadButton::Down) {
            self.cursor = (self.cursor + 1) % rows.len();
        }
        let step: i32 = if hit(PadButton::Left) {
            -1
        } else if hit(PadButton::Right) {
            1
        } else {
            0
        };
        let cycle = |v: usize, n: usize| (v as i32 + step).rem_euclid(n.max(1) as i32) as usize;
        let entries = world.pack_mode_entries();
        match rows[self.cursor] {
            Row::Game => self.game_pick = cycle(self.game_pick, entries.len()),
            Row::Emote => self.emote_pick = cycle(self.emote_pick, EMOTES.len()),
            Row::QuickChat => self.quick_pick = cycle(self.quick_pick, QUICK_CHAT.len()),
            _ => {}
        }
        if !hit(PadButton::Cross) {
            return;
        }
        match rows[self.cursor] {
            Row::Ready => {
                let ready = !self.ready;
                self.send(Message::SetReady(ready));
            }
            Row::Game | Row::Start => {
                if let Some(e) = entries.get(self.game_pick.min(entries.len().saturating_sub(1))) {
                    self.request_start(world, e);
                }
            }
            Row::Emote => self.emote(self.emote_pick),
            Row::QuickChat => self.say(QUICK_CHAT[self.quick_pick]),
            Row::Leave => {
                self.send(Message::Leave);
                self.link.close();
                self.closed = true;
            }
            Row::Close => self.toggle_screen(),
        }
    }

    /// The room screen's player rows.
    pub fn player_rows(&self) -> Vec<legaia_engine_ui::expansion_modes::netplay::NetPlayerRow> {
        let Some(l) = &self.lobby else {
            return Vec::new();
        };
        l.players
            .iter()
            .map(
                |p| legaia_engine_ui::expansion_modes::netplay::NetPlayerRow {
                    name: p.name.clone(),
                    seat: p.seat,
                    ready: p.ready,
                    host: p.id == l.host,
                    you: Some(p.id) == self.me,
                },
            )
            .collect()
    }

    /// The room screen's menu rows, against `entries` (the world's mode
    /// list).
    pub fn menu_rows(
        &self,
        entries: &[ModeEntry],
    ) -> Vec<legaia_engine_ui::expansion_modes::netplay::NetMenuRow> {
        use legaia_engine_ui::expansion_modes::netplay::NetMenuRow;
        let everyone_ready = self.lobby.as_ref().is_some_and(|l| {
            l.players
                .iter()
                .all(|p| p.id == l.host || p.seat.is_none() || p.ready)
        });
        let row = |label: &str, value: Option<String>, enabled: bool| NetMenuRow {
            label: label.to_string(),
            value,
            enabled,
        };
        self.rows()
            .into_iter()
            .map(|r| match r {
                Row::Ready => row(
                    "Ready",
                    Some(if self.ready { "Yes" } else { "No" }.into()),
                    true,
                ),
                Row::Game => row(
                    "Game",
                    entries
                        .get(self.game_pick.min(entries.len().saturating_sub(1)))
                        .map(|e| e.name.clone()),
                    !entries.is_empty(),
                ),
                Row::Start => row(
                    "Start",
                    Some(if everyone_ready { "Go" } else { "Waiting" }.into()),
                    everyone_ready && !entries.is_empty(),
                ),
                Row::Emote => row("Emote", Some(EMOTES[self.emote_pick].into()), true),
                Row::QuickChat => row("Say", Some(QUICK_CHAT[self.quick_pick].into()), true),
                Row::Leave => row("Leave room", None, true),
                Row::Close => row("Close", None, true),
            })
            .collect()
    }

    /// The room screen's cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The other players' avatars in this player's scene, for drawing.
    pub fn remote_avatars(&self) -> Vec<AvatarState> {
        match (self.me, &self.phase) {
            (Some(me), Phase::Lobby) => self.roam.remote_avatars(me, self.scene),
            _ => Vec::new(),
        }
    }

    /// The mode kind in play, if any (for a host that labels it).
    pub fn mode_kind(&self, world: &World) -> Option<ModeKind> {
        if !self.in_mode() {
            return None;
        }
        world
            .expansion
            .mode_seat
            .as_ref()
            .and_then(|s| s.active())
            .map(|m| m.kind())
    }
}

fn reject_text(r: RejectReason) -> &'static str {
    match r {
        RejectReason::ProtocolMismatch { .. } => "The relay speaks another protocol version",
        RejectReason::BuildMismatch => "The host runs another engine build",
        RejectReason::PackMismatch => "The host runs another expansion pack",
        RejectReason::NoSuchRoom => "No room with that code",
        RejectReason::RoomFull => "The room is full",
        RejectReason::ServerFull => "The relay is full",
        RejectReason::InGame => "That room is in a game; try again after it",
        RejectReason::BadRequest => "The relay refused the request",
        RejectReason::RateLimited => "Too many messages; disconnected",
        RejectReason::Malformed => "The relay could not read a message",
        RejectReason::NotHost => "Only the host can do that",
        RejectReason::NotReady => "Everyone seated must be ready first",
        RejectReason::Unexpected => "That is not possible right now",
    }
}
