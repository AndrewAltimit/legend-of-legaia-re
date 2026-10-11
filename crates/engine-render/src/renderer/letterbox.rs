//! The letterbox aspect-fit scale of the window blit.

pub(crate) fn letterbox_scale(win_w: u32, win_h: u32, tex_w: u32, tex_h: u32) -> (f32, f32) {
    let win_aspect = win_w as f32 / win_h.max(1) as f32;
    let tex_aspect = tex_w as f32 / tex_h.max(1) as f32;
    if win_aspect > tex_aspect {
        // Window wider than texture - pillarbox
        (tex_aspect / win_aspect, 1.0)
    } else {
        // Window taller than texture - letterbox
        (1.0, win_aspect / tex_aspect)
    }
}
