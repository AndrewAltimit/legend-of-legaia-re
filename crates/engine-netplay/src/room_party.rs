//! [`NetPlay`]'s party-night driving ([`crate::party`]): starting a night
//! from the room screen, the host directing it between rounds and closing
//! each round, the guests following and verifying, and the prize each
//! machine grants its own player.

use super::*;
use crate::party::{
    PartyAction, PartyEvent, PartyLink, PartyOp, RESULT_HOLD, START_TIMEOUT, key, round_id,
};
use legaia_engine_modes::party::{PartyNight, PartyPhase, round_section};
use legaia_engine_modes::session::Placement;
use legaia_engine_ui::expansion_modes::party::{PartyCard, party_card, round_line};
use legaia_netplay::protocol::SimModel;

/// Frames between re-sending a round's start order the relay refused.
const START_RESEND: u32 = 60;

impl NetPlay {
    /// The party nights the world's mode section defines, as game entries
    /// (`party-night:<id>`, by the night's name), listed ahead of the single games.
    pub(super) fn night_entries(world: &World) -> Vec<ModeEntry> {
        let (modes, _) = world.pack_mode_sections();
        modes
            .party_nights
            .iter()
            .map(|n| ModeEntry {
                id: format!("{NIGHT_ENTRY_PREFIX}{}", n.id),
                name: if n.name.is_empty() {
                    n.id.clone()
                } else {
                    n.name.clone()
                },
                kind: n
                    .rounds
                    .first()
                    .and_then(|r| {
                        legaia_engine_modes::active::entries(modes)
                            .into_iter()
                            .find(|e| e.id == r.mode)
                    })
                    .map_or(ModeKind::Golf, |e| e.kind),
            })
            .collect()
    }

    /// The party night in progress (or just over).
    pub fn party(&self) -> Option<&PartyLink> {
        self.party.as_deref()
    }

    fn notice_and_note(&mut self, text: String) {
        self.notice = Some(text.clone());
        self.note(text);
    }

    /// Host: begin party night `id` with every seated room member, in seat
    /// order. Refused, with a notice, unless everyone seated is ready and
    /// the host's world stands on a free field.
    pub(super) fn begin_party(&mut self, world: &World, id: &str) {
        if !self.is_host() {
            return;
        }
        if self.party.as_ref().is_some_and(|p| !p.night.is_over()) {
            return self.notice_and_note("A party night is already on".into());
        }
        if !Self::world_free_to_start(world) {
            return self.notice_and_note("Close what is open first, then start".into());
        }
        let Some(lobby) = self.lobby.clone() else {
            return;
        };
        if !lobby
            .players
            .iter()
            .all(|p| p.id == lobby.host || p.seat.is_none() || p.ready)
        {
            return self.notice_and_note("Everyone seated must be ready first".into());
        }
        let mut seated: Vec<_> = lobby.players.iter().filter(|p| p.seat.is_some()).collect();
        seated.sort_by_key(|p| p.seat);
        let players: Vec<_> = seated
            .iter()
            .map(|p| (key(p.id), p.name.clone(), true))
            .collect();
        let (modes, _) = world.pack_mode_sections();
        let Some(def) = modes.party_night(id) else {
            return self.notice_and_note(format!("No party night `{id}`"));
        };
        let seed = world.frame.rotate_left(23) ^ u64::from(world.rng_state) ^ 0x5041_5254;
        let night = match PartyNight::new(def, modes, &players, seed) {
            Ok(n) => n,
            Err(e) => return self.notice_and_note(format!("Could not start the night: {e}")),
        };
        let mut link = PartyLink::new(night);
        link.synced = lobby.players.iter().map(|p| p.id).collect();
        let line = format!(
            "Party night: {} - {} rounds",
            link.night.name,
            link.night.total_rounds()
        );
        self.party = Some(Box::new(link));
        self.departed.clear();
        self.note(line);
        self.screen_open = false;
        self.notice = None;
        self.party_broadcast(PartyOp::Begin);
    }

    /// Host: send the night after `op` to the room.
    fn party_broadcast(&mut self, op: PartyOp) {
        let Some(p) = self.party.as_mut() else {
            return;
        };
        p.last_phase = p.night.phase_key();
        let hash_after = p.night.scoreboard_hash();
        let payload = PartyEvent {
            op,
            night: p.night.clone(),
        }
        .encode();
        self.event_seq += 1;
        self.send(Message::Event {
            seq: self.event_seq,
            seat: 0,
            payload,
            hash_after,
        });
    }

    /// A party event from the host.
    pub(super) fn party_event(&mut self, world: &World, payload: &[u8], hash: u64) {
        let _ = world;
        if self.is_host() {
            return;
        }
        let Some(ev) = PartyEvent::decode(payload) else {
            return;
        };
        let begin = matches!(ev.op, PartyOp::Begin);
        match self.party.as_mut() {
            Some(p) if !begin => {
                let before = p.night.history.len();
                p.receive(ev, hash);
                if p.night.history.len() > before {
                    let line = p
                        .night
                        .history
                        .last()
                        .map(|r| round_line(&p.night, r, p.night.history.len()));
                    if let Some(l) = line {
                        self.note(l);
                    }
                }
            }
            _ => {
                let line = format!("Party night: {}", ev.night.name);
                self.party = Some(Box::new(PartyLink::new(ev.night)));
                self.note(line);
                self.screen_open = false;
                self.notice = None;
            }
        }
    }

    /// Host: a guest's party action (the pick).
    pub(super) fn party_action(&mut self, from: PlayerId, payload: &[u8]) {
        if !self.is_host() {
            return;
        }
        let Some(PartyAction::Pick { step, confirm }) = PartyAction::decode(payload) else {
            return;
        };
        let Some(p) = self.party.as_mut() else {
            return;
        };
        if p.night.pick(key(from), step, confirm) {
            self.party_broadcast(PartyOp::Pick {
                key: key(from),
                step,
                confirm,
            });
        }
    }

    /// Whether a round of the night is in play on this machine.
    pub(super) fn party_round_live(&self) -> bool {
        self.party.as_ref().is_some_and(|p| p.round_live)
    }

    /// The night ends early (the room closed, the connection dropped).
    pub(super) fn party_end_early(&mut self) {
        if let Some(p) = self.party.as_mut() {
            p.round_live = false;
            p.night.end_early();
        }
    }

    /// Host: end the night in progress (the room screen's row).
    pub(super) fn end_night(&mut self) {
        if !self.is_host() {
            return;
        }
        if let Some(p) = self.party.as_mut() {
            p.night.end_early();
            self.note("The host ended the party night".into());
            self.party_broadcast(PartyOp::End);
        }
    }

    /// A start order for a party round: build the round from this peer's
    /// copy of the night and seat it.
    pub(super) fn start_party_round(
        &mut self,
        world: &mut World,
        start: StartGame,
        me: PlayerId,
        host: PlayerId,
    ) {
        let Some(p) = self.party.as_deref() else {
            self.note("A party round started; you watch the next one".into());
            return;
        };
        let Some(slot) = p.night.current_slot() else {
            self.note("A party round started out of step; waiting for the host".into());
            return;
        };
        let info = p.night.infos[slot].clone();
        let mut order = start.clone();
        order.mode.clone_from(&info.mode);
        if info.kind == ModeKind::CardDuel
            && let Some((a, b)) = p.night.current_match()
        {
            // A ladder player who has left is played by the AI.
            let present = |k: u32| {
                let id = PlayerId(k);
                let here = self
                    .lobby
                    .as_ref()
                    .is_some_and(|l| l.players.iter().any(|m| m.id == id));
                (here && !self.departed.contains(&id)).then_some(id)
            };
            order.seats = vec![present(a), present(b)];
        }
        let maps = world.pack_mode_maps();
        let (modes, cards) = world.pack_mode_sections();
        let built = round_section(modes, &p.night.rounds[slot], start.seats.len())
            .map_err(|e| e.to_string())
            .and_then(|section| {
                NetMode::build(&section, cards, &maps, &order, me, host).map_err(|e| e.to_string())
            });
        let mut net = match built {
            Ok(n) => n,
            Err(e) => {
                self.notice_and_note(format!("Could not start the round: {e}"));
                if me == host {
                    self.send(Message::EndSession { by: me });
                }
                return;
            }
        };
        self.name_seats(&mut net);
        let phase = if let Some(cfg) = net.lockstep_config() {
            let session = LockstepSession::new(ModeSim::new(net.mode.clone()), cfg);
            Phase::Lockstep(Box::new(LockstepRun { session }))
        } else if me == host {
            let mut vh = ViewHost::new(&net.seats, me);
            if let Some(l) = &self.lobby {
                for m in &l.players {
                    if m.id != me && !net.seats.contains(&Some(m.id)) {
                        vh.watch(m.id);
                    }
                }
            }
            Phase::ViewHost(Box::new(vh))
        } else if net.local_seat.is_some() {
            Phase::ViewGuest(Box::new(ViewGuest::new()))
        } else {
            Phase::ViewGuest(Box::new(ViewGuest::watcher()))
        };
        let round = self.party.as_ref().map_or(0, |p| p.night.round_number());
        let total = self.party.as_ref().map_or(0, |p| p.night.total_rounds());
        let matchup = self.party.as_ref().and_then(|p| {
            let (a, b) = p.night.current_match()?;
            let n = |k| {
                p.night
                    .player(k)
                    .map_or("?", |x| x.name.as_str())
                    .to_string()
            };
            Some(format!(" ({} vs {})", n(a), n(b)))
        });
        let matchup = matchup.unwrap_or_default();
        let line = match net.local_seat {
            Some(_) => format!(
                "Round {round}/{total}: {} - {}{matchup}",
                info.kind.label(),
                info.title
            ),
            None => format!("Round {round}/{total}: {}{matchup} - you watch", info.title),
        };
        if let Some(p) = self.party.as_mut() {
            p.round_seats = net.seats.clone();
            p.round_live = true;
            p.finished_for = 0;
            p.start_pending = None;
            p.start_sent = None;
        }
        self.note(line);
        world.enter_networked_pack_mode(net.mode);
        self.phase = phase;
        self.screen_open = false;
        self.draft = None;
        self.notice = None;
    }

    /// Someone ended the session of a live party round. The host records
    /// it when its seat holds the results, and voids it when not; everyone
    /// returns to the lobby.
    pub(super) fn party_session_ended(&mut self, world: &mut World, by: PlayerId) {
        if self.is_host() {
            if Self::seat_results(world).is_none() && by != self.me.unwrap_or_default() {
                let name = self.name_of(by);
                self.note(format!("{name} ended the round"));
            }
            self.party_close_round(world, false);
        } else {
            self.leave_mode(world, false);
            if let Some(p) = self.party.as_mut() {
                p.round_live = false;
            }
        }
        self.session_over();
    }

    /// The finished round's placements, from the world's networked seat.
    fn seat_results(world: &World) -> Option<Vec<Placement>> {
        let seat = world.expansion.mode_seat.as_deref()?;
        if !seat.networked {
            return None;
        }
        match &seat.stage {
            ModeStage::Results {
                rows,
                completed: true,
                ..
            } => Some(rows.clone()),
            _ => None,
        }
    }

    /// Host: close the live round - record its placements, or void it when
    /// its seat holds none - leave the mode (`announce`: end the session for
    /// the room) and send the night on.
    fn party_close_round(&mut self, world: &mut World, announce: bool) {
        let results = Self::seat_results(world);
        if !self.party_round_live() {
            return;
        }
        if let Some(p) = self.party.as_mut() {
            p.round_live = false;
        }
        // Back to the lobby first: the relay carries the host's events
        // there, and the night's next messages are events.
        self.leave_mode(world, announce);
        if world
            .expansion
            .mode_seat
            .as_ref()
            .is_some_and(|s| s.networked)
        {
            world.exit_pack_mode();
        }
        // A player who dropped during the round leaves the night before its
        // placements count: their seat was the AI's.
        self.party_mark_departed();
        let Some(p) = self.party.as_mut() else {
            return;
        };
        let duel = p
            .night
            .current_slot()
            .is_some_and(|s| p.night.infos[s].kind == ModeKind::CardDuel);
        let op = match results {
            None => {
                p.night.void_round();
                PartyOp::Void
            }
            Some(rows) => {
                let seats = p.round_seats.clone();
                let seat_key = |seat: usize| seats.get(seat).copied().flatten().map(key);
                if duel {
                    let firsts: Vec<_> = rows.iter().filter(|r| r.place == 1).collect();
                    let winner = match firsts.as_slice() {
                        [one] => seat_key(one.seat),
                        _ => None,
                    };
                    p.night.record_duel(winner);
                    PartyOp::Duel(winner)
                } else {
                    // Rank the night's players among themselves: an AI seat
                    // that beats them takes no place of theirs.
                    let mine: Vec<(u32, u8)> = rows
                        .iter()
                        .filter_map(|r| Some((seat_key(r.seat)?, r.place)))
                        .collect();
                    let ranked: Vec<(u32, u8)> = mine
                        .iter()
                        .map(|&(k, place)| {
                            let better = mine.iter().filter(|(_, p)| *p < place).count();
                            (k, better as u8 + 1)
                        })
                        .collect();
                    p.night.record(&ranked);
                    PartyOp::Recorded(ranked)
                }
            }
        };
        let line = p
            .night
            .history
            .last()
            .filter(|_| !duel || p.night.hill.is_none())
            .map(|r| round_line(&p.night, r, p.night.history.len()));
        if let Some(l) = line {
            self.note(l);
        }
        self.party_broadcast(op);
    }

    /// Host: every player the relay reported gone leaves the night, each
    /// told to the room.
    fn party_mark_departed(&mut self) {
        let gone: Vec<PlayerId> = std::mem::take(&mut self.departed);
        for id in gone {
            if self
                .party
                .as_ref()
                .is_some_and(|p| p.night.is_active(key(id)))
            {
                if let Some(p) = self.party.as_mut() {
                    p.night.mark_left(key(id));
                }
                self.party_broadcast(PartyOp::Left(key(id)));
            }
        }
    }

    /// Whether the party card holds the pad (the world sees none): between
    /// the rounds of a night in progress, and over a finished round's
    /// results.
    pub(super) fn party_holds_pad(&self) -> bool {
        let Some(p) = self.party.as_deref() else {
            return false;
        };
        if self.screen_open || self.draft.is_some() {
            return false;
        }
        !self.in_mode() && !p.night.is_over()
            || p.night.is_over() && !self.in_mode() && !self.closed
    }

    /// Whether the world's own pad is withheld this frame: under the party
    /// card, and through a party round (the round reads the pad through the
    /// room; with the quit prompt shut, no player can close a round, or its
    /// results, under the others - the host closes it).
    pub(super) fn party_world_blocked(&self) -> bool {
        self.party_holds_pad() || (self.party_round_live() && self.in_mode())
    }

    /// One frame of the night, after the room's: the host directs, a guest
    /// follows, the picker picks, and a completed night grants this
    /// machine's player their prize.
    pub(super) fn party_frame(&mut self, world: &mut World, pad: u16) {
        let Some(me) = self.me else {
            return;
        };
        let (in_mode, screen_open, hosting) = (self.in_mode(), self.screen_open, self.is_host());
        let Some(p) = self.party.as_mut() else {
            return;
        };
        let pressed = pad & !p.last_pad;
        p.last_pad = pad;
        let hit = |b: PadButton| pressed & b.mask() != 0;
        // A finished night's podium closes on a button.
        if p.night.is_over()
            && !in_mode
            && !screen_open
            && (hit(PadButton::Cross) || hit(PadButton::Circle))
        {
            self.party = None;
            return;
        }
        // The pick.
        if let PartyPhase::Pick { picker, .. } = p.night.phase
            && picker == key(me)
            && !screen_open
        {
            let step = i32::from(hit(PadButton::Down) || hit(PadButton::Right))
                - i32::from(hit(PadButton::Up) || hit(PadButton::Left));
            let confirm = hit(PadButton::Cross);
            if step != 0 || confirm {
                if hosting {
                    if p.night.pick(key(me), step, confirm) {
                        self.party_broadcast(PartyOp::Pick {
                            key: key(me),
                            step,
                            confirm,
                        });
                    }
                } else {
                    self.send(Message::Action {
                        from: me,
                        payload: PartyAction::Pick { step, confirm }.encode(),
                    });
                }
            }
        }
        if self.is_host() {
            self.party_host(world, me);
        } else {
            self.party_guest(me);
        }
        self.party_grant(world, me);
    }

    fn party_host(&mut self, world: &mut World, me: PlayerId) {
        let in_lobby = matches!(self.phase, Phase::Lobby);
        // Players the relay reported gone leave the night (told to the room
        // from the lobby, where the relay carries the host's events).
        if in_lobby && !self.party_round_live() {
            self.party_mark_departed();
        }
        // Late joiners: out of the seats, and a copy of the night.
        if in_lobby && let Some(l) = self.lobby.clone() {
            let mut sync = false;
            for m in &l.players {
                let Some(p) = self.party.as_mut() else { break };
                if m.id == me || p.synced.contains(&m.id) {
                    continue;
                }
                p.synced.push(m.id);
                sync = true;
                if m.seat.is_some() && !p.night.is_active(key(m.id)) {
                    self.send(Message::AssignSeat {
                        player: m.id,
                        seat: None,
                    });
                }
            }
            if sync {
                self.party_broadcast(PartyOp::Sync);
            }
        }
        let live = self.party_round_live();
        if live {
            // The round closes a beat after its results come up, or at once
            // when the mode is gone without them (a desync, a quit).
            let results = Self::seat_results(world).is_some();
            let in_mode = self.in_mode();
            let Some(p) = self.party.as_mut() else { return };
            if results {
                p.finished_for += 1;
                if p.finished_for >= RESULT_HOLD || !in_mode {
                    self.party_close_round(world, in_mode);
                }
            } else if !in_mode {
                self.party_close_round(world, false);
            }
            return;
        }
        if !in_lobby {
            return;
        }
        let ready = self.lobby.as_ref().is_some_and(|l| {
            l.players
                .iter()
                .all(|m| m.id == l.host || m.seat.is_none() || m.ready)
        });
        let Some(p) = self.party.as_mut() else { return };
        if p.night.is_over() {
            return;
        }
        if matches!(p.night.phase, PartyPhase::Playing { .. }) {
            // The round is cued: start it once every seated player is ready.
            let waited = p.start_pending.get_or_insert(0);
            *waited += 1;
            let waited = *waited;
            if waited > START_TIMEOUT {
                p.start_pending = None;
                p.start_sent = None;
                p.night.void_round();
                self.note("The round could not start; it is void".into());
                self.party_broadcast(PartyOp::Void);
                return;
            }
            let due = p.start_sent.is_none_or(|at| waited >= at + START_RESEND);
            if ready && waited >= 2 && due {
                p.start_sent = Some(waited);
                let p = self.party.as_ref().expect("checked above");
                let kind = p.night.current_slot().map(|s| p.night.infos[s].kind);
                let seed = p.night.scoreboard_hash() ^ u64::from(waited) << 40;
                let model = kind.map_or(
                    SimModel::HostAuthoritative {
                        snapshot_hz: crate::mode_net::SNAPSHOT_HZ,
                    },
                    model_for,
                );
                let mode = round_id(&p.night);
                self.send(Message::Start(StartGame {
                    seed,
                    mode,
                    config: Vec::new(),
                    model,
                    seats: Vec::new(),
                }));
            }
            return;
        }
        p.night.tick();
        if p.night.phase_key() != p.last_phase {
            self.party_broadcast(PartyOp::Phase);
        }
    }

    fn party_guest(&mut self, me: PlayerId) {
        let in_lobby = matches!(self.phase, Phase::Lobby);
        let ready = self.ready;
        let Some(p) = self.party.as_mut() else { return };
        p.night.tick_display();
        // Ready up for every round this player plays (watchers need not).
        let phase = p.night.phase_key();
        let cued = matches!(
            p.night.phase,
            PartyPhase::Intro { .. } | PartyPhase::Playing { .. }
        );
        if cued && in_lobby && !ready && p.night.is_active(key(me)) && p.readied_for != Some(phase)
        {
            p.readied_for = Some(phase);
            self.send(Message::SetReady(true));
        }
    }

    /// Grant this machine's player their prize once the night completes.
    fn party_grant(&mut self, world: &mut World, me: PlayerId) {
        let Some(p) = self.party.as_mut() else { return };
        if p.granted.is_some() || !p.night.completed() {
            return;
        }
        let k = key(me);
        let won = p
            .night
            .standings()
            .iter()
            .any(|s| s.key == k && s.place == 1 && !s.left);
        let prize = p.night.prize_for(k).cloned();
        let id = p.night.id.clone();
        let lines = match prize {
            Some(prize) if p.night.is_active(k) => world.grant_party_prize(&id, &prize, won),
            _ => Vec::new(),
        };
        if let Some(p) = self.party.as_mut() {
            p.granted = Some(lines.clone());
        }
        if !lines.is_empty() {
            self.note(format!("You won: {}", lines.join(", ")));
        }
    }

    /// Raise a completed night's podium in this player's world, or take it
    /// down once the night closes. Every peer raises it from its own copy
    /// of the night (host-directed, hash-verified) at the pack's podium
    /// anchor, so the blocks and who stands on them agree everywhere; each
    /// peer stands its own leader, and the others are its roaming avatars
    /// (or stand-in figures where it does not see them).
    pub(super) fn sync_net_podium(&mut self, world: &mut World) {
        let up = match (self.me, self.party.as_deref()) {
            (Some(me), Some(p)) if p.night.completed() && !self.in_mode() => {
                Some((p.night.id.clone(), p.night.standings(), key(me)))
            }
            _ => None,
        };
        self.podium_in_world = match up {
            Some((id, standings, me)) => {
                let shown = self
                    .remote_avatars()
                    .iter()
                    .map(|a| key(a.player))
                    .collect();
                world.show_net_podium(&id, standings, me, shown)
                    && world.net_podium().is_some_and(|p| p.stage.is_some())
            }
            None => {
                if world.net_podium().is_some() {
                    world.clear_net_podium();
                }
                false
            }
        };
    }

    /// The party card to draw this frame, while one shows.
    pub fn party_card(&self) -> Option<PartyCard> {
        let p = self.party.as_deref()?;
        if self.in_mode() {
            return None;
        }
        let mut card = party_card(
            &p.night,
            self.me.map(key),
            p.granted.as_deref(),
            self.frames,
        );
        // A podium standing in the world is a caption over the field.
        if let PartyCard::Podium { in_world, .. } = &mut card {
            *in_world = self.podium_in_world;
        }
        Some(card)
    }
}
