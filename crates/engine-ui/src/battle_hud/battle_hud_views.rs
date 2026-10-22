//! The battle HUD's input views and output draws, and the colour / atlas lookups its rows share.

use super::*;

/// One row in the battle HUD's per-slot panel (built by
/// [`battle_hud_draws_for`]).
///
/// Engines populate this view from their HUD model on a per-frame basis.
/// The renderer is intentionally agnostic to the engine-core / engine-vm
/// types - pass plain data here to keep the layering clean.
#[derive(Clone, Copy)]
pub struct HudSlotView<'a> {
    /// Display name (character / monster). Empty string skips the row.
    pub name: &'a str,
    /// `true` for party rows (white text); `false` for monster rows
    /// (pale red text).
    pub is_party: bool,
    /// `true` if the actor is alive. Dead actors get a "K.O." overlay.
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Amount of AP committed to the action queue this turn.
    pub ap_filled: u8,
    /// Maximum AP for the slot this turn.
    pub ap_max: u8,
    /// Gauge fill-colour index for the drawn HP bar - retail's
    /// `FUN_80046A20` code space (`2` dead, `3` status override, `7` high,
    /// `6` mid, `9` low). Engines derive it with
    /// `legaia_engine_battle::battle_hud_state::BattleSlotHud::gauge_fill_indices`;
    /// [`gauge_fill_color`] maps it to RGBA.
    pub hp_fill: u8,
    /// Gauge fill-colour index for the drawn MP bar (same code space).
    pub mp_fill: u8,
    /// The **one** status element retail draws for this slot: the sprite id
    /// `0x18..=0x20` its priority ladder selected, or `0` for the no-ailment
    /// base marker. Engines fill it from
    /// `legaia_engine_battle::battle_hud_state::BattleSlotHud::status_sprite`; see
    /// [`status_element_label`] for the id space.
    pub status_sprite: u8,
    /// Displayed character level - the count retail draws beside the base
    /// marker when `status_sprite == 0`. `0` suppresses the number.
    pub level: u8,
}

/// One floating damage / heal / status popup.
#[derive(Clone, Copy)]
pub struct HudPopupView {
    pub slot: u8,
    pub amount: u16,
    pub is_heal: bool,
    pub is_crit: bool,
    /// Status letter to overlay on the popup ('B' = Toxic, etc.). `None`
    /// for plain numeric popups.
    pub status_letter: Option<u8>,
    /// Fade alpha 0..=1.0 multiplied into the text colour.
    pub alpha: f32,
}

/// One battle log line.
#[derive(Clone, Copy)]
pub struct HudLogView<'a> {
    pub text: &'a str,
    pub color: [f32; 4],
}

impl<'a> HudSlotView<'a> {
    /// Build a slot view from a plain-data row. The argument shape mirrors
    /// `legaia_engine_battle::battle_hud_state::SlotView`; engines drive this from
    /// `BattleHud::slot_views()` without re-implementing the field copy.
    ///
    /// `name` borrows from the caller; ownership stays in the engine-core
    /// view buffer.
    pub fn from_plain(meta: HudSlotMeta, name: &'a str) -> Self {
        Self {
            name,
            is_party: meta.is_party,
            alive: meta.alive,
            hp: meta.hp,
            hp_max: meta.hp_max,
            mp: meta.mp,
            mp_max: meta.mp_max,
            ap_filled: meta.ap_filled,
            ap_max: meta.ap_max,
            hp_fill: meta.hp_fill,
            mp_fill: meta.mp_fill,
            status_sprite: meta.status_sprite,
            level: meta.level,
        }
    }
}

/// Numeric fields of [`HudSlotView`] grouped into a payload struct so the
/// public constructor stays under clippy's argument-count threshold.
#[derive(Debug, Clone, Copy, Default)]
pub struct HudSlotMeta {
    pub is_party: bool,
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap_filled: u8,
    pub ap_max: u8,
    /// See [`HudSlotView::hp_fill`] / [`HudSlotView::mp_fill`].
    pub hp_fill: u8,
    pub mp_fill: u8,
    /// See [`HudSlotView::status_sprite`].
    pub status_sprite: u8,
    /// See [`HudSlotView::level`].
    pub level: u8,
}

/// Short label for a retail status-element sprite id.
///
/// The ids are `FUN_8002C2E4`'s priority-ladder outputs and the ailment each
/// one stands for is pinned in
/// `legaia_engine_battle_vm::status_effects::display_flags`. The id space is
/// mirrored here as literals rather than imported - not for a dependency
/// reason (this crate depends on `engine-vm`) but because that module
/// tabulates the ailment *bitmasks*, not this sprite-id band; the mapping is
/// one line per row of its table.
///
/// The retail **art** for these ids is a sprite sheet the engine does not
/// resolve, so hosts draw a labelled badge instead - the selection is what is
/// ported, not the pixels. Returns `""` for `0` (the no-ailment base marker)
/// and for any id outside the band.
pub fn status_element_label(sprite: u8) -> &'static str {
    match sprite {
        0x18 => "VNM",
        0x19 => "TOX",
        0x1A => "STN",
        0x1B => "ROT",
        0x1C => "CNF",
        0x1D => "NMB",
        0x1E => "SLP",
        0x1F => "CRS",
        // Retail's own badge cell for this id reads **Faint** - the word is
        // legible in a battle frame's framebuffer (a party member at 0 HP
        // wears it where the `LV` label would be). The tag stands in for that
        // cell, so it carries the same word rather than a "K.O." of the
        // port's own invention.
        0x20 => "Faint",
        _ => "",
    }
}

/// Badge colour for a retail status-element sprite id. Engine-chosen (the
/// retail sheet's palette is not resolved) but grouped by what the ailment
/// does: violet for the two poisons, grey for the inert pair (Stone / Numb),
/// blue for Sleep, magenta for the delegation group - which is the colour
/// family `FUN_8004A908` tints the *actor* mesh with for the same three
/// masks - amber for Rot, cyan for Curse, dim for K.O.
pub fn status_element_color(sprite: u8) -> [f32; 4] {
    match sprite {
        0x18 => [0.72, 0.45, 0.95, 1.0], // Venom
        0x19 => [0.85, 0.30, 0.90, 1.0], // Toxic
        0x1A => [0.62, 0.60, 0.55, 1.0], // Stone
        0x1B => [1.0, 0.72, 0.20, 1.0],  // Rot
        0x1C => [0.94, 0.35, 0.94, 1.0], // Confuse
        0x1D => [0.70, 0.70, 0.78, 1.0], // Numb
        0x1E => [0.45, 0.65, 1.0, 1.0],  // Sleep
        0x1F => [0.40, 0.90, 0.95, 1.0], // Curse
        0x20 => [0.55, 0.30, 0.30, 1.0], // K.O.
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

/// Retail HP-bar text colour index for a battle slot.
///
/// PORT: FUN_800349EC - returns the font-CLUT colour index the retail battle
/// HUD tints a character's HP readout with, keyed on the cur/max ratio. Index 2
/// is empty/K.O.; index 9 is danger (`cur <= max/4`); index 6 is caution
/// (`cur <= max/2`, or any time a status flag is set); index 7 is normal. The
/// thresholds use the same floored `max >> 2` / `max >> 1` comparisons as retail.
///
/// `status_active` models retail's per-character status byte (record `+0x36`,
/// `*(short *)(char*0x414 - 0x7ff7b7ca)`), which forces the caution tier even
/// above half HP; the engine approximates it with "any active status icon".
///
// Reached on both hosts through [`battle_hud_draws_for`]: the native window
// (`engine-shell/.../window/hud.rs`) and the browser play page
// (`web-play/src/play_battle.rs`) call it every battle frame.
pub fn hp_bar_color_index(cur: u16, max: u16, status_active: bool) -> u8 {
    if cur == 0 {
        return 2;
    }
    if (max >> 2) < cur {
        if status_active || cur <= (max >> 1) {
            6
        } else {
            7
        }
    } else {
        9
    }
}

/// Retail MP-bar text colour index for a battle slot.
///
/// PORT: FUN_80035EA8 - the MP sibling of [`hp_bar_color_index`]. Same
/// `cur <= max/4` / `cur <= max/2` ratio tiers (index 9 danger, 6 caution,
/// 7 normal) but with no K.O. (2) state and no status-flag override - MP has no
/// "empty = dead" colour, so a depleted bar simply reads as danger.
///
// Same reach as [`hp_bar_color_index`] - see the note there. The MP field it
// tints is drawn only when the slot carries a non-zero `mp_max`, which in the
// native window means party rows: `World` keeps the MP ceiling in
// `character_max_mp` (keyed by battle ordinal) and monsters have none.
pub fn mp_bar_color_index(cur: u16, max: u16) -> u8 {
    if (max >> 2) < cur {
        if cur <= (max >> 1) { 6 } else { 7 }
    } else {
        9
    }
}

/// Locate a solid-white, fully-opaque texel in the font atlas and return it
/// as a 1x1 `src` rect for [`TextDraw`].
///
/// This is what gives both hosts a **filled-rect primitive** without any new
/// pipeline: every HUD draw already samples the font atlas with a colour
/// multiply (`texel * color` on native, the canvas multiply-tint blit on the
/// play page), so a quad whose source is one pure-white texel is a solid
/// rect of the draw's own colour at any destination size. Safe under nearest
/// sampling too - a 1x1 source spans exactly one texel, so no fragment can
/// reach a neighbour.
///
/// Both the extracted retail font (fill texels whitewashed to `0xFF`) and
/// the placeholder font (white 5x7 strokes) carry such a texel; `None` only
/// on a custom atlas with no opaque white anywhere, in which case the bar
/// builders degrade to text-only output.
pub fn font_solid_src(font: &legaia_font::Font) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = font.atlas_dimensions();
    let rgba = font.atlas_rgba();
    for y in 0..h {
        for x in 0..w {
            let off = ((y * w + x) * 4) as usize;
            if rgba[off] == 0xFF
                && rgba[off + 1] == 0xFF
                && rgba[off + 2] == 0xFF
                && rgba[off + 3] == 0xFF
            {
                return Some((x, y, 1, 1));
            }
        }
    }
    None
}

/// Locate a fully-**opaque** texel in an arbitrary RGBA8 sprite atlas and
/// return it as a 1x1 `src` rect.
///
/// The sibling of [`font_solid_src`] for the sprite layer. It does not care
/// what colour the texel is: the overlay shader is
/// `color.rgb * texel.rgb, color.a * texel.a`, so a draw whose colour is
/// black yields black at the colour's own alpha whatever the source pixel
/// was. Only the alpha has to be `0xFF`.
///
/// This is what lets a host put a translucent plate **under** sprites drawn
/// from the same atlas: both halves of the overlay are one draw list, and
/// within it order is preserved - whereas the text half always composites on
/// top of every sprite half, so a plate emitted there would darken the very
/// readout it backs.
pub fn atlas_opaque_texel(rgba: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    for y in 0..height {
        for x in 0..width {
            let off = ((y * width + x) * 4) as usize;
            if rgba.get(off + 3).copied() == Some(0xFF) {
                return Some((x, y, 1, 1));
            }
        }
    }
    None
}

/// RGBA for a retail gauge / readout tint index.
///
/// The index space is retail's (`FUN_80046A20` / the readout-tint pair
/// `FUN_800349EC` / `FUN_80035EA8`: `2` dead, `3` status override, `7` high,
/// `6` mid, `9` low), and so are the **colours** - they are no longer
/// approximations.
///
/// Retail resolves a tier by *selecting a whole font CLUT*: the drawn
/// numerals' palette byte is `tier + 6`, whose 16-entry CLUT sits at VRAM
/// `(16 * (tier + 6), 510)`, and the glyph body is that CLUT's **entry 15**.
/// Read straight out of a retail battle frame's VRAM (a party frame with one
/// member at 0 HP, two in the danger tier), which pins five of the six rows
/// at once:
///
/// | tier | palette | VRAM x | entry 15 | reads as |
/// |---|---|---|---|---|
/// | 2 empty / K.O. | 8 | 128 | `(230, 32, 0)` | red |
/// | 3 status lock | 9 | 144 | `(230, 106, 230)` | magenta |
/// | 6 caution | 12 | 192 | `(230, 172, 0)` | amber |
/// | 7 normal | 13 | 208 | `(205, 205, 205)` | light grey |
/// | 9 danger | 15 | 240 | `(222, 90, 0)` | orange |
///
/// Note tier 2 is **red**, not a grey-out: retail paints a downed member's
/// whole readout in the brightest colour on the strip. The port used to grey
/// it, which read as "this panel is disabled" rather than "this member is
/// down".
pub fn gauge_fill_color(idx: u8) -> [f32; 4] {
    match idx {
        // Each tier's entry 15 on the 5-bit grid (the CLUT is 5-bit).
        2 => crate::MENU_TEXT_RED,
        3 => [
            legaia_render_kernels::ui_tint::ink_component(28),
            legaia_render_kernels::ui_tint::ink_component(13),
            legaia_render_kernels::ui_tint::ink_component(28),
            1.0,
        ],
        6 => crate::MENU_TEXT_GOLD,
        9 => crate::MENU_TEXT_ORANGE,
        _ => READOUT_NORMAL,
    }
}

/// Retail's "normal" readout colour - entry 15 of the tier-7 CLUT, the 5-bit
/// `25` that a retail frame shows as `(206, 206, 206)`. It is also the colour
/// every **name** and caption on the battle screen is drawn in, downed
/// members included; see [`gauge_fill_color`]. The same ink as the menus'
/// [`MENU_TEXT_WHITE`](crate::MENU_TEXT_WHITE): one CLUT.
pub const READOUT_NORMAL: [f32; 4] = crate::MENU_TEXT_WHITE;

/// The modulation that draws a **numeral cell** in `ink`.
///
/// The 8x12 numeral cells and the battle strip's `/` are decoded under the
/// tier-7 CLUT (sub-palette 13, `legaia_ui_formats::title_pak::MENU_GLYPH_ATLAS_TEXT_CLUT_ROW`),
/// so their texels already carry that CLUT's ink and outline. Retail picks a
/// tier by selecting another CLUT, not by modulating, so the normal tier
/// draws them unmodulated; another tier scales the baked ink onto its own.
/// Multiplying the cell by the ink itself drew the normal readout at
/// `206 * 206 / 255 = 166` with a `27` outline where retail shows `206` / `33`.
pub fn numeral_cell_tint(ink: [f32; 4]) -> [f32; 4] {
    let base = READOUT_NORMAL;
    [ink[0] / base[0], ink[1] / base[1], ink[2] / base[2], ink[3]]
}

/// Everything one battle-HUD frame draws, plus the chrome inputs the
/// retail-shaped strip needs. Bundled so [`battle_hud_draws_for`] stays under
/// clippy's argument-count threshold as the surface grows.
#[derive(Default)]
pub struct BattleHudFrame<'a> {
    /// Per-slot rows, indexed by **absolute actor-table slot** (party
    /// `0..party_count`, monsters above). Inactive slots are empty-name
    /// entries the builder skips - compacting would mis-anchor popups.
    pub slots: &'a [HudSlotView<'a>],
    pub popups: &'a [HudPopupView],
    pub log: &'a [HudLogView<'a>],
    /// 1x1 solid-white atlas rect from [`font_solid_src`]. `None` degrades
    /// the strip to text-only (no filled rects).
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Surface size in pixels. The party strip is laid out on the retail
    /// 320x240 stage and integer-upscaled + centred into this surface, the
    /// same transform the boot/menu stage uses.
    pub surface: (u32, u32),
    /// System-UI atlas rects for the strip chrome (gradient fill + gold
    /// 9-slice frame) and the gold/green HP / MP label sprites
    /// (`OVERLAY_SYSTEM_UI_LABEL_HP` / `_MP`, `legaia_ui_formats::title_pak`).
    /// `None` degrades the chrome to solid-texel rects and the labels to
    /// tinted "HP" / "MP" text.
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    /// Top-left announcement plaque label. Retail draws the **acting
    /// actor's** name in a framed plaque at the top-left corner - "Vahn"
    /// while Vahn's item lands (`captures/tetsu_idle`), "Tetsu" through the
    /// enemy's rush - so hosts pass
    /// `engine-battle-hud::battle_plaque_label`. The plaque *skin* is
    /// not yet pinned; the label draws as clean text at the plaque's
    /// captured text inset until it is.
    pub plaque: Option<&'a str>,
    /// Element-badge index (`0..8`) the plaque wears in front of the name.
    /// Retail's plaque interior is `20 + 5 + name width` when the actor
    /// carries one, which is the second half of the pinned plaque law
    /// (`battle_chrome::name_plaque`). `None` draws the bare name.
    pub plaque_badge: Option<u8>,
    /// A battle message holding the **top-of-screen banner**. Retail's
    /// banner and the actor-name plaque are one seat (content pen
    /// `(16, 12)`), so this wins: while a message is up the builder draws
    /// the class-0 frame and its text and emits no plaque.
    pub banner: Option<&'a str>,
    /// The host is already drawing its own box on that seat (the sparring
    /// tutorial prompt). Suppresses the plaque without drawing a banner -
    /// two text runs on one pen is the artifact this exists to stop.
    pub plaque_seat_taken: bool,
    /// Atlas cells for the status-element and element badges
    /// ([`crate::battle_hud_chrome::BattleBadgeRects`]). Any `None` cell
    /// falls back to the engine's labelled text tag.
    pub badges: Option<&'a crate::battle_hud_chrome::BattleBadgeRects>,
    /// The roster panels (placement records 6 / 78 / 79) are parked this
    /// frame - retail's `06/1 4E/1 4F/1`, which sends the cluster to
    /// `y = 236` under the 228-line display window. The engine stage is 240
    /// lines, so the builder emits no panel draws instead. Hosts pass the
    /// inverse of `engine-battle-hud::battle_panels_visible`, which
    /// carries the per-phase rule (up at the round prompt and while an item
    /// / magic window is browsed, parked by the ring and every action but a
    /// party-wide one).
    pub panels_parked: bool,
    /// Rows the roster panels sit below their rest seat this frame, while
    /// the round prompt's open glide raises them from seat A
    /// (`engine-battle-hud::battle_round_prompt_panels_dy`): records
    /// 6 / `0x4E` / `0x4F` spawn at `y = 236` and walk to `170`. `0` at
    /// rest.
    pub panels_dy: i32,
    /// The `Begin` breadcrumb tab is up at `(16, 12)` and the plaque sits
    /// behind it at `(68, 12)` - retail's placement records 1 and 26, the
    /// shape every command-entry step from the ring on carries
    /// (`engine-battle-hud::battle_begin_tab_visible`). Without it the
    /// plaque takes its action seat `(16, 12)` (record 68).
    pub begin_tab: bool,
    /// The `Begin` tab's offset from its seat while it walks there from the
    /// round prompt's `Begin` seat, as the ring opens
    /// (`engine-battle-hud::battle_begin_tab_offset`). `(0, 0)` at
    /// rest.
    pub begin_tab_offset: (i32, i32),
    /// The third breadcrumb tab behind `Begin | <name>` - the chosen ring
    /// arm's chip glided onto the trail (record `0x0D` `Attack` from the
    /// attack-mode prompt through the arts entry, record `0x0E` the Ra-Seru
    /// name through the spell window;
    /// `engine-battle-hud::battle_breadcrumb_third_tab`). Drawn only
    /// with [`Self::begin_tab`], abutting the plaque's right cap.
    pub third_tab: Option<&'a str>,
    /// The art / spell / item name retail draws under the action
    /// (placement records 76 / 77): centred on `x = 0xA0` by
    /// [`crate::battle_name_banner::banner_x`], pen row `148`.
    pub move_name: Option<&'a str>,
    /// The bottom-right target plaque (placement record 81): the monster a
    /// party member's action is aimed at and its element badge, on a blue
    /// plate whose right cap ends at `x = 312`, on the bar's row.
    pub target_plaque: Option<(&'a str, Option<u8>)>,
    /// The actor-name plaque's offset below its rest seat while its raise
    /// glide runs (`engine-battle-hud::battle_action_plaque_dy`):
    /// negative from the seed's raise until it lands. `0` at rest.
    pub plaque_dy: i32,
    /// The target plaque's offset below its rest seat while its raise glide
    /// runs (`engine-battle-hud::battle_target_plaque_dy`).
    pub target_plaque_dy: i32,
    /// The active-actor bar's offset below its rest seat while its raise
    /// glide runs (`engine-battle-hud::battle_readout_bar_dy`):
    /// positive from the seed's raise until it lands. `0` at rest.
    pub bar_dy: i32,
    /// The **target-select** plaque (placement record `0x29`): the monster
    /// the open target cursor rests on, and its element badge, on the blue
    /// plate class at retail's seat - content box centred on `x = 0xE8`,
    /// pulled left to end by `0x130`, row `162`
    /// (`crate::battle_chrome::target_select_plaque_x`). `None`
    /// unless a picker's cursor sits on the enemy row
    /// (`engine-battle-hud::battle_target_select_plaque`).
    pub target_select: Option<(&'a str, Option<u8>)>,
    /// The full-width message bar (HUD element `0x5B`, placement record 91 -
    /// `(16, 236)` sliding to `(16, 194)`, width 288): the steal / stolen-item
    /// caption `engine-battle-hud::battle_message_bar` names. It takes
    /// the active-actor bar's own seat, so while it is up neither that bar
    /// nor the target plaque on the same row draws
    /// (`player_steal_skeleton_banner` shows the caption alone on the row).
    pub message_bar: Option<&'a str>,
    /// The message bar's offset below its rest seat while its raise glide
    /// runs (`engine-battle-hud::battle_message_bar_dy`): positive from the
    /// raise until it lands. `0` at rest.
    pub message_bar_dy: i32,
    /// The ring's AP plate (placement record 82 at `(208, 172)`) and the
    /// value it shows - the acting member's Spirit gauge - or `None` when
    /// the plate is off. Drawn from the status screen's own gauge pieces,
    /// so it needs [`Self::chrome`].
    pub ap_plate_value: Option<u8>,
    /// How far right of its seat the AP plate sits while it slides in with
    /// the ring (`engine-battle-hud::battle_ring_ap_plate_dx`). `0` at
    /// rest.
    pub ap_plate_dx: i32,
    /// Stage rect (`x, y, w, h`) of a box the **host** draws on top of the
    /// battle screen this frame - today the sparring-tutorial prompt, whose
    /// rect is the retail emitter's own (`battle_tutorial::BoxStyle::box_rect`).
    ///
    /// Retail's bottom-anchored prompt styles anchor at `y = 0xCC / 0xB0 /
    /// 0x9A`, and a one-line style-2/3 box therefore lands on `188..212` -
    /// exactly the rows the active-actor bar occupies, and inside the roster
    /// panels' `164..212`. Drawing both puts two text runs on the same
    /// pixels, the same defect the plaque seat already has an exclusive
    /// branch for; the builder resolves it the same way and omits whichever
    /// party surface the box's drawn footprint covers.
    ///
    /// This is the box's **centre** rect: the drawn window skin extends
    /// [`HOST_BOX_SKIN`] px beyond it on every side (the reading box's
    /// 9-slice inflation), and that inflated rect is what is tested.
    ///
    /// `None` (the default) leaves both surfaces alone.
    pub host_box: Option<(i32, i32, i32, i32)>,
    /// Actor-table slot of the actor the frame belongs to
    /// (`engine-battle-hud::battle_active_actor`). When it names a live
    /// party member, retail replaces that member's resting panel readout with
    /// the full-width active-actor bar; hosts also pass the same actor's name
    /// as [`Self::plaque`].
    pub active_slot: Option<u8>,
    /// The **commit log** - one row per member who has committed a command
    /// this round (`engine-battle-hud::battle_commit_log`), laid out by
    /// `legaia_engine_battle_vm::battle_commit_log` and drawn as gold plates.
    pub commit_log: &'a [legaia_engine_battle_vm::battle_commit_log::CommitLogRow],
    /// The battle-intro enemy-name labels, `(label, pen x)`, pen y
    /// [`BATTLE_INTRO_NAME_Y`] (`engine-battle-hud::battle_intro_names`,
    /// retail `FUN_801D9D3C`). Empty outside the intro timer.
    pub intro_names: &'a [(String, i32)],
    /// The boss-name banner a battle-stage module raises, `(label, pen x,
    /// pen y)` (`engine-battle-hud::battle_stage_banner`, PROT 0968's
    /// phase 5): a bare text actor, no frame. `None` outside it.
    pub stage_banner: Option<(&'a str, i32, i32)>,
    /// Diagnostic readout ([`diag_hud_enabled`]): the engine's debug rows -
    /// monster HP numerals/bars (retail draws **no** monster gauge,
    /// `docs/subsystems/battle-action.md`), per-slot LV / AP readouts, and
    /// the base-marker + level element (`FUN_8002C2E4`'s no-ailment arm,
    /// whose retail widget placement is not pinned). Off by default.
    pub diag: bool,
}

/// The two draw lists one battle-HUD frame produces: `text` samples the
/// dialog-font atlas (glyphs + solid-texel rects), `sprites` samples the
/// resident system-UI atlas (strip chrome + HP/MP label sprites). Hosts
/// composite `sprites` under `text`, the same layering as the dialog and
/// menu chrome.
#[derive(Default)]
pub struct BattleHudDraws {
    pub text: Vec<TextDraw>,
    pub sprites: Vec<SpriteDraw>,
}

/// Shared host toggle for the diagnostic battle readout (and the engine's
/// "ENCOUNTER!" transition banner, which has no retail counterpart): set
/// `LEGAIA_DIAG_HUD` to anything but `0`/empty. Reads the environment so
/// both hosts resolve the same answer; on wasm the variable never exists
/// and the diagnostics stay off.
pub fn diag_hud_enabled() -> bool {
    std::env::var("LEGAIA_DIAG_HUD")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

#[cfg(test)]
mod ink_tests {
    use super::*;

    /// Retail's battle and menu text is CLUT-7 grey on screen, `206`, never
    /// white (`rim_elm_gimard_victory`'s plaques, `v0_1_battle_command_submenu`'s
    /// chips, `menu_equipment_field`'s rows).
    #[test]
    fn the_normal_ink_is_the_tier_7_grey() {
        assert_eq!(READOUT_NORMAL, crate::MENU_TEXT_WHITE);
        assert_eq!((READOUT_NORMAL[0] * 255.0).round() as u8, 206);
        assert_eq!(crate::battle_command_ui::LABEL_INK, READOUT_NORMAL);
        // The options screen stages the same three CLUT rows as every menu.
        assert_eq!(crate::OPTIONS_INK_WHITE, crate::MENU_TEXT_WHITE);
        assert_eq!(crate::OPTIONS_INK_GOLD, crate::MENU_TEXT_GOLD);
        assert_eq!(crate::OPTIONS_INK_TEAL, crate::MENU_TEXT_TEAL);
    }

    /// A numeral cell is decoded under the tier-7 CLUT, so the normal tier
    /// draws it unmodulated, and another tier lands its own entry-15 ink on
    /// the baked `206`.
    #[test]
    fn numeral_cells_draw_the_normal_tier_unmodulated() {
        assert_eq!(numeral_cell_tint(READOUT_NORMAL), [1.0; 4]);
        let gold = gauge_fill_color(6);
        let t = numeral_cell_tint(gold);
        for k in 0..3 {
            assert!((t[k] * READOUT_NORMAL[k] - gold[k]).abs() < 1e-6);
        }
        assert_eq!(t[3], gold[3]);
    }

    /// Every caption the battle builder lays out - the actor plaque and the
    /// move name here - carries the tier-7 ink, not white.
    #[test]
    fn battle_captions_draw_in_the_tier_7_ink() {
        let font = legaia_font::synthetic_for_tests();
        let draws = battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                surface: (320, 240),
                plaque: Some("Vahn"),
                move_name: Some("Somersault"),
                ..Default::default()
            },
            (0, 0),
        );
        assert!(!draws.text.is_empty());
        for d in &draws.text {
            assert_eq!(d.color, READOUT_NORMAL, "{d:?}");
        }
    }
}
