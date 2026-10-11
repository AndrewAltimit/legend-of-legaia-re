//! Driving a scene: entering it against a flag baseline, settling it, probing it with pad
//! input and stepping the player onto a tile.

use super::*;

/// The player's world `(x, z)`.
pub(super) fn player_xz(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.unwrap_or(0) as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

pub(super) fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// Where the context holding the pad came to rest, as `(pc, opcode)`: the
/// modal timeline's, else the first live spawned record's, else the scene's
/// own entry script's. Reporting the entry script's `field_pc` for a record
/// that holds the pad read as a stall where there was none - `edlast`'s
/// "jump-to-self" was its entry script idling while the credits record ran.
fn park_site(host: &SceneHost) -> Option<(usize, u8)> {
    let at = |bc: &[u8], pc: usize| bc.get(pc).map(|op| (pc, *op));
    if let Some(tl) = host.world.cutscene.timeline.as_ref() {
        return at(&tl.bytecode, tl.pc);
    }
    if let Some(h) = host.world.field_vm.helper_contexts.first() {
        return at(&h.bytecode, h.pc);
    }
    at(&host.world.field_bytecode, host.world.field_pc)
}

/// The flag state every entry of a scene starts from.
///
/// The ladder re-enters a scene several times (the driven probe, its
/// released-pad control, one attempt per exit site) and those entries are
/// only comparable if they are all the *same visit*. Most chapter-1 interiors
/// carry a C1-gated first-visit record that self-latches, so the second entry
/// of a scene is a materially different scene - `izumi`'s spring
/// choreography spawns once and then never again. Restoring the banks before
/// every entry is what keeps the control a control.
#[derive(Clone)]
pub(super) struct FlagBaseline {
    system: Vec<u8>,
    story: u32,
}

impl FlagBaseline {
    pub(super) fn snapshot(host: &SceneHost) -> Self {
        Self {
            system: host.world.flags.system_flags.clone(),
            story: host.world.flags.story_flags,
        }
    }
    pub(super) fn restore(&self, host: &mut SceneHost) {
        host.world.flags.system_flags.clone_from(&self.system);
        host.world.flags.story_flags = self.story;
    }
}

/// Enter `name` through the mode-appropriate entry point, leaving the flag
/// banks exactly as they are - so whatever the previous entry latched stays
/// latched. This is a *revisit*.
pub(super) fn enter_raw(host: &mut SceneHost, name: &str) -> bool {
    // Drop whatever the PREVIOUS scene left holding input. A scene that ends
    // the sweep parked on a dialogue (28 of the closure's scenes open one from
    // their entry script and never close it under a released pad) otherwise
    // hands that dialogue to the next scene, whose rung-4 verdict then reports
    // its predecessor's park - `jouine` scored a rung-4 stall it does not have
    // when entered on its own. A verdict has to be a property of the scene.
    host.world.dialog.current = None;
    host.world.dialog.inline = None;
    host.world.cutscene.timeline = None;
    host.world.field_vm.helper_contexts.clear();
    let r = if is_world_map_scene(name) {
        host.enter_world_map_scene(name)
    } else {
        host.enter_field_scene(name, 0)
    };
    r.is_ok() && matches!(host.world.mode, SceneMode::Field | SceneMode::WorldMap)
}

/// Restore the flag banks and enter `name`. This is a *first visit*.
pub(super) fn enter(host: &mut SceneHost, name: &str, base: &FlagBaseline) -> bool {
    base.restore(host);
    enter_raw(host, name)
}

/// Outcome of letting a freshly-entered scene's script run itself out.
pub(super) enum Settle {
    /// Control came back to the player.
    Released,
    /// The script left the scene on its own (a scripted `0x3F`).
    Departed(String),
    /// The budget ran out with the timeline or a dialogue still holding
    /// input; `(pc, opcode)` is where the field VM came to rest.
    Parked {
        held_by: &'static str,
        at: Option<(usize, u8)>,
    },
}

/// Tick until the scene hands control back, with the pad released except
/// for Confirm pulses that page any dialogue the entry script opens.
///
/// "Control back" is three things, not one: no cutscene timeline, no dialogue
/// owning input, **and** no spawned record still running. The third is not
/// cosmetic - a first-visit record is a helper context rather than a
/// timeline, so a settle that ignored them returned while `izumi`'s spring
/// choreography was still `MoveTo`-ing the player, and the walk probe then
/// scored thirty tiles of script-driven motion as locomotion.
pub(super) fn settle(host: &mut SceneHost) -> Settle {
    for i in 0..SETTLE_TICKS {
        // Released = the pad is the player's again: no timeline, no
        // dialogue, no helper record (retail's engaged bit is raised for
        // every context the script runner steps, so a first-visit helper
        // that walks the player refuses the pad until it ends), and no
        // spawn still queued to start one.
        // Not before two ticks: the entry script's op-`0x44` spawns queue on
        // the first and their helpers take their first slice on the second,
        // so a check on the entry frame sees none of them.
        // A record that ended on `3E FF` has committed a fight whose intro
        // runs after the record is gone; retail's field frame (tile compare
        // included) is held until it is fought, and the fight itself is not
        // the field either, so the scene has not settled. `jouine` P2[5]
        // stages the Cort fight that way.
        if i >= 2
            && host.world.scene_transition_hold.is_none()
            && !host.world.cutscene_timeline_active()
            && !host.world.dialogue_owns_input()
            && host.world.field_vm.helper_contexts.is_empty()
            && host.world.field_vm.pending_record_spawns.is_empty()
            && !host.world.field_scripts_held_for_battle()
            && host.world.mode != legaia_engine_mode::mode::SceneMode::Battle
        {
            return Settle::Released;
        }
        // A dialogue page waits for Confirm, as it does for a player: pulse
        // Cross (press on one tick, release on the next, so every page sees
        // a fresh edge) while a dialogue owns input, and leave the pad
        // released otherwise so nothing else the script runs is steered.
        let pad = if host.world.dialogue_owns_input() && i % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Settle::Departed(name),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let held_by = if host.world.cutscene_timeline_active() {
        "cutscene timeline"
    } else if host.world.field_scripts_held_for_battle()
        || host.world.mode == legaia_engine_mode::mode::SceneMode::Battle
    {
        "a committed battle"
    } else if host.world.dialogue_owns_input() {
        "dialogue"
    } else {
        "a spawned record"
    };
    Settle::Parked {
        held_by,
        at: park_site(host),
    }
}

/// What one locomotion probe observed.
pub(super) struct Probe {
    /// Furthest Chebyshev tile distance from the probe's start the player
    /// ever reached - not the final position, which a wall-slide can shorten.
    pub(super) tiles: i32,
    /// A walk-on trigger took the scene away mid-probe, and where it went.
    pub(super) left: Option<String>,
    /// A random encounter replaced the world mid-probe (after which
    /// `player_world` is a battle-arena transform, so the probe stops).
    pub(super) encounter: bool,
    /// The tile the probe started on - what a stall is reported against.
    pub(super) start_tile: (i16, i16),
}

/// Run `pads` in sequence, `WALK_TICKS` ticks each, and report the furthest
/// the player got. `pads` is `[0]` for the neutral control and the four
/// cardinals for the driven probe.
pub(super) fn probe(host: &mut SceneHost, pads: &[u16], ticks_each: usize) -> Probe {
    let (sx, sz) = player_xz(host);
    let start_tile = tile_of(sx, sz);
    let mut out = Probe {
        tiles: 0,
        left: None,
        encounter: false,
        start_tile,
    };
    for &mask in pads {
        for i in 0..ticks_each {
            // A dialogue the walk itself opens is paged as a player pages it,
            // with the direction still held: Cross pulses (press, release)
            // only while a dialogue owns input, so the pulse cannot start an
            // interact of its own. Without it, one walk-touch sign froze
            // every later direction - `tower`'s spawn faces the rapid
            // elevator, whose record types "Out of service" while its switch
            // `0x1C6` is off, and the probe stood in that text box for the
            // remaining three cardinals. The control applies the same rule.
            let mask = if host.world.dialogue_owns_input() && i % 2 == 0 {
                mask | PadButton::Cross.mask()
            } else {
                mask
            };
            host.world.set_pad(mask);
            match host.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => {
                    out.left = Some(name);
                    return out;
                }
                Ok(_) => {}
                Err(_) => return out,
            }
            if host.world.mode == SceneMode::Battle {
                out.encounter = true;
                return out;
            }
            let (x, z) = player_xz(host);
            let (tx, tz) = tile_of(x, z);
            out.tiles = out
                .tiles
                .max((tx - start_tile.0).abs().max((tz - start_tile.1).abs()) as i32);
        }
    }
    host.world.set_pad(0);
    out
}

/// The four cardinals, held in turn.
///
/// The sweep is deliberate: a scene whose spawn faces a wall on one axis
/// still proves locomotion on another, and scoring a single direction would
/// report a wall as a stall.
pub(super) const CARDINALS: [PadButton; 4] = [
    PadButton::Up,
    PadButton::Down,
    PadButton::Left,
    PadButton::Right,
];

/// Seat the player one tile off `(tx, tz)` then onto it, ticking between, so
/// the walk-on dispatch sees a genuine tile crossing (retail's dispatcher
/// fires on a tile *change*).
///
/// The approach tile is one that carries no walk-on trigger of its own when
/// there is one: a record the approach fires holds the player, and retail's
/// dispatcher consumes a crossing made under that hold rather than deferring
/// it (`jouine`'s P2[16] band at `(17, 17)` sits beside another band).
pub(super) fn step_onto_tile(host: &mut SceneHost, tx: u8, tz: u8) {
    host.world.set_pad(0);
    let world = |t: u8| i16::from(t) * 128 + 0x40;
    let near = [
        (tx.checked_sub(1), Some(tz)),
        (tx.checked_add(1).filter(|&x| x < 128), Some(tz)),
        (Some(tx), tz.checked_sub(1)),
        (Some(tx), tz.checked_add(1).filter(|&z| z < 128)),
    ];
    let candidates: Vec<(u8, u8)> = near
        .into_iter()
        .filter_map(|(x, z)| Some((x?, z?)))
        .collect();
    let (ox, oz) = candidates
        .iter()
        .copied()
        .find(|&(x, z)| !host.tile_has_walk_on_trigger(world(x), world(z)))
        .or_else(|| candidates.first().copied())
        .unwrap_or((tx, tz));
    host.world.seat_player_at_tile(ox, oz);
    let _ = host.tick();
    host.world.seat_player_at_tile(tx, tz);
}
