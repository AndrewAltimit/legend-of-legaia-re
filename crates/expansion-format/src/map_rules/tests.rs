use crate::ExpansionPack;
use crate::manifest::PackManifest;
use crate::refs::{DiscRef, DiscResolver};
use crate::validate::{Issue, Severity, validate, validate_with};

const BATTLE: &str = r#"
[monsters.ogre]
base = { kind = "monster", id = 5 }

[formations.pack]
monsters = ["ogre"]

[[encounters]]
scene = "ring"
steps = [10, 20]
formations = [{ formation = "pack" }]

[[encounters]]
scene = "rateless"
formations = [{ formation = "pack" }]
"#;

const DIALOG: &str = r#"
[lines.hello]
text = "Hello."
"#;

const QUESTS: &str = r#"
[[quests]]
id = "errand"
title = "Errand"
giver = "elder"
objectives = [{ kind = "talk", npc = "elder" }]
"#;

const MAP: &str = r######"
name = "Hall"
base = { scene = { kind = "scene", name = "town01" } }

[tiles]
rows = ["#####", "#...#", "#...#", "#####"]
[tiles.legend]
"#" = { walls = "all", wall = { kind = "box", height = 128, tpage = 28, clut = 32576, tile = 2 } }
"." = {}

[[spawns]]
id = "door"
tile = [1, 1]

[[exits]]
rect = [3, 2, 3, 2]
to = { kind = "retail", scene = { kind = "scene", name = "town01" }, tile = [28, 91], facing = 4 }

[[triggers]]
rect = [2, 1, 2, 1]
event = { kind = "battle", formation = "pack" }

[[triggers]]
rect = [2, 2, 2, 2]
event = { kind = "dialog", line = "hello" }

[[encounters]]
rect = [1, 1, 3, 2]
table = "ring"

[[props]]
mesh = { object = 3 }
tile = [3, 1]
top = 48

[[npcs]]
id = "elder"
model = { kind = "npc", scene = "town01", index = 4 }
tile = [2, 2]
line = "hello"
"######;

fn pack(map: &str) -> ExpansionPack {
    let mut manifest = PackManifest::new("test", "Test", "0.1.0");
    manifest.description = "d".into();
    manifest.authors = vec!["me".into()];
    manifest.license = "CC0-1.0".into();
    let mut p = ExpansionPack {
        manifest,
        battle: toml::from_str(BATTLE).expect("battle"),
        dialog: toml::from_str(DIALOG).expect("dialog"),
        quests: toml::from_str(QUESTS).expect("quests"),
        ..Default::default()
    };
    p.maps
        .insert("hall".into(), toml::from_str(map).expect("map parses"));
    p
}

fn errors(issues: &[Issue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_sound_map_validates_clean() {
    let issues = validate(&pack(MAP));
    assert!(errors(&issues).is_empty(), "{issues:#?}");
    assert!(
        issues.iter().all(|i| !i.path.starts_with("maps/")),
        "{issues:#?}"
    );
}

#[test]
fn broken_references_are_errors() {
    let map = MAP
        .replace("formation = \"pack\"", "formation = \"nope\"")
        .replace("line = \"hello\" }", "line = \"gone\" }")
        .replace("table = \"ring\"", "table = \"tower_1f\"");
    let e = errors(&validate(&pack(&map)));
    for want in [
        "formation \"nope\"",
        "line \"gone\"",
        "encounter table `tower_1f`",
    ] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
}

#[test]
fn a_rateless_table_only_warns() {
    let map = MAP.replace("table = \"ring\"", "table = \"rateless\"");
    let issues = validate(&pack(&map));
    assert!(errors(&issues).is_empty(), "{issues:#?}");
    assert!(issues.iter().any(|i| i.message.contains("no `steps`")));
}

#[test]
fn ids_and_layout_problems_are_errors() {
    let map = format!(
        "{MAP}\n[[spawns]]\nid = \"door\"\ntile = [2, 1]\n\n[[npcs]]\nid = \"elder\"\nmodel = {{ kind = \"npc\", scene = \"town01\", index = 0 }}\ntile = [9, 9]\n"
    );
    let e = errors(&validate(&pack(&map)));
    for want in [
        "spawn id \"door\" is used twice",
        "npc id \"elder\" is already defined",
        "placement 0 is the scene controller",
        "off the map",
    ] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
}

#[test]
fn exits_to_other_maps_name_real_maps_and_spawns() {
    let map = MAP.replace(
        "to = { kind = \"retail\", scene = { kind = \"scene\", name = \"town01\" }, tile = [28, 91], facing = 4 }",
        "to = { kind = \"map\", map = \"hall\", spawn = \"nowhere\" }",
    );
    let e = errors(&validate(&pack(&map)));
    assert!(
        e.iter().any(|m| m.contains("no spawn \"nowhere\"")),
        "{e:#?}"
    );
    let map = MAP.replace(
        "to = { kind = \"retail\", scene = { kind = \"scene\", name = \"town01\" }, tile = [28, 91], facing = 4 }",
        "to = { kind = \"map\", map = \"cellar\" }",
    );
    let e = errors(&validate(&pack(&map)));
    assert!(e.iter().any(|m| m.contains("map \"cellar\"")), "{e:#?}");
}

#[test]
fn a_wall_mesh_and_a_mesh_ground_cannot_share_a_tile() {
    let map = MAP.replace(
        "\".\" = {}",
        "\".\" = {}\n\"m\" = { ground = { kind = \"mesh\", index = 1 }, wall = { kind = \"mesh\", index = 2 } }",
    );
    let e = errors(&validate(&pack(&map)));
    assert!(e.iter().any(|m| m.contains("one cell mesh")), "{e:#?}");
}

/// A disc with 3 meshes in every scene's geometry pack and no npc 4.
struct SmallDisc;

impl DiscResolver for SmallDisc {
    fn resolve(&self, r: &DiscRef) -> Result<(), String> {
        match r {
            DiscRef::Npc { index, .. } if *index >= 4 => Err("no such placement".into()),
            DiscRef::Scene { name } if name == "nowhere" => Err("not on this disc".into()),
            _ => Ok(()),
        }
    }

    fn scene_geometry_len(&self, _scene: &str) -> Option<usize> {
        Some(3)
    }
}

#[test]
fn the_resolver_checks_npcs_mesh_indices_and_exit_scenes() {
    let map = MAP.replace(
        "name = \"town01\" }, tile = [28, 91]",
        "name = \"nowhere\" }, tile = [28, 91]",
    );
    let e = errors(&validate_with(&pack(&map), Some(&SmallDisc)));
    for want in ["npc town01 #4", "mesh 3 is past the last", "scene nowhere"] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
}

#[test]
fn a_dealer_npc_and_trigger_name_a_real_dealer() {
    let map = format!(
        "{MAP}\n[[npcs]]\nid = \"dealer\"\nmodel = {{ kind = \"npc\", scene = \"town01\", index = 2 }}\ntile = [1, 2]\nshop = \"nowhere\"\n\n[[triggers]]\nrect = [3, 2, 3, 2]\nevent = {{ kind = \"card_dealer\" }}\n"
    );
    let e = errors(&validate(&pack(&map)));
    for want in ["shop \"nowhere\"", "card dealer \"\""] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
}

#[test]
fn gates_challenges_modes_and_rest_points_name_real_content() {
    let map = format!(
        r#"{MAP}
[[triggers]]
rect = [1, 2, 1, 2]
event = {{ kind = "challenge", challenge = "nope", start = true }}
requires = {{ quests = ["ghost"], flags_clear = [" "] }}

[[triggers]]
rect = [3, 1, 3, 1]
event = {{ kind = "mode", mode = "no-table" }}

[[triggers]]
rect = [1, 1, 1, 1]
event = {{ kind = "rest", cost = 100, save = true }}
"#
    )
    .replace(
        "facing = 4 }\n",
        "facing = 4 }\nrequires = { flags = [\"key\"] }\nlocked_line = \"sealed\"\n",
    )
    .replace(
        "tile = [2, 2]\nline = \"hello\"\n",
        "tile = [2, 2]\nline = \"hello\"\nline_after = \"bye\"\nafter = { quests = [\"errand\"] }\n",
    );
    let e = errors(&validate(&pack(&map)));
    for want in [
        "challenge \"nope\"",
        "quest \"ghost\"",
        "an empty pack flag id",
        "no modes.toml instance \"no-table\"",
        "line \"sealed\"",
        "line \"bye\"",
    ] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
    assert!(!e.iter().any(|m| m.contains("errand")), "{e:#?}");
}

#[test]
fn a_seated_clip_wants_a_seat_and_a_walk_clip_should_walk() {
    // town01 placement 10 per the committed posture table: clip 18 seated,
    // clip 15 standing, clip 44 walking.
    let npc = r#"
[[npcs]]
id = "sitter"
model = { kind = "npc", scene = "town01", index = 10 }
tile = [3, 2]
clip = 18
patrol = [[1, 2]]
walk_clip = 15
line = "hello"
"#;
    let warnings = |map: &str| -> Vec<String> {
        validate(&pack(map))
            .into_iter()
            .filter(|i| i.severity == Severity::Warning && i.path == "maps/hall.toml")
            .map(|i| format!("{}: {}", i.field, i.message))
            .collect()
    };
    let bare = warnings(&format!("{MAP}{npc}"));
    assert!(
        bare.iter()
            .any(|w| w.starts_with("npcs[1].clip") && w.contains("seated")),
        "{bare:#?}"
    );
    assert!(
        bare.iter()
            .any(|w| w.starts_with("npcs[1].walk_clip") && w.contains("standing")),
        "{bare:#?}"
    );
    let seat = "\n[[decor]]\nmesh = { object = 3 }\ntile = [3, 2]\n";
    let fixed = npc.replace("walk_clip = 15", "walk_clip = 44");
    let seated = warnings(&format!("{MAP}{fixed}{seat}"));
    assert!(
        !seated.iter().any(|w| w.starts_with("npcs[1].")),
        "{seated:#?}"
    );
}

/// A disc whose every mesh is a 32-unit cube on its origin.
struct CubeDisc;

impl DiscResolver for CubeDisc {
    fn resolve(&self, _r: &DiscRef) -> Result<(), String> {
        Ok(())
    }

    fn mesh_shape(
        &self,
        _base_scene: &str,
        _mesh: &crate::map::PropMesh,
        _rotation: [f32; 3],
        scale: f32,
    ) -> Option<crate::map_rest::Shape> {
        let h = 16.0 * scale;
        Some(crate::map_rest::Shape {
            hull: vec![[-h, -h], [h, -h], [h, h], [-h, h]],
            bottom: 0.0,
            top: 2.0 * h,
        })
    }
}

#[test]
fn resting_is_judged_with_the_resolvers_meshes_and_on_without_one() {
    let map = format!(
        "{MAP}\n[[props]]\nid = \"cup\"\nmesh = {{ object = 1 }}\ntile = [1, 2]\noffset = [0, 40, 0]\n\n[[props]]\nid = \"lost\"\nmesh = {{ object = 1 }}\ntile = [2, 1]\non = \"nothing\"\n"
    );
    let p = pack(&map);
    // No disc: the broken `on` is an error, the floating cup unjudged.
    let issues = validate(&p);
    assert!(
        errors(&issues)
            .iter()
            .any(|m| m.contains("props[2].on") && m.contains("no prop")),
        "{issues:#?}"
    );
    assert!(!issues.iter().any(|i| i.message.contains("floats")));
    let issues = validate_with(&p, Some(&CubeDisc));
    assert!(
        issues.iter().any(|i| i.severity == Severity::Warning
            && i.field == "props[1]"
            && i.message.contains("floats 40")),
        "{issues:#?}"
    );
}

#[test]
fn a_ceiling_holds_its_ranges_and_names_hung_items() {
    let hung = "\n[[props]]\nid = \"lamp\"\nmesh = { object = 3 }\ntile = [2, 1]\nhangs = true\n\n[[props]]\nid = \"crate\"\nmesh = { object = 4 }\ntile = [1, 2]\n";
    let good = format!(
        "{MAP}{hung}\n[ceiling]\nheight = 2048\ncell = {{ tpage = 28, clut = 32576, tile = 2 }}\nchain = {{ tpage = 28, clut = 32576, tile = 2, width = 6, items = [\"lamp\"] }}\n"
    );
    let issues = validate(&pack(&good));
    assert!(errors(&issues).is_empty(), "{issues:#?}");
    let bad = format!(
        "{MAP}{hung}\n[ceiling]\nheight = 5000\ncell = {{ tpage = 28, clut = 32576, tile = 70 }}\nchain = {{ tpage = 28, clut = 32576, tile = 2, width = 0, items = [\"crate\", \"gone\"] }}\n"
    );
    let e = errors(&validate(&pack(&bad)));
    for want in [
        "height 5000",
        "atlas cell 70",
        "width 0",
        "\"crate\" names no prop",
        "\"gone\" names no prop",
    ] {
        assert!(e.iter().any(|m| m.contains(want)), "{want} in {e:#?}");
    }
    let empty = format!("{MAP}\n[ceiling]\nheight = 900\n");
    let issues = validate(&pack(&empty));
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("the ceiling draws nothing")),
        "{issues:#?}"
    );
}
