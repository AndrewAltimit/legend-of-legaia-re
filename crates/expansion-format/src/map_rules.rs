//! The validator's rules for `maps/<id>.toml` ([`SectionRules`] for
//! [`MapDef`]).
//!
//! A map names content of every other section - formations, bosses, lines,
//! quests, encounter tables, other maps - and coordinates on the disc. These
//! rules check each against the section that defines it, the way the engine
//! will resolve it at run time, so a broken reference is a validation error
//! instead of a log line the first time a player steps on the zone.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::{
    ExitTarget, FLOOR_TIERS, Ground, MAX_WALL_HEIGHT, MapDef, MapEvent, PropMesh, TileGrid,
    WallMesh,
};
use crate::refs::DiscRef;
use crate::validate::{Checker, SectionRules};

/// The map id of the file being checked (`maps/<id>.toml`).
fn map_id(cx: &Checker<'_>) -> String {
    cx.file()
        .strip_prefix("maps/")
        .and_then(|f| f.strip_suffix(".toml"))
        .unwrap_or_default()
        .to_string()
}

impl SectionRules for MapDef {
    fn declare(&self, cx: &mut Checker<'_>) {
        for (i, n) in self.npcs.iter().enumerate() {
            cx.declare("npc", &n.id, &format!("npcs[{i}].id"));
        }
    }

    fn check(&self, cx: &mut Checker<'_>) {
        let grid = match self.tile_grid() {
            Ok(g) => Some(g),
            Err(e) => {
                cx.error("tiles", format!("{e:#}"));
                None
            }
        };
        if self.floor_heights.len() > FLOOR_TIERS {
            cx.error(
                "floor_heights",
                format!(
                    "{} tiers listed; a map has at most {FLOOR_TIERS}",
                    self.floor_heights.len()
                ),
            );
        }
        let base = check_base(self, cx);
        if let Some(g) = &grid {
            for (what, c, r) in self.off_grid_refs(g) {
                let field = off_grid_field(self, &what, [c, r]);
                cx.error(&field, format!("{what} names tile ({c}, {r}), off the map"));
            }
        }
        check_legend(self, cx, base.as_deref());
        check_spawns(self, cx, grid.as_ref());
        check_exits(self, cx);
        check_triggers(self, cx);
        check_encounters(self, cx);
        check_props(self, cx, base.as_deref());
        crate::map_detail_rules::check_detail(self, cx, base.as_deref(), grid.as_ref());
        crate::terrain_rules::check_terrain(self, cx, grid.as_ref());
        check_npcs(self, cx, base.as_deref(), grid.as_ref());
        check_entrances(self, cx);
        check_enter_cutscenes(self, cx);
        crate::audio::check_map_audio(&self.audio, cx, grid.as_ref());
        if let Some(bgm) = &self.bgm {
            if matches!(bgm, DiscRef::Bgm { .. }) {
                cx.disc_ref("bgm", bgm);
            } else {
                cx.error("bgm", format!("{bgm} is not a bgm ref"));
            }
        }
    }
}

/// Arrival cutscenes: each names a cutscene, and one bound to another map
/// would never play here.
fn check_enter_cutscenes(m: &MapDef, cx: &mut Checker<'_>) {
    for (i, e) in m.enter_cutscenes.iter().enumerate() {
        let at = format!("enter_cutscenes[{i}]");
        e.requires.check(cx, &format!("{at}.requires"));
        cx.reference("cutscene", &e.cutscene, &format!("{at}.cutscene"));
        let bound = cx
            .pack()
            .cutscenes
            .get(&e.cutscene)
            .and_then(|c| c.map.clone());
        let here = cx
            .file()
            .strip_prefix("maps/")
            .and_then(|f| f.strip_suffix(".toml"));
        if let (Some(b), Some(h)) = (bound, here)
            && b != h
        {
            cx.error(
                &format!("{at}.cutscene"),
                format!("cutscene `{}` plays on map `{b}`, not this one", e.cutscene),
            );
        }
    }
}

/// Entrances: a retail scene tile rect that warps onto one of this map's
/// spawns, gated by quest prerequisites.
fn check_entrances(m: &MapDef, cx: &mut Checker<'_>) {
    for (i, e) in m.entrances.iter().enumerate() {
        let at = format!("entrances[{i}]");
        if !matches!(e.scene, DiscRef::Scene { .. }) {
            cx.error(
                &format!("{at}.scene"),
                "an entrance names a scene (kind = \"scene\")",
            );
        }
        cx.disc_ref(&format!("{at}.scene"), &e.scene);
        if e.rect.iter().any(|&t| t > 0x7F) {
            cx.error(
                &format!("{at}.rect"),
                "field-grid tiles run 0..=127 on each axis",
            );
        }
        if m.spawn(&e.spawn).is_none() {
            cx.error(
                &format!("{at}.spawn"),
                if e.spawn.is_empty() {
                    "the map has no spawn to arrive at".to_string()
                } else {
                    format!("the map has no spawn {:?}", e.spawn)
                },
            );
        }
        e.requires.check(cx, &format!("{at}.requires"));
    }
}

/// The base scene: a scene ref the disc has. Returns its name.
fn check_base(m: &MapDef, cx: &mut Checker<'_>) -> Option<String> {
    match &m.base.scene {
        DiscRef::Scene { name } if name.is_empty() => {
            cx.error("base.scene", "the map names no base scene");
            None
        }
        r @ DiscRef::Scene { name } => {
            cx.disc_ref("base.scene", r);
            Some(name.clone())
        }
        other => {
            cx.error("base.scene", format!("{other} is not a scene ref"));
            None
        }
    }
}

fn check_legend(m: &MapDef, cx: &mut Checker<'_>, base: Option<&str>) {
    for (key, style) in &m.tiles.legend {
        let at = format!("tiles.legend.{key:?}");
        if let (Some(Ground::Mesh { index }), Some(base)) = (&style.ground, base) {
            cx.geometry_index(&format!("{at}.ground"), base, *index);
        }
        match &style.wall {
            None => {}
            Some(_) if matches!(style.ground, Some(Ground::Mesh { .. })) => cx.error(
                &format!("{at}.wall"),
                "a tile draws one cell mesh: a `mesh` ground and a wall cannot share a style",
            ),
            Some(WallMesh::Mesh { index }) => {
                if let Some(base) = base {
                    cx.geometry_index(&format!("{at}.wall"), base, *index);
                }
            }
            Some(WallMesh::Box { height, .. }) if *height == 0 || *height > MAX_WALL_HEIGHT => {
                cx.error(
                    &format!("{at}.wall.height"),
                    format!("wall height {height} is outside 1..={MAX_WALL_HEIGHT}"),
                );
            }
            Some(WallMesh::Box { .. }) => {}
        }
        if style.wall.is_some() && style.walls.mask() == 0 {
            cx.warning(
                &format!("{at}.wall"),
                "the style draws a wall but has no wall quarters; the player walks through it",
            );
        }
    }
}

/// Whether the tile is floor the player can stand on: on the grid, not void
/// and not fully walled.
fn standable(grid: &TileGrid, t: [u16; 2]) -> bool {
    grid.get(usize::from(t[0]), usize::from(t[1]))
        .is_some_and(|t| !t.void && t.walls != 0xF)
}

fn check_spawns(m: &MapDef, cx: &mut Checker<'_>, grid: Option<&TileGrid>) {
    if m.spawns.is_empty() {
        cx.error("spawns", "the map has no spawn; nothing can enter it");
    }
    let mut seen = BTreeSet::new();
    for (i, s) in m.spawns.iter().enumerate() {
        let at = format!("spawns[{i}]");
        if !seen.insert(s.id.as_str()) {
            cx.error(
                &format!("{at}.id"),
                format!("spawn id {:?} is used twice", s.id),
            );
        }
        if s.facing >= 8 {
            cx.error(
                &format!("{at}.facing"),
                format!("facing {} is not a compass sector 0..8", s.facing),
            );
        }
        if let Some(g) = grid
            && !standable(g, s.tile)
        {
            cx.warning(
                &format!("{at}.tile"),
                "the spawn stands on a void or walled tile",
            );
        }
    }
}

fn check_ids<'a>(cx: &mut Checker<'_>, what: &str, ids: impl Iterator<Item = &'a str>) {
    let mut seen = BTreeSet::new();
    for (i, id) in ids.enumerate() {
        if !id.is_empty() && !seen.insert(id) {
            cx.error(
                &format!("{what}[{i}].id"),
                format!("{what} id {id:?} is used twice"),
            );
        }
    }
}

fn check_exits(m: &MapDef, cx: &mut Checker<'_>) {
    check_ids(cx, "exits", m.exits.iter().map(|e| e.id.as_str()));
    let own = map_id(cx);
    for (i, e) in m.exits.iter().enumerate() {
        e.requires.check(cx, &format!("exits[{i}].requires"));
        if let Some(l) = &e.locked_line {
            cx.reference("line", l, &format!("exits[{i}].locked_line"));
            if e.requires.is_empty() {
                cx.warning(
                    &format!("exits[{i}].locked_line"),
                    "the exit has no `requires`, so it is never shut",
                );
            }
            let long = cx.pack().dialog.lines.get(l).map(|line| {
                crate::boss_script::banner_text(line).chars().count()
                    > crate::boss_script::BANNER_WARN_CHARS
            });
            if e.locked_toast && long == Some(true) {
                cx.warning(
                    &format!("exits[{i}].locked_line"),
                    format!(
                        "a toast shows one row; line `{l}`'s first page runs past {} characters",
                        crate::boss_script::BANNER_WARN_CHARS
                    ),
                );
            }
        } else if e.locked_toast {
            cx.warning(
                &format!("exits[{i}].locked_toast"),
                "`locked_toast` with no `locked_line` shows nothing",
            );
        }
        let at = format!("exits[{i}].to");
        match &e.to {
            ExitTarget::Retail {
                scene,
                tile,
                facing,
            } => {
                if matches!(scene, DiscRef::Scene { .. }) {
                    cx.disc_ref(&format!("{at}.scene"), scene);
                } else {
                    cx.error(
                        &format!("{at}.scene"),
                        format!("{scene} is not a scene ref"),
                    );
                }
                if let Some([c, r]) = tile
                    && (*c >= 128 || *r >= 128)
                {
                    cx.error(
                        &format!("{at}.tile"),
                        format!("tile ({c}, {r}) is off the 128 x 128 field grid"),
                    );
                }
                if *facing >= 8 {
                    cx.error(
                        &format!("{at}.facing"),
                        format!("facing {facing} is not a compass sector 0..8"),
                    );
                }
            }
            ExitTarget::Map { map, spawn } => {
                if *map == own {
                    cx.warning(&at, "the exit leads back into the same map");
                }
                cx.reference("map", map, &format!("{at}.map"));
                if let Some(target) = cx.pack().maps.get(map)
                    && target.spawn(spawn).is_none()
                {
                    cx.error(
                        &format!("{at}.spawn"),
                        format!("map `{map}` has no spawn {spawn:?}"),
                    );
                }
            }
        }
    }
}

fn check_triggers(m: &MapDef, cx: &mut Checker<'_>) {
    check_ids(cx, "triggers", m.triggers.iter().map(|t| t.id.as_str()));
    for (i, t) in m.triggers.iter().enumerate() {
        t.requires.check(cx, &format!("triggers[{i}].requires"));
        let at = format!("triggers[{i}].event");
        match &t.event {
            MapEvent::Battle { formation } => {
                cx.reference("formation", formation, &format!("{at}.formation"))
            }
            MapEvent::Boss { boss } => cx.reference("boss", boss, &format!("{at}.boss")),
            MapEvent::Dialog { line } => cx.reference("line", line, &format!("{at}.line")),
            MapEvent::Toast { line } => {
                cx.reference("line", line, &format!("{at}.line"));
                let long = cx.pack().dialog.lines.get(line).map(|l| {
                    crate::boss_script::banner_text(l).chars().count()
                        > crate::boss_script::BANNER_WARN_CHARS
                });
                if long == Some(true) {
                    cx.warning(
                        &format!("{at}.line"),
                        format!(
                            "a toast shows one row; line `{line}`'s first page runs past {} characters",
                            crate::boss_script::BANNER_WARN_CHARS
                        ),
                    );
                }
            }
            MapEvent::StartQuest { quest } => cx.reference("quest", quest, &format!("{at}.quest")),
            MapEvent::Talk { npc } => {
                if !talk_key_is_used(cx, npc) {
                    cx.warning(
                        &format!("{at}.npc"),
                        format!(
                            "no quest gives from or talks to `{npc}`, so the talk counts for nothing"
                        ),
                    );
                }
            }
            MapEvent::CardDealer { dealer } => {
                let dealers = &cx.pack().cards.dealers;
                let known = if dealer.is_empty() {
                    !dealers.is_empty()
                } else {
                    dealers.iter().any(|d| d.id == *dealer)
                };
                if !known {
                    cx.error(
                        &format!("{at}.dealer"),
                        format!("card dealer {dealer:?} is not a cards.toml [[dealers]] id"),
                    );
                }
            }
            MapEvent::Shop { shop } => {
                let shops = &cx.pack().items.shops;
                let known = if shop.is_empty() {
                    !shops.is_empty()
                } else {
                    shops.iter().any(|s| s.id == *shop)
                };
                if !known {
                    cx.error(
                        &format!("{at}.shop"),
                        format!("shop {shop:?} is not an items.toml [[shops]] id"),
                    );
                }
            }
            MapEvent::SetFlag { flag } | MapEvent::Counter { counter: flag, .. } => {
                if flag.is_empty() {
                    cx.error(&at, "the event names no flag or counter");
                }
            }
            MapEvent::Challenge { challenge, start } => {
                cx.reference("challenge", challenge, &format!("{at}.challenge"));
                let names_formation = cx
                    .pack()
                    .quests
                    .challenges
                    .iter()
                    .find(|c| c.id == *challenge)
                    .is_none_or(|c| c.formation.is_some());
                if *start && !names_formation {
                    cx.error(
                        &format!("{at}.start"),
                        format!("challenge `{challenge}` names no formation to start"),
                    );
                }
            }
            MapEvent::Mode { mode } => {
                if !cx.pack().modes.has_instance(mode) {
                    cx.error(
                        &format!("{at}.mode"),
                        format!("no modes.toml instance {mode:?}"),
                    );
                }
            }
            MapEvent::Rest { .. } => {}
            MapEvent::Cutscene { cutscene } => {
                cx.reference("cutscene", cutscene, &format!("{at}.cutscene"))
            }
            MapEvent::Sound { sound } => {
                crate::audio::check_sound(cx, &format!("{at}.sound"), sound);
            }
        }
    }
}

/// Whether any quest takes `npc` as its giver or a talk objective.
fn talk_key_is_used(cx: &Checker<'_>, npc: &str) -> bool {
    let quests = &cx.pack().quests;
    quests.quests.iter().any(|q| {
        q.giver.as_deref() == Some(npc)
            || q.objectives
                .iter()
                .any(|o| matches!(&o.goal, crate::quest::Goal::Talk { npc: n } if n == npc))
    })
}

fn check_encounters(m: &MapDef, cx: &mut Checker<'_>) {
    check_ids(cx, "encounters", m.encounters.iter().map(|z| z.id.as_str()));
    for (i, z) in m.encounters.iter().enumerate() {
        let at = format!("encounters[{i}].table");
        match cx
            .pack()
            .battle
            .encounters
            .iter()
            .find(|e| e.scene == z.table)
        {
            None => cx.error(
                &at,
                format!(
                    "encounter table `{}` is not a battle.toml [[encounters]] scene",
                    z.table
                ),
            ),
            Some(e) if e.steps.is_none() => cx.warning(
                &at,
                format!(
                    "table `{}` has no `steps`; a map has no retail rate to borrow, so it rolls nothing",
                    z.table
                ),
            ),
            Some(_) => {}
        }
    }
}

fn check_props(m: &MapDef, cx: &mut Checker<'_>, base: Option<&str>) {
    check_ids(cx, "props", m.props.iter().map(|p| p.id.as_str()));
    let mut tiles = BTreeMap::new();
    for (i, p) in m.props.iter().enumerate() {
        let at = format!("props[{i}]");
        if let Some(prev) = tiles.insert(p.tile, i) {
            cx.error(
                &format!("{at}.tile"),
                format!(
                    "tile {:?} already holds props[{prev}]; one prop a tile",
                    p.tile
                ),
            );
        }
        check_mesh(cx, &format!("{at}.mesh"), &p.mesh, base);
        check_animators(cx, &format!("{at}.animate"), &p.animate);
        if let Some(l) = &p.light {
            check_light(cx, &format!("{at}.light"), l.radius, l.flicker, l.period);
        }
        if let Some(s) = p.scale
            && !(s.is_finite() && s > 0.0)
        {
            cx.error(&format!("{at}.scale"), format!("scale {s} is not positive"));
        }
        if p.top.is_none() && p.footprint.is_some() {
            cx.warning(
                &format!("{at}.footprint"),
                "a footprint without a `top` is unread",
            );
        }
        if let Some([w, d]) = p.footprint
            && (w == 0 || d == 0 || w > 1024 || d > 1024)
        {
            cx.error(
                &format!("{at}.footprint"),
                format!("footprint [{w}, {d}] is outside 1..=1024 per axis"),
            );
        }
        if p.top.is_some() && p.solid {
            cx.warning(
                &format!("{at}.solid"),
                "a standable prop does not close its tile; `solid` is unread",
            );
        }
    }
}

/// A mesh reference: a base-pack index, another scene's pack index, a PROT
/// coordinate or a pack asset.
pub(crate) fn check_mesh(cx: &mut Checker<'_>, at: &str, mesh: &PropMesh, base: Option<&str>) {
    match mesh {
        PropMesh::Object { object } => {
            if let Some(base) = base {
                cx.geometry_index(at, base, *object);
            }
        }
        PropMesh::SceneObject { scene, object } => {
            let r = DiscRef::Scene {
                name: scene.clone(),
            };
            cx.disc_ref(&format!("{at}.scene"), &r);
            cx.geometry_index(at, scene, *object);
        }
        PropMesh::Tmd { tmd } => {
            if matches!(tmd, DiscRef::Tmd { .. }) {
                cx.disc_ref(&format!("{at}.tmd"), tmd);
            } else {
                cx.error(&format!("{at}.tmd"), format!("{tmd} is not a tmd ref"));
            }
        }
        PropMesh::Asset { asset, colors } => {
            crate::map_detail_rules::check_model_asset(cx, at, asset, *colors);
        }
    }
}

/// Animator periods and ranges.
pub(crate) fn check_animators(
    cx: &mut Checker<'_>,
    at: &str,
    anims: &[crate::map_detail::Animator],
) {
    use crate::map_detail::Animator;
    for (i, a) in anims.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let p = a.period();
        let turns = matches!(a, Animator::Spin { .. } | Animator::Orbit { .. });
        if p == 0 || (!turns && p < 0) {
            cx.error(
                &format!("{at}.period"),
                format!(
                    "period {p} frames; it must be at least 1 (a spin or orbit may be negative)"
                ),
            );
        }
        if let Animator::Pulse { amount, .. } = a
            && !(0.0..1.0).contains(amount)
        {
            cx.error(
                &format!("{at}.amount"),
                format!("pulse amount {amount} is outside 0..1"),
            );
        }
    }
}

/// A light's reach, flicker depth and flicker period.
pub(crate) fn check_light(cx: &mut Checker<'_>, at: &str, radius: u16, flicker: f32, period: i32) {
    if radius == 0 {
        cx.error(&format!("{at}.radius"), "a light needs a radius");
    }
    if !(0.0..=1.0).contains(&flicker) {
        cx.error(
            &format!("{at}.flicker"),
            format!("flicker {flicker} is outside 0..=1"),
        );
    }
    if flicker > 0.0 && period < 1 {
        cx.error(
            &format!("{at}.period"),
            "a flickering light needs a period of at least 1",
        );
    }
}

fn check_npcs(m: &MapDef, cx: &mut Checker<'_>, base: Option<&str>, grid: Option<&TileGrid>) {
    let mut tiles = BTreeMap::new();
    for (i, n) in m.npcs.iter().enumerate() {
        let at = format!("npcs[{i}]");
        match &n.model {
            None => cx.error(&format!("{at}.model"), "the NPC names no model"),
            Some(r @ DiscRef::Npc { scene, .. }) => {
                cx.disc_ref(&format!("{at}.model"), r);
                if base.is_some_and(|b| b != scene) {
                    cx.warning(
                        &format!("{at}.model"),
                        format!(
                            "the model is from `{scene}`, not the base scene; only a party-member \
                             placement draws outside its own scene"
                        ),
                    );
                }
            }
            Some(other) => cx.error(&format!("{at}.model"), format!("{other} is not an npc ref")),
        }
        if let Some(prev) = tiles.insert(n.tile, i) {
            cx.error(
                &format!("{at}.tile"),
                format!("tile {:?} already holds npcs[{prev}]", n.tile),
            );
        }
        if n.facing >= 8 {
            cx.error(
                &format!("{at}.facing"),
                format!("facing {} is not a compass sector 0..8", n.facing),
            );
        }
        if n.clip == Some(0) || n.walk_clip == Some(0) {
            cx.error(
                &format!("{at}.clip"),
                "clip ids are 1-based (ANM record + 1); leave `clip` out for the placement's own",
            );
        }
        if let Some(DiscRef::Npc { scene, index }) = &n.model
            && !n.scenery
        {
            check_npc_postures(m, cx, &at, n, scene, *index);
        }
        if let Some(g) = grid {
            for t in std::iter::once(&n.tile).chain(&n.patrol) {
                if !standable(g, *t) {
                    cx.warning(
                        &format!("{at}.tile"),
                        format!("tile {t:?} is void or walled; the NPC cannot stand there"),
                    );
                }
            }
        }
        if m.props.iter().any(|p| p.tile == n.tile) {
            cx.warning(&format!("{at}.tile"), "the NPC stands on a prop's tile");
        }
        if !n.line.is_empty() {
            cx.reference("line", &n.line, &format!("{at}.line"));
        }
        n.requires.check(cx, &format!("{at}.requires"));
        n.after.check(cx, &format!("{at}.after"));
        n.present.check(cx, &format!("{at}.present"));
        if let Some(c) = &n.cutscene {
            cx.reference("cutscene", c, &format!("{at}.cutscene"));
        }
        if !n.line_after.is_empty() {
            cx.reference("line", &n.line_after, &format!("{at}.line_after"));
            if n.after.is_empty() {
                cx.warning(
                    &format!("{at}.line_after"),
                    "the NPC has no `after` gate, so `line_after` is never said",
                );
            }
        }
        if let Some(shop) = &n.shop {
            let dealer = cx.pack().cards.dealers.iter().any(|d| d.id == *shop);
            let item_shop = cx.pack().items.shops.iter().any(|s| s.id == *shop);
            match (dealer, item_shop) {
                (false, false) => cx.error(
                    &format!("{at}.shop"),
                    format!(
                        "shop {shop:?} is neither an items.toml [[shops]] id nor a cards.toml [[dealers]] id"
                    ),
                ),
                (true, true) => cx.warning(
                    &format!("{at}.shop"),
                    format!("shop {shop:?} is both an item shop and a card dealer; the item shop opens"),
                ),
                _ => {}
            }
        }
        if !n.scenery
            && n.line.is_empty()
            && n.shop.is_none()
            && n.cutscene.is_none()
            && !talk_key_is_used(cx, &n.id)
        {
            cx.warning(
                &format!("{at}.line"),
                "the NPC says nothing, runs no shop and gives no quest",
            );
        }
    }
}

/// The NPC's clips against the measured posture table
/// ([`crate::npc_postures`]): a seated clip needs something to sit on - a
/// decoration or a standable prop on the NPC's tile - a patrol's
/// `walk_clip` should walk, and a `clip` must be one the model can play.
/// Placements the table does not cover, and scenery NPCs (often not a
/// figure at all: town01's windmill sails are a placement), are not
/// checked.
fn check_npc_postures(
    m: &MapDef,
    cx: &mut Checker<'_>,
    at: &str,
    n: &crate::map::NpcDef,
    scene: &str,
    index: u16,
) {
    use crate::npc_postures::{Posture, table};
    let Some(row) = table().get(scene, index) else {
        return;
    };
    let list = |p: Posture| {
        let v = row.clips(p);
        if v.is_empty() {
            "none".to_string()
        } else {
            v.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
        }
    };
    if let Some(c) = n.clip
        && row.posture(Some(c)).is_none()
    {
        cx.warning(
            &format!("{at}.clip"),
            format!(
                "clip {c} is not one `{scene}` placement {index}'s model can play (its bone \
                 count differs); standing clips: {}",
                list(Posture::Standing)
            ),
        );
    }
    let seat = m.decor.iter().any(|d| d.tile == n.tile)
        || m.props.iter().any(|p| p.tile == n.tile && p.top.is_some());
    if row.posture(n.clip) == Some(Posture::Seated) && !seat {
        let which = n.clip.map_or_else(
            || "the placement's own clip".to_string(),
            |c| format!("clip {c}"),
        );
        cx.warning(
            &format!("{at}.clip"),
            format!(
                "{which} is a seated pose with nothing under it: put a seat (a decoration or a \
                 prop with a `top`) on tile {:?}, or pick a standing clip: {}",
                n.tile,
                list(Posture::Standing)
            ),
        );
    }
    if let Some(w) = n.walk_clip
        && !n.patrol.is_empty()
        && let Some(p) = row.posture(Some(w))
        && p != Posture::Walking
    {
        cx.warning(
            &format!("{at}.walk_clip"),
            format!(
                "walk clip {w} is a {} pose; walking clips: {}",
                p.word(),
                list(Posture::Walking)
            ),
        );
    }
}

#[cfg(test)]
mod tests;

/// The field an [`MapDef::off_grid_refs`] entry is about: the spawn, prop
/// or NPC it names by id whose tile (or patrol stop) is that tile; empty for
/// the zones, whose rectangles the zone checks report on their own.
fn off_grid_field(map: &MapDef, what: &str, tile: [u16; 2]) -> String {
    let Some((kind, id)) = what.split_once(' ') else {
        return String::new();
    };
    let id = id.trim_matches('"');
    match kind {
        "spawn" => map
            .spawns
            .iter()
            .position(|s| s.id == id && s.tile == tile)
            .map(|i| format!("spawns[{i}].tile")),
        "prop" => map
            .props
            .iter()
            .position(|p| p.id == id && p.tile == tile)
            .map(|i| format!("props[{i}].tile")),
        "npc" => map
            .npcs
            .iter()
            .position(|n| n.id == id)
            .map(|i| format!("npcs[{i}]")),
        _ => None,
    }
    .unwrap_or_default()
}
