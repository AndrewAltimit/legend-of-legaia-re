//! Disc-gated: Rebirth's main quest line played through on the native
//! session, from a New Game to the First Breath.
//!
//! `engine-expansion`'s `rebirth_chain` proves the quest tracker completes
//! when fed the right events, and `engine-scene-host`'s
//! `expansion_rebirth_reach_disc` that every place on every map is walkable
//! from the map's first spawn. This test joins the two in the running game:
//! every way the party moves is the one a player takes, gated as the player
//! meets it.
//!
//! - A New Game slate, then Rim Elm (`town01`) with the party raised to
//!   level 30 (the stand-in for a late save). The pause menu's Travel page
//!   offers the tower before its first visit; the party walks onto the
//!   woodpile entrance instead.
//! - Each floor is left by its stair exit: the player is seated on a floor
//!   sub-cell beside the exit that the walk from where they arrived reaches,
//!   then on one inside it, and the next tick has to land on the exit's
//!   map. A shut gate or a walled-off stair fails here.
//! - Each NPC, examine point and offer is pressed with Cross from a
//!   reached sub-cell, its dialogue is paged with Cross (the first option of
//!   every offer is the fight), and every fight it starts is entered and won
//!   on auto at a hundredth of the enemies' stats.
//! - Every step settles: the field comes back free (no dialogue, cutscene,
//!   battle or scene change holding it) within a tick budget, or the test
//!   names the step that softlocked.
//!
//! Levels between floors are raised with the shared level cheat, as a
//! player levelling in the Isles would. Skips when `LEGAIA_DISC_BIN` is
//! unset.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use legaia_engine_cheats::cheats::{PlayerCheat, WorldCheats};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{ExitTarget, MapDef, TileRect};

/// Ticks one step may take to hand the field back.
const SETTLE_TICKS: u32 = 120_000;

struct Run {
    s: NativeSession,
    pack: Arc<ExpansionPack>,
    battles: u32,
    log: Vec<String>,
    /// Pack lines seen, with the option picked.
    lines: Vec<(String, Option<usize>)>,
}

/// Reached sub-cells (64 units, four to a tile) of the active map.
struct Reach {
    w: i32,
    h: i32,
    seen: Vec<bool>,
}

impl Reach {
    fn at(&self, sc: i32, sr: i32) -> bool {
        sc >= 0 && sr >= 0 && sc < self.w && sr < self.h && self.seen[(sr * self.w + sc) as usize]
    }

    /// The reached sub-cells inside `rect` grown by `grow` tiles.
    fn cells_in(&self, rect: &TileRect, grow: i32) -> Vec<(i32, i32)> {
        let (c0, r0) = (i32::from(rect[0]) - grow, i32::from(rect[1]) - grow);
        let (c1, r1) = (i32::from(rect[2]) + grow, i32::from(rect[3]) + grow);
        let mut out = Vec::new();
        for sr in r0 * 2..=r1 * 2 + 1 {
            for sc in c0 * 2..=c1 * 2 + 1 {
                if self.at(sc, sr) {
                    out.push((sc, sr));
                }
            }
        }
        out
    }
}

fn in_rect(rect: &TileRect, sc: i32, sr: i32) -> bool {
    let (c, r) = (sc / 2, sr / 2);
    c >= i32::from(rect[0])
        && c <= i32::from(rect[2])
        && r >= i32::from(rect[1])
        && r <= i32::from(rect[3])
}

fn sub_centre(def: &MapDef, sc: i32, sr: i32) -> (i16, i16) {
    let (x, z) = tile_centre_world(def, (sc / 2) as u16, (sr / 2) as u16);
    let off = |s: i32| if s % 2 == 0 { -32 } else { 32 };
    (x + off(sc), z + off(sr))
}

impl Run {
    fn open() -> Option<Self> {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
            return None;
        }
        let extracted = ["extracted", "../extracted", "../../extracted"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
        let Some(extracted) = extracted else {
            eprintln!("[skip] extracted/ missing");
            return None;
        };
        let cfg = BootConfig {
            scene: "town01".into(),
            enable_audio: false,
        };
        let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
        let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
        let pack = Arc::new(ExpansionPack::from_files(&files).expect("the Rebirth pack parses"));
        s.host
            .activate_expansion_pack(pack.clone())
            .expect("the Rebirth pack activates");
        Some(Self {
            s,
            pack,
            battles: 0,
            log: Vec::new(),
            lines: Vec::new(),
        })
    }

    fn opts() -> FieldLiveOpts {
        FieldLiveOpts {
            live_loop: false,
            player_battle: false,
            battle_bgm: None,
        }
    }

    fn map(&self) -> String {
        self.s
            .host
            .active_expansion_map()
            .map(str::to_string)
            .unwrap_or_else(|| format!("<{}>", self.s.host.world.active_scene_label))
    }

    fn def(&self) -> &MapDef {
        let id = self.s.host.active_expansion_map().expect("on a pack map");
        &self.pack.maps[id]
    }

    fn tick(&mut self, pad: u16) {
        self.s.host.world.set_pad(pad);
        self.s.tick().expect("tick");
        let w = &self.s.host.world;
        assert!(
            !w.game_over && !w.game_over_hold,
            "the party wiped on {}; log:\n{}",
            self.map(),
            self.log.join("\n")
        );
    }

    /// Whether something other than the free field holds the game.
    fn busy(&self) -> bool {
        let w = &self.s.host.world;
        w.mode != SceneMode::Field
            || w.dialogue_owns_input()
            || w.expansion_cutscene_active()
            || w.cutscene_timeline_active()
            || w.expansion.quests.dialog.pending.is_some()
            || !w.expansion.quests.dialog.queued.is_empty()
            || w.scene_transition_hold.is_some()
            || w.pending_named_scene_transition.is_some()
    }

    /// Run until the field is free again, paging dialogue and cutscenes
    /// with Cross and letting every fight run on auto.
    fn settle(&mut self, what: &str) {
        let mut idle = 0;
        let mut in_battle = false;
        for i in 0..SETTLE_TICKS {
            let busy = self.busy();
            if self.s.host.world.mode == SceneMode::Battle {
                if !in_battle {
                    self.battles += 1;
                }
                in_battle = true;
            } else {
                in_battle = false;
            }
            let pad = if busy && i % 3 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            };
            self.tick(pad);
            if let Some(p) = self.s.host.world.expansion.quests.dialog.pending.as_ref() {
                let seen = (p.line.clone(), p.choice);
                if self.lines.last() != Some(&seen) {
                    self.lines.push(seen);
                }
            }
            if self.busy() {
                idle = 0;
            } else {
                idle += 1;
                if idle > 150 {
                    return;
                }
            }
        }
        let w = &self.s.host.world;
        eprintln!(
            "[softlock] naming {} inline {:?} menu {} cut {:?}",
            w.name_entry_active(),
            w.dialog
                .inline
                .as_ref()
                .map(|d| (d.pc, d.panel.is_some(), d.done)),
            self.s.field_menu_is_open(),
            w.expansion.cutscene.queued
        );
        panic!(
            "softlock after {what} on {}: mode {:?}, dialogue {}, cutscene {}, pending {:?}; log:\n{}",
            self.map(),
            w.mode,
            w.dialogue_owns_input(),
            w.expansion_cutscene_active(),
            w.expansion
                .quests
                .dialog
                .pending
                .as_ref()
                .map(|p| p.line.clone()),
            self.log.join("\n")
        );
    }

    fn level(&mut self, level: u8) {
        let t = self.s.host.world.tables.starting_party.clone();
        let w = &mut self.s.host.world;
        w.apply_cheat(&PlayerCheat::PartyLevel(level), t.as_ref());
        w.apply_cheat(&PlayerCheat::Restore, None);
        self.s
            .host
            .world
            .expansion_notify(legaia_engine_expansion::quest::QuestEvent::Refresh);
        self.log.push(format!("level {level}"));
    }

    /// The player's sub-cell on the active map.
    fn player_sub(&self) -> (i32, i32) {
        let def = self.def();
        let ms = &self.s.host.world.actors[0].move_state;
        let (x, z) = (i32::from(ms.world_x), i32::from(ms.world_z));
        let m = self.s.host.world.expansion.map.as_ref().expect("map");
        let (c, r) = m
            .tile_at_world(x, z)
            .unwrap_or_else(|| panic!("the player at ({x}, {z}) is off {}", self.map()));
        let (cx, cz) = tile_centre_world(def, c, r);
        (
            i32::from(c) * 2 + i32::from(x >= i32::from(cx)),
            i32::from(r) * 2 + i32::from(z >= i32::from(cz)),
        )
    }

    /// Flood the active map's walk from where the player stands.
    fn flood(&self) -> Reach {
        let def = self.def();
        let rows = def.tiles.rows.len() as i32;
        let cols = def
            .tiles
            .rows
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap_or(0) as i32;
        let (w, h) = (cols * 2, rows * 2);
        let mut seen = vec![false; (w * h) as usize];
        let world = &self.s.host.world;
        let open = |sc: i32, sr: i32| {
            let (x, z) = sub_centre(def, sc, sr);
            !world.field_tile_is_wall(x, z)
        };
        let (sc0, sr0) = self.player_sub();
        let mut q = VecDeque::new();
        // The arrival seat may sit on a stair's own wall cell; start from
        // every open sub-cell of its tile and the tiles around it.
        for dr in -2..=3 {
            for dc in -2..=3 {
                let (sc, sr) = ((sc0 & !1) + dc, (sr0 & !1) + dr);
                if sc >= 0 && sr >= 0 && sc < w && sr < h && open(sc, sr) {
                    seen[(sr * w + sc) as usize] = true;
                    q.push_back((sc, sr));
                }
            }
        }
        while let Some((sc, sr)) = q.pop_front() {
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nc, nr) = (sc + dc, sr + dr);
                if nc < 0 || nr < 0 || nc >= w || nr >= h {
                    continue;
                }
                let i = (nr * w + nc) as usize;
                if !seen[i] && open(nc, nr) {
                    seen[i] = true;
                    q.push_back((nc, nr));
                }
            }
        }
        Reach { w, h, seen }
    }

    /// Seat the player on sub-cell `(sc, sr)` of the active map.
    fn seat(&mut self, sc: i32, sr: i32, facing: Option<i16>) {
        let (x, z) = sub_centre(self.def(), sc, sr);
        let w = &mut self.s.host.world;
        w.seat_player_at_tile((x >> 7) as u8, (z >> 7) as u8);
        let ms = &mut w.actors[0].move_state;
        ms.world_x = x;
        ms.world_z = z;
        if let Some(f) = facing {
            ms.render_26 = f;
        }
    }

    /// Walk out of the active map by exit `exit` and land on its map.
    fn take_exit(&mut self, exit: &str) {
        let def = self.def().clone();
        let from = self.map();
        let e = def
            .exits
            .iter()
            .find(|e| e.id == exit)
            .unwrap_or_else(|| panic!("{from} has no exit {exit}"))
            .clone();
        assert!(
            self.s.host.world.expansion_prerequisites_hold(&e.requires),
            "{from}: exit {exit} is still shut ({:?}); log:\n{}",
            e.requires,
            self.log.join("\n")
        );
        let reach = self.flood();
        let inside = reach.cells_in(&e.rect, 0);
        let outside: Vec<_> = reach
            .cells_in(&e.rect, 1)
            .into_iter()
            .filter(|&(sc, sr)| !in_rect(&e.rect, sc, sr))
            .collect();
        let (Some(&a), Some(&b)) = (outside.first(), inside.first()) else {
            panic!(
                "{from}: exit {exit} at {:?} is walled off from the arrival",
                e.rect
            );
        };
        self.seat(a.0, a.1, None);
        self.tick(0);
        self.seat(b.0, b.1, None);
        for _ in 0..4 {
            self.tick(0);
            if self.map() != from {
                break;
            }
        }
        if let ExitTarget::Map { map, .. } = &e.to {
            assert_eq!(&self.map(), map, "{from}: exit {exit} did not land");
        }
        self.log.push(format!("{from} -> {} by {exit}", self.map()));
        self.settle(&format!("arriving by {exit}"));
    }

    /// Press Cross at trigger `id` of the active map, from a reached
    /// sub-cell on it or beside it facing it, and settle what it starts.
    fn press(&mut self, id: &str) {
        let def = self.def().clone();
        let map = self.map();
        let t = def
            .triggers
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("{map} has no trigger {id}"))
            .clone();
        assert!(
            self.s.host.world.expansion_prerequisites_hold(&t.requires),
            "{map}: trigger {id} is shut ({:?}); log:\n{}",
            t.requires,
            self.log.join("\n")
        );
        let reach = self.flood();
        let notices = self.s.host.world.expansion.quests.notices.len();
        let mut seats: Vec<((i32, i32), Option<i16>)> = reach
            .cells_in(&t.rect, 0)
            .into_iter()
            .map(|c| (c, None))
            .collect();
        // Beside it, facing it: the compass point whose action probe (a
        // tile ahead along the retail facing probe, `FIELD_FACING_PROBES`)
        // lands inside the trigger.
        const PROBES: [(i32, i32); 8] = [
            (0, 64),
            (-64, 64),
            (-64, 0),
            (-64, -64),
            (0, -64),
            (64, -64),
            (64, 0),
            (64, 64),
        ];
        {
            let m = self.s.host.world.expansion.map.as_ref().expect("map");
            for (sc, sr) in reach.cells_in(&t.rect, 1) {
                if in_rect(&t.rect, sc, sr) {
                    continue;
                }
                let (x, z) = sub_centre(&def, sc, sr);
                for (sector, (dx, dz)) in PROBES.iter().enumerate() {
                    let ahead = m.tile_at_world(i32::from(x) + 2 * dx, i32::from(z) - 2 * dz);
                    if ahead
                        .is_some_and(|(c, r)| in_rect(&t.rect, i32::from(c) * 2, i32::from(r) * 2))
                    {
                        let facing = (((sector as i16) << 9) - 0x800 + 0x100) & 0x0FFF;
                        seats.push(((sc, sr), Some(facing)));
                    }
                }
            }
        }
        assert!(
            !seats.is_empty(),
            "{map}: trigger {id} at {:?} is walled off from the arrival",
            t.rect
        );
        for ((sc, sr), facing) in seats.into_iter().take(64) {
            self.seat(sc, sr, facing);
            self.tick(0);
            self.tick(0);
            self.tick(PadButton::Cross.mask());
            self.tick(0);
            let fired = self.busy() || self.s.host.world.expansion.quests.notices.len() > notices;
            if fired {
                let before = self.battles;
                let l0 = self.lines.len();
                self.settle(&format!("pressing {id}"));
                self.log.push(format!(
                    "{map}: pressed {id} ({} fight(s)) lines {:?}",
                    self.battles - before,
                    &self.lines[l0..]
                ));
                return;
            }
        }
        panic!(
            "{map}: Cross at trigger {id} {:?} fired nothing; log:\n{}",
            t.rect,
            self.log.join("\n")
        );
    }

    /// Walk onto walk-on trigger `id`: step from a reached sub-cell beside
    /// it onto a reached one inside it, then settle what it starts.
    fn walk_onto(&mut self, id: &str) {
        let def = self.def().clone();
        let map = self.map();
        let t = def
            .triggers
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("{map} has no trigger {id}"))
            .clone();
        let reach = self.flood();
        let inside = reach.cells_in(&t.rect, 0);
        let outside: Vec<_> = reach
            .cells_in(&t.rect, 1)
            .into_iter()
            .filter(|&(sc, sr)| !in_rect(&t.rect, sc, sr))
            .collect();
        let (Some(&a), Some(&b)) = (outside.first(), inside.first()) else {
            panic!("{map}: trigger {id} at {:?} is walled off", t.rect);
        };
        self.seat(a.0, a.1, None);
        self.tick(0);
        self.seat(b.0, b.1, None);
        self.tick(0);
        let before = self.battles;
        self.settle(&format!("walking onto {id}"));
        self.log.push(format!(
            "{map}: walked onto {id} ({} fight(s))",
            self.battles - before
        ));
    }

    /// Talk to pack NPC `id` with Cross from a reached sub-cell beside it,
    /// trying each facing until `done` holds after the talk settles.
    fn talk_npc(&mut self, id: &str, done: impl Fn(&Run) -> bool) {
        let def = self.def().clone();
        let map = self.map();
        let k = def
            .npcs
            .iter()
            .position(|n| n.id == id)
            .unwrap_or_else(|| panic!("{map} has no NPC {id}"));
        let tile = def.npcs[k].tile;
        let rect: TileRect = [tile[0], tile[1], tile[0], tile[1]];
        let reach = self.flood();
        let cells: Vec<_> = reach
            .cells_in(&rect, 1)
            .into_iter()
            .filter(|&(sc, sr)| !in_rect(&rect, sc, sr))
            .collect();
        assert!(!cells.is_empty(), "{map}: NPC {id} is walled off");
        for (sc, sr) in cells {
            for sector in 0..8i16 {
                let facing = ((sector << 9) - 0x800 + 0x100) & 0x0FFF;
                self.seat(sc, sr, Some(facing));
                self.tick(0);
                self.tick(PadButton::Cross.mask());
                self.tick(0);
                if !self.busy() {
                    continue;
                }
                let l0 = self.lines.len();
                self.settle(&format!("talking to {id}"));
                if done(self) {
                    self.log.push(format!(
                        "{map}: talked to {id} lines {:?}",
                        &self.lines[l0..]
                    ));
                    return;
                }
            }
        }
        panic!(
            "{map}: no talk to {id} did what it should; log:\n{}",
            self.log.join("\n")
        );
    }

    fn completed(&self, quest: &str) -> bool {
        self.s
            .host
            .world
            .expansion
            .quests
            .quest(quest)
            .is_some_and(|q| q.completions > 0)
    }

    fn expect_done(&self, quest: &str) {
        assert!(
            self.completed(quest),
            "quest {quest} is not complete ({:?}) on {}; log:\n{}",
            self.s.host.world.expansion.quests.quest(quest),
            self.map(),
            self.log.join("\n")
        );
    }
}

#[test]
fn the_rebirth_main_line_plays_from_a_new_game_to_the_first_breath() {
    let Some(mut run) = Run::open() else {
        return;
    };
    let opts = Run::opts();
    // A New Game, then Rim Elm with a party that has grown to level 30.
    assert!(run.s.start_new_game(&opts).is_some(), "the New Game enters");
    // The opening's scripted chain is the retail game's to play; this run
    // stands in Rim Elm after it, as the play page's scene pick does.
    run.s.host.world.abandon_opening_chain();
    run.s
        .enter_field_live("town01", &opts)
        .expect("enter Rim Elm");
    run.s.host.world.toggles.enemy_stat_permille = 10;
    run.level(30);
    run.settle("Rim Elm");

    // The Travel page offers the tower now, before any visit: its entrance
    // is open to this party.
    let travel: BTreeSet<String> = run
        .s
        .host
        .world
        .expansion_journal()
        .expect("the Journal")
        .maps
        .into_iter()
        .map(|m| m.map_id)
        .collect();
    assert!(
        travel.contains("rebirth_tower_1f"),
        "Travel offers the tower: {travel:?}"
    );

    // The woodpile entrance: step off it, then onto it.
    run.s.host.world.seat_player_at_tile(16, 43);
    run.tick(0);
    run.s.host.world.seat_player_at_tile(17, 43);
    for _ in 0..4 {
        run.tick(0);
        if run.s.host.active_expansion_map().is_some() {
            break;
        }
    }
    assert_eq!(
        run.map(),
        "rebirth_tower_1f",
        "the woodpile enters the tower"
    );
    run.log
        .push("town01 -> rebirth_tower_1f by the woodpile".into());
    run.settle("the tower intro");

    // 1F: the stone wakes, then sets the Warden's Trial.
    run.press("warden_stone");
    run.expect_done("tower_rumour");
    if !run.completed("tower_trial") {
        // Three won fights; the bell rings one each.
        for _ in 0..6 {
            let won = run
                .s
                .host
                .world
                .expansion
                .quests
                .quest("tower_trial")
                .map(|q| q.objectives[0]);
            if won.is_some_and(|n| n >= 3) {
                break;
            }
            run.press("gauntlet_bell");
        }
        run.level(40);
        run.press("warden_stone");
    }
    run.expect_done("tower_trial");

    // 2F: the Cinder Warden.
    run.level(50);
    run.take_exit("stair_up");
    run.press("brazier");
    run.expect_done("ember_stair");

    // 3F: the Garden Gauntlet.
    run.take_exit("stair_up");
    run.press("sundial");
    run.expect_done("hanging_garden");

    // 4F: the Verdant Colossus.
    run.level(60);
    run.take_exit("stair_up");
    run.press("sunken_court");
    run.expect_done("verdant_vault");

    // 5F: the Gambler's Rest.
    run.take_exit("stair_up");
    run.press("keeper");
    run.expect_done("gamblers_rest");

    // 6F: the Engine Gauntlet, then the Iron Regent.
    run.level(70);
    run.take_exit("stair_up");
    run.press("regent_alcove");
    // The alcove's talk and the Regent's offer share its tiles: one press
    // may already have fought him.
    run.press("gauntlet_lever");
    if !run.completed("engine_deep") {
        run.press("regent_challenge");
    }
    run.expect_done("engine_deep");

    // 7F: the Azure Seraph.
    run.level(80);
    run.take_exit("stair_up");
    run.press("inner_hall");
    run.expect_done("sanctum_echoes");

    // 8F: the throne's approach plays the Sovereign's rise on the walk up,
    // then the throne offers the fight.
    run.level(88);
    run.take_exit("stair_up");
    let def = run.def().clone();
    let approach = def
        .triggers
        .iter()
        .find(|t| t.id == "throne_approach")
        .expect("8F throne_approach")
        .clone();
    let reach = run.flood();
    let inside = reach.cells_in(&approach.rect, 0);
    let outside: Vec<_> = reach
        .cells_in(&approach.rect, 1)
        .into_iter()
        .filter(|&(sc, sr)| !in_rect(&approach.rect, sc, sr))
        .collect();
    let (Some(_), Some(_)) = (outside.first(), inside.first()) else {
        panic!("8F: the throne approach is walled off");
    };
    run.walk_onto("throne_approach");
    if !run.completed("tower_sovereign") {
        run.press("throne");
    }
    run.expect_done("tower_sovereign");
    assert!(
        run.s
            .host
            .world
            .expansion
            .quests
            .progress
            .flags
            .is_set("throne_broken"),
        "the ending cutscene ran to its flag"
    );

    // Back to the stone by the Journal's Travel page, at level 99.
    run.level(99);
    assert!(
        run.s
            .host
            .world
            .request_expansion_travel("rebirth_tower_1f", "dais"),
        "Travel to the Atrium"
    );
    run.tick(0);
    assert_eq!(run.map(), "rebirth_tower_1f");
    run.settle("travelling home");
    run.press("warden_stone");
    run.expect_done("rebirth");
    if !run.completed("first_breath") {
        run.press("warden_stone");
    }
    run.expect_done("first_breath");
    eprintln!(
        "[ran] Rebirth main line: {} fights; {}",
        run.battles,
        run.log.join(" | ")
    );
}

fn active(run: &Run, quest: &str) -> bool {
    run.s
        .host
        .world
        .expansion
        .quests
        .quest(quest)
        .is_some_and(|q| {
            q.completions > 0 || q.status == legaia_engine_expansion::quest::QuestStatus::Active
        })
}

#[test]
fn the_rebirth_isles_chain_plays_from_the_sea_gate_to_the_pearl() {
    let Some(mut run) = Run::open() else {
        return;
    };
    let opts = Run::opts();
    assert!(run.s.start_new_game(&opts).is_some(), "the New Game enters");
    run.s.host.world.abandon_opening_chain();
    run.s.host.world.toggles.enemy_stat_permille = 10;
    run.level(30);
    // The Atrium, by the Travel page this time (the entrance is open).
    run.s
        .enter_field_live("town01", &opts)
        .expect("enter Rim Elm");
    assert!(
        run.s
            .host
            .world
            .request_expansion_travel("rebirth_tower_1f", "")
    );
    run.tick(0);
    assert_eq!(run.map(), "rebirth_tower_1f", "Travel enters the tower");
    run.settle("the tower intro");
    run.press("warden_stone");
    run.expect_done("tower_rumour");

    // The sea gate opens once Ysolde has spoken.
    run.take_exit("sea_gate");
    run.talk_npc("isles_ferrywoman", |r| active(r, "isles_landfall"));
    run.take_exit("dune_path");
    run.level(35);
    run.talk_npc("isles_elder", |r| r.completed("isles_landfall"));
    if !active(&run, "isles_thornwood") {
        run.talk_npc("isles_elder", |r| active(r, "isles_thornwood"));
    }
    run.take_exit("to_thornwood");
    run.walk_onto("hive_gate");
    assert!(
        run.s.host.world.expansion_boss_cleared("isles_hive_queen"),
        "the Thornwood Queen falls; log:\n{}",
        run.log.join("\n")
    );
    run.talk_npc("isles_hermit", |r| r.completed("isles_thornwood"));
    run.expect_done("isles_thornwood");
    if !active(&run, "isles_pearl") {
        run.talk_npc("isles_hermit", |r| active(r, "isles_pearl"));
    }
    run.take_exit("grotto_stair");
    run.press("pearl");
    run.expect_done("isles_pearl");
    eprintln!(
        "[ran] Rebirth Isles: {} fights; {}",
        run.battles,
        run.log.join(" | ")
    );
}
