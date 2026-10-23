//! Special-use items (Door of Light / Door of Wind / Incense): use routes, warp destinations and the confirm session.

use super::*;

/// The window-14 target-panel preview mode for a picked item - the
/// retail preview word `DAT_801E46CC` derivation: only an item whose
/// record kind byte (`0x80074368 + id*0xC + 0`) is `2` **and** whose
/// item-effect class (`0x800752C0 + eff*4 + 0`) is `6` (the
/// permanent-stat Waters) previews; the effect arg (`+1`) maps `0 -> 1`
/// (Life Water), `1 -> 2` (Power Water / ATK), `2 -> 3` (Guardian
/// Water / UDF+LDF), `3 -> 4` (Swift Water / SPD), `4 -> 5` (Wisdom
/// Water / INT), `5 -> 1` (Magic Water shares the HP/MP panel).
/// Everything else is mode `0` - the plain `cur/max` panel.
///
/// PORT: FUN_801D6A54 (target-panel preview-mode derivation)
///
/// Driven from [`target_panel_view_model_of`], the host entry point for the
/// window-14 panel: the staged bag id resolves the item record's kind
/// byte and effect descriptor through the world's disc-parsed
/// [`legaia_game_tables::item_effect::ItemEffectTable`].
pub fn target_panel_mode(item_kind: u8, effect_class: u8, effect_arg: u8) -> u32 {
    if item_kind != 2 || effect_class != 6 {
        return 0;
    }
    match effect_arg {
        0 | 5 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        _ => 0,
    }
}

/// Fixed bag ids the three special Use routes consume (`FUN_80042310` /
/// `FUN_80043048` calls with literal ids in the submenu handlers).
pub const DOOR_OF_LIGHT_ITEM_ID: u8 = 0x88;
pub const DOOR_OF_WIND_ITEM_ID: u8 = 0x89;
pub const INCENSE_ITEM_ID: u8 = 0x8A;

/// Menu exit codes the special routes hand to the outer menu state
/// machine (`_DAT_8007B43C`, with the `DAT_801E46A0 = 0xF2` fade): `4` =
/// the Door of Light dungeon-escape handoff, `5` = the Door of Wind
/// world-map warp.
pub const MENU_EXIT_CODE_FIELD_ESCAPE: u32 = 4;
pub const MENU_EXIT_CODE_WORLD_MAP_WARP: u32 = 5;

/// VA of the Door of Light prompt `FUN_801D1DAC` (window 10) draws at
/// `(WX, WY)` (`lui a0,0x801d; addiu a0,a0,-0x156c` at `0x801D1DB8`).
pub const DOOR_OF_LIGHT_PROMPT_VA: u32 = 0x801C_EA94;
/// VAs of the Incense prompt's two lines `FUN_801D1F10` (window 12) draws
/// at `(WX, WY)` and `(WX + 0xC, WY + 0xE)` (`0x801D1F1C` / `0x801D1F50`).
/// The first carries a `0xC2` item-name token naming Incense.
pub const INCENSE_PROMPT_VAS: [u32; 2] = [0x801C_EAB4, 0x801C_EAC8];

/// One menu-overlay pool string as bytes: from `va` to its NUL, the
/// two-byte `0xC0..=0xCF` tokens kept whole so a substitution survives for
/// the caller to expand. `None` when `va` is outside the image.
///
/// The pool's leading `@` is kept: neither the expander `FUN_80036514` nor
/// the renderer `FUN_80036888` treats it specially, and it is a blank glyph
/// of width `0`, so retail draws it as a one-pixel advance
/// (`widths[c] + pad + 1`) before the first visible glyph.
pub fn menu_overlay_bytes(overlay: &[u8], va: u32) -> Option<Vec<u8>> {
    // A string a language pack moved is read where the code now finds it.
    let va = legaia_game_tables::menu_string_refs::resolve_va(overlay, va);
    let off = va.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let body = overlay.get(off..)?;
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&b) = body.get(i) {
        if b == 0 {
            break;
        }
        out.push(b);
        if b & 0xF0 == 0xC0 {
            out.extend(body.get(i + 1).copied());
            i += 2;
        } else {
            i += 1;
        }
    }
    Some(out)
}

/// VA of window 7's sentence (`FUN_801DCCB4`, `lui a0,0x801e; addiu
/// a0,a0,0x46e4` at `0x801DCCC0`): a `0xC3` spell-name token whose operand
/// byte the renderer patches with the leveled spell's id (`sb v1,0x1(a0)` at
/// `0x801DCD14`) before the rest of the line.
pub const SPELL_LEVEL_NOTICE_TEMPLATE_VA: u32 = 0x801E_46E4;

/// Window 7's sentence with its spell-name token expanded, as font bytes.
///
/// `template` is the raw sentence ([`menu_overlay_bytes`] at
/// [`SPELL_LEVEL_NOTICE_TEMPLATE_VA`]); each `0xC3 xx` token is replaced by
/// `spell_name_bytes(spell_id)` - retail's `FUN_80036514` splices the spell
/// table's raw `name_ptr` string there (its `0xC3` arm reads
/// `0x800754C8 + id*12 + 8`), so the element plate's `0xCE` escape comes
/// with it. The renderer has already patched the operand, so `xx` is
/// ignored and `spell_id` used. Any other token is kept whole.
pub fn spell_level_notice_line(
    template: &[u8],
    spell_id: u8,
    spell_name_bytes: impl Fn(u8) -> Option<Vec<u8>>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&b) = template.get(i) {
        if b == 0xC3 {
            out.extend(spell_name_bytes(spell_id).unwrap_or_default());
            i += 2;
        } else if b & 0xF0 == 0xC0 {
            out.push(b);
            out.extend(template.get(i + 1).copied());
            i += 2;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// The special Use routes' own prompt lines, read off one PROT 0899 image:
/// the strings `FUN_801D1DAC` (Door of Light, window 10) and `FUN_801D1F10`
/// (Incense, window 12) draw above their Yes / No rows. Kept as raw bytes
/// so the Incense line's `0xC2` item-name token can be expanded against the
/// item table at draw time ([`Self::lines`]). Empty until a host installs
/// the overlay - the windows then draw no prompt text rather than invented
/// text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseRoutePrompts {
    pub door_light: Vec<u8>,
    pub incense: [Vec<u8>; 2],
}

impl UseRoutePrompts {
    /// Read both routes' lines out of a PROT 0899 image.
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let s = |va: u32| menu_overlay_bytes(overlay, va).unwrap_or_default();
        Self {
            door_light: s(DOOR_OF_LIGHT_PROMPT_VA),
            incense: [s(INCENSE_PROMPT_VAS[0]), s(INCENSE_PROMPT_VAS[1])],
        }
    }

    /// `true` once a host has installed real disc text.
    pub fn is_installed(&self) -> bool {
        !self.door_light.is_empty() || !self.incense[0].is_empty()
    }

    /// `route`'s prompt lines with each `0xC1..=0xC7` token expanded by
    /// `name(token, operand)` (retail's `FUN_80036514` splice; the Incense
    /// line's `0xC2 0x8A` is the item name). The Door of Wind route has no
    /// prompt window. Empty when nothing is installed.
    pub fn lines(&self, route: UseRoute, name: impl Fn(u8, u8) -> Option<String>) -> Vec<String> {
        let raw: Vec<&[u8]> = match route {
            UseRoute::DoorOfLight => vec![&self.door_light],
            UseRoute::Incense => vec![&self.incense[0], &self.incense[1]],
            _ => Vec::new(),
        };
        raw.into_iter()
            .filter(|l| !l.is_empty())
            .map(|l| crate::shop::expand_name_escapes(l, &name))
            .collect()
    }
}

/// The item-effect **class byte** (`0x800752C0 + eff*4 + 0`) of the three
/// special Use items. These three ids are the only ones retail's dispatch
/// (`FUN_801D7E50`) sends to a dedicated submenu, and each one's class is
/// fixed disc data, so the engine can key the class off the id while
/// [`PauseItemRow`] carries no effect record of its own.
fn special_use_effect_class(item_id: u8) -> Option<u8> {
    match item_id {
        DOOR_OF_LIGHT_ITEM_ID => Some(0x80),
        DOOR_OF_WIND_ITEM_ID => Some(0x81),
        INCENSE_ITEM_ID => Some(0x82),
        _ => None,
    }
}

/// Which of the three special Use routes - if any - a bag id opens.
///
/// All three route out of the ordinary target-panel flow at the same place
/// (`FUN_801D7E50` phase 2), and they differ only in the screen they raise:
/// Door of Light raises the Yes/No window 10 (`FUN_801D1DAC`), Incense the
/// Yes/No window 12 (`FUN_801D1F10`), and Door of Wind the destination
/// **list** window 11, driven by the kind-4 list kernel rather than a
/// picker. [`SpecialUseSession::new`] is what turns the route into the
/// right opening phase, so all three belong here.
///
/// The route itself comes from [`use_route_for_effect`] - the ported
/// dispatch is the decision point, and this wrapper only supplies the
/// class byte.
pub fn special_use_route_for_item(item_id: u8) -> Option<UseRoute> {
    // The all-party flag byte is irrelevant on this path: the three class
    // bytes below are all matched before `use_route_for_effect` looks at
    // it, which is why 0 is a faithful stand-in for the record's `+2`.
    match use_route_for_effect(special_use_effect_class(item_id)?, 0) {
        route @ (UseRoute::DoorOfLight | UseRoute::DoorOfWind | UseRoute::Incense) => Some(route),
        // The two generic apply routes are not special-route screens.
        _ => None,
    }
}

/// One row of the Door of Wind destination list - a **visible** placement
/// record from the quick-travel table (`DAT_80073A98`, 6-byte stride;
/// parser [`legaia_game_tables::worldmap_menu`]).
///
/// The visible set is built by the same walk the world-map landmark menu
/// runs (`FUN_80030628` case `0x19`, `0x80031870..0x800318dc`): each record
/// is skipped when its `name_idx` repeats the last **accepted** row's, then
/// gated on the system flag at `record[1] + 0x20` (`FUN_8003CE64`); an
/// accepted row is pushed as the string id `0x8000 | record_index`, which
/// `FUN_8002FF8C` resolves back through `names[placement[index].name_idx]`.
/// [`Self::record_index`] is that record ordinal - retail's
/// `_DAT_8007BB88`, which `FUN_801D8B90` phase 3 scales by 6 straight back
/// into the same table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarpDestination {
    /// Ordinal of the placement record in `DAT_80073A98` (**not** the
    /// visible row ordinal - locked landmarks leave gaps).
    pub record_index: u32,
    /// Landmark name from `DAT_80073B18`.
    pub name: String,
    /// Destination scene id, record `+2`. Retail stages it into the
    /// world-state word `0x80084628`.
    pub scene_id: u16,
    /// World-map marker x, record `+4` -> `0x80084624`.
    pub menu_x: u8,
    /// World-map marker y, record `+5` -> `0x8008462C`.
    pub menu_y: u8,
}

/// The destination a committed Door of Wind use staged, in the shape
/// `FUN_801D8B90` phase 3 writes it (`0x80084624` / `0x80084628` /
/// `0x8008462C`) before handing the outer menu SM exit code
/// [`MENU_EXIT_CODE_WORLD_MAP_WARP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedWarp {
    pub scene_id: u16,
    pub menu_x: u8,
    pub menu_y: u8,
}

/// Which submenu a confirmed Use-list pick routes to - the
/// `FUN_801D7E50` phase-2 dispatch on the picked item's effect class
/// (`801d7f80..801d7fd8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseRoute {
    /// Effect flag bit `0x20` set (all-party): submenu 9
    /// (`FUN_801D7FF8`) - the target panel opens in all-row hand mode
    /// with no row navigation.
    ApplyAll,
    /// Default route: submenu 0xA (`FUN_801D8308`) - single-target pick
    /// over the party rows.
    ApplySingle,
    /// Effect class `0x80` (Door of Light): submenu 0xB
    /// (`FUN_801D8A58`).
    DoorOfLight,
    /// Effect class `0x81` (Door of Wind): submenu 0xC
    /// (`FUN_801D8B90`).
    DoorOfWind,
    /// Effect class `0x82` (Incense): submenu 0xD (`FUN_801D8D94`).
    Incense,
}

/// Route a confirmed Use pick by its item-effect record: class byte
/// `0x80`/`0x81`/`0x82` take the dedicated flows; anything else goes to
/// the all-party apply when the flag byte (`+2`) has bit `0x20`, else
/// the single-target apply.
///
/// Driven from [`special_confirm_route_for_item`], which every Use-list
/// confirm runs. The `ApplyAll` / `ApplySingle` split it also decides is
/// not consulted there: the engine's target shape comes from the inner
/// [`InventoryUseSession`], which reads its own catalog rather than the
/// item-effect flag byte.
///
/// PORT: FUN_801D7E50 (Use-list phase-2 effect-class dispatch)
pub fn use_route_for_effect(effect_class: u8, effect_flags: u8) -> UseRoute {
    match effect_class {
        0x80 => UseRoute::DoorOfLight,
        0x81 => UseRoute::DoorOfWind,
        0x82 => UseRoute::Incense,
        _ if effect_flags & 0x20 != 0 => UseRoute::ApplyAll,
        _ => UseRoute::ApplySingle,
    }
}

/// Terminal result of a special Use route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialUseOutcome {
    /// Backed out - retail returns to the Use list (submenu 6) without
    /// consuming anything.
    Cancelled,
    /// Door of Light confirmed: one `0x88` consumed; the menu closes
    /// with exit code [`MENU_EXIT_CODE_FIELD_ESCAPE`] (the field-side
    /// dungeon-escape handoff).
    FieldEscape,
    /// Door of Wind destination picked: one `0x89` consumed; the menu
    /// closes with exit code [`MENU_EXIT_CODE_WORLD_MAP_WARP`].
    /// `landmark` indexes the quick-travel placement table (retail
    /// `0x80073A98`, 6-byte records - `legaia_game_tables::worldmap_menu`);
    /// retail stages record `+2`/`+4`/`+5` into the world-state words
    /// `0x80084628`/`0x80084624`/`0x8008462C` before the handoff.
    Warp { landmark: usize },
    /// Incense confirmed: one `0x8A` consumed and the class-`0x82`
    /// encounter-suppression effect applied through the SCUS item-effect
    /// applier (`FUN_800402F4`); the flow drops back to the Use list.
    EncounterSuppress,
}

/// Phase of a [`SpecialUseSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialUsePhase {
    /// Yes/No confirm (Door of Light / Incense). Unlike the Throw Out
    /// confirm, retail seeds the cursor to **0 - "Yes"**
    /// (`801d8ab4` / `801d8df0` zero `DAT_801E46D0`).
    Confirm,
    /// Door of Wind destination list (window 11, driven by the kind-4
    /// list kernel; the hand hides while the kernel idles).
    PickDestination,
    Done(SpecialUseOutcome),
}

/// State machine for the three special Use routes (submenus
/// 0xB / 0xC / 0xD). The session is pure routing - the host applies the
/// outcome (consume the fixed item id, close the menu with the exit
/// code, or apply the encounter suppression).
///
/// The three `PORT:` tags sit on the **arms** rather than here, and that
/// placement is load-bearing: a tag on the type resolves, for the runtime
/// reach join, to the type's first function - `new` - which every route
/// constructs. All three addresses would then read *entered* the moment any
/// one route ran, rather than each answering for its own arm.
pub struct SpecialUseSession {
    pub route: UseRoute,
    /// Destination names for the Door of Wind list (unlocked landmarks,
    /// in placement-table order).
    pub landmarks: Vec<String>,
    /// Confirm row (0 = Yes) or destination row.
    pub cursor: usize,
    pub phase: SpecialUsePhase,
}

impl SpecialUseSession {
    /// Start the route's flow. `DoorOfWind` opens the destination list;
    /// `DoorOfLight` / `Incense` open the Yes/No confirm seeded on Yes.
    /// (`ApplyAll` / `ApplySingle` are not special routes - they keep
    /// the target-panel flow and construct no session here.)
    pub fn new(route: UseRoute, landmarks: Vec<String>) -> Self {
        let phase = match route {
            UseRoute::DoorOfWind => SpecialUsePhase::PickDestination,
            _ => SpecialUsePhase::Confirm,
        };
        Self {
            route,
            landmarks,
            cursor: 0,
            phase,
        }
    }

    /// The fixed bag id the finished route consumed, if any.
    pub fn consumed_item_id(&self) -> Option<u8> {
        match &self.phase {
            SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape) => Some(DOOR_OF_LIGHT_ITEM_ID),
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { .. }) => Some(DOOR_OF_WIND_ITEM_ID),
            SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress) => Some(INCENSE_ITEM_ID),
            _ => None,
        }
    }

    /// The menu exit code the finished route hands to the outer menu SM
    /// (`_DAT_8007B43C`), if the route exits the menu.
    pub fn exit_code(&self) -> Option<u32> {
        match &self.phase {
            SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape) => {
                Some(MENU_EXIT_CODE_FIELD_ESCAPE)
            }
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { .. }) => {
                Some(MENU_EXIT_CODE_WORLD_MAP_WARP)
            }
            _ => None,
        }
    }

    /// Drive one frame from an edge-triggered PSX pad word.
    pub fn input_pad_edge(&mut self, pressed: u16) {
        match self.phase {
            SpecialUsePhase::Confirm => self.confirm_input(pressed),
            SpecialUsePhase::PickDestination => self.pick_destination_input(pressed),
            SpecialUsePhase::Done(_) => {}
        }
    }

    /// The Yes/No confirm window shared by the two confirm routes. One body,
    /// because retail's two routines differ only in which window descriptor
    /// they raise and which outcome the Yes row commits: Door of Light hands
    /// the field the escape exit code, Incense applies the class-`0x82`
    /// encounter suppression in place.
    ///
    /// PORT: FUN_801D8A58 (Door of Light confirm + exit-code 4 handoff)
    /// PORT: FUN_801D8D94 (Incense confirm + class-0x82 apply)
    fn confirm_input(&mut self, pressed: u16) {
        let up = pressed & PadButton::Up.mask() != 0;
        let down = pressed & PadButton::Down.mask() != 0;
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        if circle {
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Cancelled);
            return;
        }
        // FUN_801D688C over 2 rows with wrap.
        if up || down {
            self.cursor ^= 1;
        }
        if cross {
            self.phase = if self.cursor == 0 {
                match self.route {
                    UseRoute::Incense => {
                        SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress)
                    }
                    _ => SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape),
                }
            } else {
                // "No" confirms back to the Use list.
                SpecialUsePhase::Done(SpecialUseOutcome::Cancelled)
            };
        }
    }

    /// The Door of Wind destination list (window 11, driven by the kind-4 list
    /// kernel rather than a Yes/No picker) - retail submenu `0xC`, phases
    /// 2..3 of `FUN_801D8B90`.
    ///
    /// Reached from the Use list: [`special_use_route_for_item`] routes bag id
    /// `0x89` here, `legaia_engine_menu_runtime::field_menu_dispatch::build_pause_items_session`
    /// fills the rows from the disc placement table, and
    /// [`items_screen_model`] projects them through the shared list channel so
    /// both hosts draw the screen with the list renderer they already call.
    ///
    /// A pick warps: retail's phase 4 writes `_DAT_8007B43C = 5` and the
    /// outer menu SM acts on it; the port stages the destination on
    /// `legaia_engine_core::world::MenuState::pending_warp` and the world tick's
    /// menu-warp drain (`World::drain_staged_menu_warp`) resolves the staged
    /// scene word - a raw CDNAME TOC index - into the named scene
    /// transition the scene host consumes, seating the party at the
    /// record's tile. The bag decrement, the exit code and the staged
    /// triple are all committed by this screen.
    ///
    /// PORT: FUN_801D8B90 (Door of Wind destination list + exit-code 5 warp)
    fn pick_destination_input(&mut self, pressed: u16) {
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        if circle {
            // Retail restores the saved Use-list scroll
            // (`DAT_801EF070/74`) on the way back.
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Cancelled);
            return;
        }
        self.cursor = list_kernel_navigate(self.cursor, self.landmarks.len(), pressed);
        if cross && self.cursor < self.landmarks.len() {
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Warp {
                landmark: self.cursor,
            });
        }
    }
}
