"""Detector controls: the synthetic cases `--selftest` runs, and that every
ordinary run re-checks before trusting a verdict.
"""

from ui_host_drift.rows.frame_content import STD_METHOD_NAMES

# Detector control suite for `is_screen_signature`. Real signatures, copied
# from `crates/engine-ui/src` (multi-line ones flattened - the caller feeds it
# a raw source span either way).
SELFTEST_SCREENS: list[tuple[str, str]] = [
    # A model in, quads out: the ordinary screen shape.
    ("shop_draws_for",
     "pub fn shop_draws_for(font: &legaia_font::Font, title: &str, rows: &[ShopRow<'_>], "
     "cursor: usize, gold: Option<i32>, pen: (i32, i32)) -> Vec<TextDraw>"),
    # A glyph layout is a model of text, not a draw record - the sibling of
    # `sprite_draws_for` that must stay counted, or the surface loses a
    # screen fragment both hosts really do share.
    ("text_draws_for",
     "pub fn text_draws_for(layout: &legaia_font::Layout, pen: (i32, i32), "
     "color: [f32; 4]) -> Vec<TextDraw>"),
    # Sprite-returning screens are screens: the parameters are a model.
    ("equip_screen_sprites_for",
     "pub fn equip_screen_sprites_for(view: &EquipView<'_>, rects: &SaveMenuAtlasRects, "
     "origin: (i32, i32), scale: u32) -> Vec<SpriteDraw>"),
    # Tuple returns count too - the painter family returns quads beside
    # pictogram / cursor requests.
    ("sell_quantity_draws_for",
     "pub fn sell_quantity_draws_for(font: &legaia_font::Font, rect: PainterRect, "
     "selected: bool, heading: &str, quantity: u32, held: u32, unit_price: u32) "
     "-> (Vec<TextDraw>, Option<PainterPictogram>, Option<PainterSprite>)"),
    # Intermediate records are screens: a model in, a resolved screen out,
    # still needing the host's atlas or font to become quads. These four are
    # the whole reason the surface widened.
    ("persistent_hud_draws",
     "pub fn persistent_hud_draws(points: i32, best_points: i32, rod_index: u32, "
     "lure_count: i32) -> Vec<HudDraw>"),
    ("number_digit_cells",
     "pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell>"),
    ("bar_frame",
     "pub fn bar_frame(x: i32, y: i32, value: i32, segments: i32, style: i32) -> BarFrame"),
    ("equip_compare_panel_fields",
     "pub fn equip_compare_panel_fields(view: &EquipComparePanelView<'_>, "
     "pen: (i32, i32)) -> Vec<ComparePanelField>"),
    # Rect returns are screens: the projection stops one step earlier than a
    # quad, at the rect a caller then paints into. Both shapes are here
    # because the record-name rule matches neither, which is how the two below
    # went unwatched - not counted, and not waivable either.
    ("painter_rect",
     "pub fn painter_rect(descriptor: &MenuWindowDescriptor) -> PainterRect"),
    ("guarded_box_rect",
     "pub fn guarded_box_rect(x: i32, y: i32, w: i32, h: i32) "
     "-> Option<(i32, i32, i32, i32)>"),
]

SELFTEST_TRANSFORMS: list[tuple[str, str]] = [
    # The shape the return-type-only rule could not see.
    ("sprite_draws_for",
     "pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) "
     "-> Vec<SpriteDraw>"),
    # Same shape with the other two record types, so the rule is not pinned
    # to one name.
    ("rebatch_text",
     "pub fn rebatch_text(draws: &[TextDraw], origin: (i32, i32)) -> Vec<TextDraw>"),
    ("merge_sprites",
     "pub fn merge_sprites(a: &[SpriteDraw], b: &[SpriteDraw]) -> Vec<SpriteDraw>"),
    # Not a draw builder at all - no quads out.
    ("scale_stage_text_draws",
     "pub fn scale_stage_text_draws(draws: &mut [TextDraw], stage_origin: (i32, i32), "
     "stage_scale: u32)"),
    # A rect in the PARAMETER list is a model, not a transform input: rects
    # are deliberately kept out of `TRANSFORM_PARAM_RE`, so this stays a
    # screen-shaped signature that simply returns no geometry.
    ("enemy_target_menu_rows_y",
     "pub fn enemy_target_menu_rows_y(host_box: Option<(i32, i32, i32, i32)>) -> i32"),
    # The intermediate-record renderer: HudDraw list in, TextDraw list out.
    # A projection of a screen it did not build - the screens are
    # `persistent_hud_draws` / `catch_hud_draws` above.
    ("fishing_hud_draws_for",
     "pub fn fishing_hud_draws_for(font: &legaia_font::Font, items: &[HudDraw], "
     "captions: &FishingCaptions<'_>, atlas: &FishingHudAtlas<'_>, origin: (i32, i32)) "
     "-> Vec<TextDraw>"),
    ("compare_panel_draws_for",
     "pub fn compare_panel_draws_for(font: &legaia_font::Font, "
     "fields: &[ComparePanelField]) -> Vec<TextDraw>"),
]

# Control suite for `signature_end`, the scan the whole call graph rests on.
# Each case is `(label, source, should_find_a_body)`.
SELFTEST_SIGNATURES: list[tuple[str, str, bool]] = [
    ("ordinary fn has a body", "fn f(a: i32) -> u8 { 0 }", True),
    ("array return type's `;` is not a terminator",
     "fn tint(x: u8) -> [f32; 4] { [0.0; 4] }", True),
    ("trait declaration has no body", "fn on_scene_enter(&mut self, s: &str);", False),
    ("generic fn has a body", "fn f<T: Into<u8>>(v: T) -> Vec<TextDraw> { vec![] }", True),
]

# Control suite for the sim-pair comparators, over synthetic bodies so the
# modes are pinned independently of whatever the tree happens to look like.
# Each case is `(label, mode, body_a, body_b, extra, should_diverge)`.
SELFTEST_SIM: list[tuple[str, str, str, str, object, bool]] = [
    ("symbols_all: both call it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "damage_finish_lazy(&g)", ["damage_finish_lazy"], False),
    ("symbols_all: one host misses it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "let d = ad_hoc_damage(&g);",
     ["damage_finish_lazy"], True),
    ("symbols_same: neither calls it is agreement", "symbols_same",
     "nothing()", "nothing_else()", ["set_card_slots_mode"], False),
    ("symbols_same: exactly one calls it is drift", "symbols_same",
     "s.set_card_slots_mode(true)", "s.reset()", ["set_card_slots_mode"], True),
    ("pattern_same: same set in any order", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_b(); w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", False),
    ("pattern_same: a missing installer is drift", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", True),
]


# Control suite for the page-key detector (tier 5), over synthetic page
# sources. `(label, source, should_flag)`. Both directions matter: a detector
# that flagged every `.key` would fail the play page's scene-group records,
# and one that flagged nothing would have passed the minigames page's real
# `{a:1,s:2,d:3}` table for as long as it existed.
SELFTEST_PAGE_KEYS: list[tuple[str, str, bool]] = [
    ("e.key read is a page-side table", "const k = e.key.toLowerCase();", True),
    ("event.key read is a page-side table", "if (event.key === 'a') go();", True),
    ("e.code against a bindable key is a page-side table",
     "if (e.code === 'KeyA') go();", True),
    ("e.code against a non-pad key is fine", "if (e.code === 'Escape') close();", False),
    ("a plain object's .key field is not a KeyboardEvent",
     "scenes.filter(s => s.category === g.key);", False),
    ("dataset.key is an attribute, not a KeyboardEvent",
     "const k = kbd.dataset.key;", False),
    ("resolving through the engine table is the fix",
     "const b = window.legaiaPadButtonOf(e.code); if (b === 'Circle') cast();", False),
    ("a bindable code in a set membership test is a page-side table",
     "const startEdge = p.has('Enter');", True),
    ("...including the key this rule was written for",
     "if (held.has('Space')) pause();", True),
    ("a non-key literal in a set test is fine",
     "if (seenScenes.has('town01')) skip();", False),
    ("testing the pad bit is the fix",
     "const startEdge = (padMaskOf(p) & window.legaiaPadButton('Start')) !== 0;", False),
    ("a gamepad layout array in a page is a controller table",
     "const B = ['Cross', 'Circle', 'Square', 'Triangle', 'L1', 'R1'];", True),
    ("reading the Gamepad API outside the adopter is a second reader",
     "const pads = navigator.getGamepads();", True),
    ("an object keyed by button name is a lookup, not a layout",
     "const FACE_LANES = { Square: 1, Circle: 2, Triangle: 3, Cross: 4 };", False),
    ("three button names are a short list, not a layout",
     "const lanes = ['Square', 'Circle', 'Triangle'];", False),
    ("polling through the adopter is the fix",
     "const { pressed } = window.legaiaGamepadEdges();", False),
]


# Control suite for the constant-pair normaliser. A normaliser that collapsed
# everything to "" would report every pair equal and the check would be
# theatre, so both directions are pinned: noise must vanish, values must not.
#
# Each case is `(label, source_a, source_b, should_match)`.
SELFTEST_CONSTANTS: list[tuple[str, str, str, bool]] = [
    (
        "formatting and import aliasing are noise",
        "{ use legaia_game_tables::menu_windows::window_ids as w;\n"
        "  [ (w::TAB_ITEMS, (16, 12, 60, 12)),\n"
        "    (w::TAB_MAGIC, (16, 12, 60, 12)) ] }",
        "{use foo::bar as w; [(w::TAB_ITEMS,(16,12,60,12)),(w::TAB_MAGIC,(16,12,60,12)),]}",
        True,
    ),
    (
        "comments are noise",
        "(18, 18, 284, 200) // the near-fullscreen stage rect",
        "(18, 18, 284, 200) /* same rect, different note */",
        True,
    ),
    (
        "one changed digit is a value change",
        "(8, 140)",
        "(8, 141)",
        False,
    ),
    (
        "a dropped table row is a value change",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::A, (1, 2, 3, 4))]",
        False,
    ),
    (
        "a reordered table is a value change (id order is the table)",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::B, (5, 6, 7, 8)), (w::A, (1, 2, 3, 4))]",
        False,
    ),
]


# Control suite for `word_set`, which stands in for the per-name
# `re.search(r"\bNAME\b", body)` the reachability pass used to run. The
# substitution is the reason this gate costs a second instead of half a
# minute, and it is only sound if the two predicates agree on every string -
# so each case is checked twice: against the stated expectation, and against
# the regex it replaced. A control that only asked "does word_set say yes"
# would pass just as happily for a tokeniser that had drifted along with it.
SELFTEST_WORDS: list[tuple[str, str, str, bool]] = [
    ("plain call", "let v = foo(bar);", "foo", True),
    ("method position", "self.model.foo();", "foo", True),
    ("path position", "engine_ui::foo(&font)", "foo", True),
    ("prefixed name", "let v = draw_foo(bar);", "foo", False),
    ("suffixed name", "let v = foo_draws_for(bar);", "foo", False),
    # The one an identifier-shaped tokeniser gets wrong: `[A-Za-z_][\w]*`
    # finds `foo` inside `1foo`, `\bfoo\b` does not.
    ("digit-prefixed run", "let v = 1foo;", "foo", False),
    ("digit-suffixed run", "let v = foo2;", "foo", False),
    ("absent", "let v = bar(baz);", "foo", False),
]


# Control suite for the toggle-initialiser detector. A tier that cannot tell
# `new(false)` from `new(true)` proves nothing about the hosts.
SELFTEST_DIAG: list[tuple[str, str, bool]] = [
    ("atomic off", "std::sync::atomic::AtomicBool::new(false)", True),
    ("atomic on", "std::sync::atomic::AtomicBool::new(true)", False),
    ("cell off", "Cell::new(false)", True),
    ("bare off", "false", True),
    ("bare on", "true", False),
    ("expression", "cfg!(debug_assertions)", False),
]


# Positive control. A rule engine that matched nothing would report every
# surface clean, which is the failure mode this whole file exists to refuse -
# so each case pins one direction of one detector against a synthetic source.
SELFTEST_RENDER: list[tuple[str, dict, str, bool]] = [
    (
        "requires (lookahead trigger): battle surface that never gates the ground pass",
        {"trigger": r"(?s)\A(?=.*\buploadGround\s*\().*\bplay_battle_active\b",
         "requires": [r"\bsetGroundEnable\s*\(\s*false\s*\)"]},
        "this.renderer.uploadGround(p, u, c, i);\nif (rt.play_battle_active()) draw();",
        True,
    ),
    (
        "requires (lookahead trigger): battle surface that gates it, either order",
        {"trigger": r"(?s)\A(?=.*\buploadGround\s*\().*\bplay_battle_active\b",
         "requires": [r"\bsetGroundEnable\s*\(\s*false\s*\)"]},
        "if (rt.play_battle_active()) this.renderer.setGroundEnable(false);\nthis.renderer.uploadGround(p, u, c, i);",
        False,
    ),
    (
        "requires: triggering file that reaches the kernel",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "let (t, _) = resolve_env_draws(&e, &r, lut);\nlet o = coplanar_draw_offsets(&t, &p);",
        False,
    ),
    (
        "requires: triggering file that does NOT reach the kernel",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "let (t, _) = resolve_env_draws(&e, &r, lut);\nout.append(t);",
        True,
    ),
    (
        "requires: non-triggering file is not a finding",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "fn draw_hud() { let x = 1; }",
        False,
    ),
    (
        "requires: the kernel named only in a comment does not count",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "// runs coplanar_draw_offsets later\nlet (t, _) = resolve_env_draws(&e, &r, lut);",
        True,
    ),
    (
        "forbids: multi-line white fill of a packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nself.flat\n    .extend(std::iter::repeat_n([255u8; 4], n).flatten());",
        True,
    ),
    (
        "forbids: single-line white fill of a packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nlet flat = vec![255u8; n * 4];",
        True,
    ),
    (
        "forbids: a resolved stream is not a finding",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "let flat = crate::packet_color::hybrid(&mesh, &shading);",
        False,
    ),
    (
        "forbids: the textured FLAG byte 255 is not a white fill",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "// packet_color\nlet mut flat = Vec::new();\nflat.extend_from_slice(&[c[0], c[1], c[2], 255]);",
        False,
    ),
    (
        "forbids: a white literal far from any packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nlet flat = pc(&m);\nlet a = 1;\nlet b = 2;\n"
        "let c = 3;\nlet tint = [255u8, 0, 0, 255];",
        False,
    ),
]


# Control suites for the three tiers above. Each is `(label, ..., want)`, and
# each runs on every invocation for the reason the suites above it do: a
# detector that matches nothing reports every surface clean, which is exactly
# the silence these tiers exist to break.
SELFTEST_OWNERSHIP: list[tuple[str, str, str, bool]] = [
    ("a struct field is ownership",
     "struct App {\n    camera: Camera,\n    n: u8,\n}", "Camera", True),
    ("an Option field is ownership",
     "pub(crate) struct R {\n    pub scene_host: Option<SceneHost>,\n}", "SceneHost", True),
    ("a borrowed parameter on its own line is not ownership",
     "fn frame(\n    cam: &Camera,\n) -> u8 {\n    0\n}", "Camera", False),
    ("a construction is ownership", "let c = Camera::new();", "Camera", True),
    ("a `::default()` construction is ownership",
     "Self { camera: Camera::default() }", "Camera", True),
    ("a `use` line is not ownership", "use legaia_engine_field::camera::Camera;", "Camera", False),
    ("a match arm is not ownership", "match m { Camera => 1, _ => 0 }", "Camera", False),
    ("a by-reference parameter is not ownership",
     "fn frame(cam: &Camera) -> u8 { 0 }", "Camera", False),
    ("a return type is not ownership", "fn make() -> Camera { todo!() }", "Camera", False),
]

SELFTEST_VARIANT_USE: list[tuple[str, str, str, str, bool]] = [
    ("a qualified match arm answers the variant",
     "match m { SceneMode::Fishing => hud(), _ => () }", "SceneMode", "Fishing", True),
    ("an unqualified mention does not",
     "let s: FishingSession = fishing_session();", "SceneMode", "Fishing", False),
    ("a different enum's variant does not",
     "MinigameSubId::Fishing => 972,", "SceneMode", "Fishing", False),
]

SELFTEST_CALL_FORM: list[tuple[str, str, str, bool]] = [
    ("a method call is a caller", "host.world.enter_dance(g);", "enter_dance", True),
    ("a path call is a caller", "World::enter_dance(&mut w, g);", "enter_dance", True),
    ("the definition is not a caller", "pub fn enter_dance(&mut self, g: DanceGame) {", "enter_dance", False),
    ("a same-named free fn is not a method caller", "enter_dance(g);", "enter_dance", False),
]


# Control suite for the frame-path scanner, over synthetic frame paths. The
# first pair is the defect this tier was written for and its fix, in shape: an
# arm that `continue`s before a kernel the fall-through path runs, and the
# same arm with the kernel stepped on it. Each case is `(label, source,
# anchor, exit_word, arm_substring, expected_skips)`.
SELFTEST_FRAME: list[tuple[str, str, str, str, str, list[str]]] = [
    (
        "an arm that continues before a kernel skips it",
        "for _ in 0..n {\n"
        "    if self.boot_ui.is_active() {\n"
        "        self.tick_boot_ui();\n"
        "        self.prev_pad = self.pad;\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_field_party_hud();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.boot_ui.is_active()",
        ["tick_field_party_hud"],
    ),
    (
        "the same arm stepping the kernel skips nothing",
        "for _ in 0..n {\n"
        "    if self.boot_ui.is_active() {\n"
        "        self.tick_boot_ui();\n"
        "        self.tick_field_party_hud();\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_field_party_hud();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.boot_ui.is_active()",
        [],
    ),
    (
        "a kernel ABOVE the exit has already run this iteration",
        "for _ in 0..n {\n"
        "    self.tick_play_clock();\n"
        "    if self.paused {\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_dev_menu();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.paused",
        ["tick_dev_menu"],
    ),
    (
        "a `let ... else` return arm is an arm",
        "fn tick_frame(&mut self) {\n"
        "    let Some(h) = self.host.as_mut() else {\n"
        "        return;\n"
        "    };\n"
        "    self.tick_camera();\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "let Some(h) = self.host.as_mut() else",
        ["tick_camera"],
    ),
    (
        "a nested return still names its own arm",
        "fn tick_frame(&mut self) {\n"
        "    self.tick_camera();\n"
        "    if entered {\n"
        "        self.on_scene_change();\n"
        "        return;\n"
        "    }\n"
        "    self.poll_field_shop();\n"
        "    self.drive_npc_clips();\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "if entered",
        ["drive_npc_clips", "poll_field_shop"],
    ),
    (
        "an exit at the path's own depth is the end, not an arm",
        "fn tick_frame(&mut self) {\n"
        "    self.tick_camera();\n"
        "    return;\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "",
        ["<arm not found>"],
    ),
]


# Control suite for the content scan. Each case is
# `(label, kernel, host source, engine api, expected calls)`; the first pair
# is the shape this tier was written for - an empty body paired with one that
# does the work - and the last two are the two ways the scan can go blind
# (a helper chain not followed, a std-named call counted as engine).
SELFTEST_CONTENT: list[tuple[str, str, str, set[str], set[str]]] = [
    (
        "an empty body reaches nothing",
        "tick_props",
        "fn tick_props(&mut self) {}",
        {"advance_clips"},
        set(),
    ),
    (
        "a body that calls the engine reaches it",
        "tick_props",
        "fn tick_props(&mut self) { self.world.advance_clips(1); }",
        {"advance_clips"},
        {"advance_clips"},
    ),
    (
        "a helper chain is followed",
        "tick_props",
        "fn tick_props(&mut self) { self.inner(); }\n"
        "fn inner(&mut self) { self.world.advance_clips(1); }",
        {"advance_clips"},
        {"advance_clips"},
    ),
    (
        "a std-named call is not credited to the engine",
        "tick_props",
        "fn tick_props(&mut self) { self.rows.clear(); }",
        {"clear"} - STD_METHOD_NAMES,
        set(),
    ),
]


SELFTEST_BOOT_INSTALL = [
    ("dotted on one line", "host.world.install_menu_text(&scus);", {"install_menu_text"}),
    ("dot on the next line", "host.world\n    .set_item_catalog(x);", {"set_item_catalog"}),
    ("a definition is not a call", "pub fn install_menu_text(&mut self) {}", set()),
    ("a non-world receiver is not a boot install", "tracker.set_growth(x);", set()),
]


SELFTEST_SAVE_IO = [
    (
        "a call inside the applier is allowed",
        "fn apply_card_outcome(&mut self) { self.write_session_into_card(0, 1); }",
        [("write_session_into_card", "apply_card_outcome")],
    ),
    (
        "a call from a page button is attributed to it",
        "pub fn quick_save(&mut self) {\n    let _ = self.write_session_into_card(0, 1);\n}",
        [("write_session_into_card", "quick_save")],
    ),
    (
        "a definition is not a call",
        "pub(crate) fn write_session_into_card(&mut self) -> Result<(), String> { Ok(()) }",
        [],
    ),
    (
        "a test module is not shipped code",
        "#[cfg(test)]\nmod tests {\n    fn t() { rt.write_session_into_card(0, 1); }\n}",
        [],
    ),
    (
        "a test-only conjunction module is not shipped code",
        "#[cfg(all(test, not(target_arch = \"wasm32\")))]\nmod cross {\n    fn t() { rt.write_session_into_card(0, 1); }\n}",
        [],
    ),
    (
        "an any(test, ..) module still ships",
        "#[cfg(any(test, feature = \"x\"))]\nmod m {\n    fn t() { rt.write_session_into_card(0, 1); }\n}",
        [("write_session_into_card", "t")],
    ),
    (
        "a #[test] fn is not shipped code",
        "#[test]\nfn writes() { rt.write_session_into_card(0, 1); }",
        [],
    ),
]

# Detector controls for the row self-check's path shape: what the walk
# over the rows reads as a path, and what it must leave alone.
SELFTEST_PATH_STRINGS: list[tuple[str, str, bool]] = [
    ("a host source file", "crates/web-play/src/play_menu.rs", True),
    ("a source directory", "crates/engine-shell/src/window", True),
    ("a site script", "site/js/webgl-shaders.js", True),
    ("a function name", "play_overlay_draws_json", False),
    ("a Rust path, not a file path", "crates::engine_ui::menu", False),
    ("a regex over a path", r"crates/web-play/src/play_\w+\.rs", False),
    ("prose that mentions a path", "see crates/engine-ui/src for the builders", False),
    ("a path outside the known roots", "target/port-catalog/catalog.csv", False),
]

# Detector controls for tier 15: each source and the env-read violations the
# scan must report for it (`(call, name)`; a non-literal name is `None`).
SELFTEST_ENV_READS: list[tuple[str, str, list[tuple[str, object]]]] = [
    (
        "a gameplay switch read through the full path",
        'let on = std::env::var_os("LEGAIA_BATTLE_FRAME_CADENCE").is_some();',
        [("var_os", "LEGAIA_BATTLE_FRAME_CADENCE")],
    ),
    (
        "a read through a `use std::env;` import",
        'use std::env;\nlet v = env::var("LEGAIA_SOMETHING");',
        [("var", "LEGAIA_SOMETHING")],
    ),
    (
        "a non-literal name is never allowed",
        "let v = std::env::var(name);",
        [("var", None)],
    ),
    ("a declared diagnostic gate", 'std::env::var_os("LEGAIA_DIAG_HUD")', []),
    ("an inline test's disc gate", 'std::env::var_os("LEGAIA_DISC_BIN")?;', []),
    ("a read in a comment", '// std::env::var_os("LEGAIA_ARTS_SAVED_LIST")', []),
    ("temp_dir is not a read", "let d = std::env::temp_dir();", []),
]
