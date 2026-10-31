use super::*;

/// A 4 x 3 room on tier 0, a `C` counter box 88 high on its east column.
const ROOM: &str = r#"
name = "room"
[tiles]
rows = ["...C", "...C", "...C"]
[tiles.legend]
"." = { label = "floor" }
"C" = { label = "counter", walls = "all", wall = { kind = "box", height = 88, tpage = 1, clut = 1, tile = 1 } }
"#;

/// Object 1: a table 120 across, top 80 up. Object 2: a mug 16 across,
/// 12 tall, its origin 2 above its base. Object 3: a plank 200 wide.
fn shapes(m: &PropMesh, r: [f32; 3], s: f32) -> Option<Shape> {
    let PropMesh::Object { object } = m else {
        return None;
    };
    let (hw, hd, bottom, top) = match object {
        1 => (60.0, 60.0, 0.0, 80.0),
        2 => (8.0, 8.0, -2.0, 10.0),
        3 => (100.0, 20.0, 0.0, 4.0),
        _ => return None,
    };
    let (sn, c) = r[1].to_radians().sin_cos();
    let hull = [[-hw, -hd], [hw, -hd], [hw, hd], [-hw, hd]]
        .map(|[x, z]| [(x * c + z * sn) * s, (-x * sn + z * c) * s])
        .to_vec();
    Some(Shape {
        hull,
        bottom: bottom * s,
        top: top * s,
    })
}

fn room(extra: &str) -> MapDef {
    toml::from_str(&format!("{ROOM}\n{extra}")).unwrap()
}

#[test]
fn on_sets_the_base_on_a_mesh_top_and_a_standable_top() {
    let def = room(
        r#"
[[props]]
id = "table"
mesh = { object = 1 }
tile = [1, 1]
scale = 1.5

[[props]]
id = "mug"
mesh = { object = 2 }
tile = [0, 0]
offset = [128, 0, 100]
on = "table"

[[props]]
id = "crate"
mesh = { object = 1 }
tile = [2, 0]
top = 50

[[decor]]
mesh = { object = 2 }
tile = [2, 0]
offset = [0, 3, 0]
on = "crate"
"#,
    );
    let (out, problems) = rest_on(&def, &shapes);
    assert!(problems.is_empty(), "{problems:?}");
    // 80 * 1.5 = 120 top, less the mug's -2 bottom.
    assert_eq!(out.props[1].offset[1], 122);
    assert!(out.props[1].on.is_empty());
    // The standable top's 50, the -2 bottom, its own 3.
    assert_eq!(out.decor[0].offset[1], 55);
    // Resolving again is the identity.
    assert_eq!(rest_on(&out, &shapes).0, out);
    assert!(rest_issues(&def, &shapes).is_empty());
}

#[test]
fn on_chains_and_reports_broken_references() {
    let def = room(
        r#"
[[props]]
id = "table"
mesh = { object = 1 }
tile = [1, 1]

[[props]]
id = "plank"
mesh = { object = 3 }
tile = [1, 0]
offset = [0, 0, 128]
on = "table"

[[props]]
id = "mug"
mesh = { object = 2 }
tile = [0, 1]
offset = [128, 0, 0]
on = "plank"

[[props]]
id = "a"
mesh = { object = 2 }
tile = [0, 2]
on = "b"

[[props]]
id = "b"
mesh = { object = 2 }
tile = [1, 2]
on = "a"

[[props]]
id = "lost"
mesh = { object = 2 }
tile = [2, 2]
on = "nothing"
"#,
    );
    let (out, problems) = rest_on(&def, &shapes);
    // table 80, plank 4 more, the mug's -2 bottom.
    assert_eq!(out.props[1].offset[1], 80);
    assert_eq!(out.props[2].offset[1], 86);
    assert!(problems.iter().all(|p| p.broken));
    assert!(
        problems.iter().any(|p| p.message.contains("itself")),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| p.field == "props[5].on" && p.message.contains("no prop")),
        "{problems:?}"
    );
}

#[test]
fn raised_items_over_nothing_float() {
    let def = room(
        r#"
[[props]]
id = "table"
mesh = { object = 1 }
tile = [1, 1]

[[props]]
id = "hovering_mug"
mesh = { object = 2 }
tile = [0, 1]
offset = [128, 100, 0]

[[props]]
id = "sunk_mug"
mesh = { object = 2 }
tile = [1, 0]
offset = [0, 60, 128]

[[props]]
id = "loose_mug"
mesh = { object = 2 }
tile = [0, 0]
offset = [0, 40, 0]

[[props]]
id = "lamp"
mesh = { object = 2 }
tile = [0, 2]
offset = [0, 250, 0]
hangs = true

[[props]]
id = "bottle"
mesh = { object = 2 }
tile = [3, 0]
offset = [0, 90, 0]

[[decor]]
mesh = { object = 2 }
tile = [2, 2]
"#,
    );
    let issues = rest_issues(&def, &shapes);
    let of = |id: &str| issues.iter().find(|i| i.id == id).map(|i| &i.kind);
    assert!(
        matches!(of("hovering_mug"), Some(RestKind::OffTop { by, .. }) if (*by - 18.0).abs() < 0.5),
        "{issues:?}"
    );
    assert!(
        matches!(of("sunk_mug"), Some(RestKind::OffTop { by, .. }) if (*by + 22.0).abs() < 0.5),
        "{issues:?}"
    );
    assert!(
        matches!(of("loose_mug"), Some(RestKind::Floating { gap }) if (*gap - 38.0).abs() < 0.5),
        "{issues:?}"
    );
    // Hung, on the counter's box (88 + the -2 bottom), on the floor.
    assert_eq!(of("lamp"), None);
    assert_eq!(of("bottle"), None);
    assert_eq!(issues.len(), 3, "{issues:?}");
    assert!(issues[0].to_string().contains("rest it with `on`"));
}

#[test]
fn an_item_wider_than_its_support_overhangs() {
    let def = room(
        r#"
[[props]]
id = "stool"
mesh = { object = 2 }
tile = [1, 1]
scale = 2.0

[[props]]
id = "plank"
mesh = { object = 3 }
tile = [0, 1]
offset = [128, 0, 0]
on = "stool"
"#,
    );
    let issues = rest_issues(&def, &shapes);
    assert_eq!(issues.len(), 1, "{issues:?}");
    // The plank's 100 half-width past the stool's 16.
    assert!(
        matches!(&issues[0].kind, RestKind::Overhang { support, by }
            if support == "\"stool\"" && (*by - 84.0).abs() < 0.5),
        "{issues:?}"
    );
}

#[test]
fn an_unmeasured_mesh_nearby_withholds_judgement() {
    let def = room(
        r#"
[[props]]
mesh = { asset = "models/table.glb" }
tile = [1, 1]

[[props]]
mesh = { object = 2 }
tile = [2, 1]
offset = [-100, 70, 0]
"#,
    );
    assert!(rest_issues(&def, &shapes).is_empty());
}

#[test]
fn a_flat_card_holds_nothing() {
    let card = vec![[-40.0, 0.0], [40.0, 0.0], [40.0, 0.0], [-40.0, 0.0]];
    assert!(!hull_contains(&card, [10.0, 0.0]));
    let square = vec![[-40.0, -40.0], [40.0, -40.0], [40.0, 40.0], [-40.0, 40.0]];
    assert!(hull_contains(&square, [10.0, 0.0]));
    // Clockwise reads the same.
    let cw: Vec<_> = square.iter().rev().copied().collect();
    assert!(hull_contains(&cw, [10.0, 0.0]));
    assert!(!hull_contains(&cw, [50.0, 0.0]));
}
