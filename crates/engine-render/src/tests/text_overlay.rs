use super::*;

#[test]
fn letterbox_scale_pillarbox() {
    let (sx, sy) = letterbox_scale(800, 400, 100, 100);
    assert!((sx - 0.5).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 1.0).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn letterbox_scale_letterbox() {
    let (sx, sy) = letterbox_scale(400, 800, 100, 100);
    assert!((sx - 1.0).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 0.5).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn sprite_draws_translate_world_positions_with_anchor() {
    let reqs = vec![
        SpriteRequest {
            world_x: 5,
            world_y: 7,
            atlas_src: (16, 0, 14, 15),
            color: [1.0, 1.0, 1.0, 1.0],
        },
        SpriteRequest {
            world_x: 0,
            world_y: 0,
            atlas_src: (0, 16, 14, 15),
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    let draws = sprite_draws_for(&reqs, (100, 200));
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0].dst, (105, 207, 14, 15));
    assert_eq!(draws[0].src, (16, 0, 14, 15));
    assert_eq!(draws[1].dst, (100, 200, 14, 15));
    assert_eq!(draws[1].color, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn text_draws_translate_layout_to_screen_space() {
    let font = legaia_font::synthetic_for_tests();
    let layout = font.layout(b"Ab");
    let pen = (10, 20);
    let color = [1.0, 0.5, 0.25, 1.0];
    let draws = text_draws_for(&layout, pen, color);
    assert_eq!(draws.len(), layout.glyphs.len());
    let g0 = layout.glyphs[0];
    let d0 = draws[0];
    assert_eq!(d0.dst.0, pen.0 + g0.dst_x);
    assert_eq!(d0.dst.1, pen.1 + g0.dst_y);
    assert_eq!(d0.dst.2, g0.width);
    assert_eq!(d0.src, (g0.atlas_x, g0.atlas_y, g0.width, g0.height));
    assert_eq!(d0.color, color);
}
