//! The Legaia look of the desktop programs: one palette, in a dark and a
//! light variant, and the spacing values, shared by `legaia-explorer`, the
//! game's launcher (`legaia-engine` with no subcommand) and the play
//! window's own chrome (the key hint, the toggle notices, the `F12` panel).
//!
//! The dark palette is the launcher's: a deep blue-grey ground, lighter card
//! and button tones, and the gold of the game's title lettering for the
//! accent and the primary button. The light palette carries the same hues on
//! a white ground, with the accent darkened until it reads on white.
//!
//! [`game_exit`] is the wording the launcher and Explorer share for a game
//! that stopped with an error.
//!
//! [`disc_text`] is the wording they share for a disc image that was cut
//! short.
//!
//! [`resolve_dark`] picks a palette for a program with a theme setting: the
//! player's choice, else the desktop's preference.
//!
//! [`icons`] carries the two programs' icons (`icons/`, drawn by
//! `icons/make_icons.py`), and [`win_icon`] embeds one into a Windows
//! executable from a build script. [`release_layout`] is where the release
//! archive puts the engine, for the two programs that start it.
//! [`placement`] is where each program's window reopens: where it last
//! closed, when that is still on a connected monitor.
//! [`drop_backend`] is the windowing backend the launcher and Explorer open
//! on so a dropped file reaches them. [`settings_file`] is how both treat
//! their own settings file: a broken one is set aside, never overwritten,
//! and every write lands whole. [`folder_list`] is the folder listing
//! behind the in-app file finders a controller can work (the launcher's and
//! Explorer's), where the desktop's file dialog wants a mouse.
//!
//! No dependencies: each consumer converts a [`Rgb`] into its toolkit's
//! colour (`egui::Color32::from_rgb`, a `[f32; 4]` for the launcher's
//! software canvas and the play window's text layer).

#![no_std]

// `game_exit` speaks in exit statuses and owned strings.
extern crate std;

pub mod disc_text;
pub mod drop_backend;
pub mod folder_list;
pub mod game_exit;
pub mod icons;
pub mod placement;
pub mod release_layout;
pub mod settings_file;
pub mod win_icon;

/// An sRGB colour, 8 bits per channel, opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Normalised `[r, g, b, 1.0]`, still sRGB-encoded (what a colour
    /// multiplied into an sRGB texture expects).
    pub const fn to_f32(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            1.0,
        ]
    }

    /// WCAG relative luminance, `0..=1`.
    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                // (c + 0.055) / 1.055 raised to 2.4, without `std`'s `powf`.
                pow_2_4((c + 0.055) / 1.055)
            }
        }
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG contrast ratio against `other`, `1..=21`.
    pub fn contrast(self, other: Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// `x^2.4` for `x` in `0..=1`: `x^2 * x^0.4`, the fractional power by a few
/// Newton steps on `y^5 = x^2`.
fn pow_2_4(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let target = x * x;
    let mut y = x;
    for _ in 0..30 {
        let y4 = y * y * y * y;
        y -= (y4 * y - target) / (5.0 * y4);
    }
    x * x * y
}

/// Every colour role the desktop programs draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The deepest surface: the launcher's body, Explorer's text fields and
    /// viewports.
    pub ground: Rgb,
    /// The backdrop behind a preview (Explorer's 3D view): a mid tone, so a
    /// mesh the disc bakes near black and a bright one both stand off it.
    pub stage: Rgb,
    /// Panels and bands (Explorer's side and top panels, the launcher's
    /// header band).
    pub band: Rgb,
    /// Cards and windows over a band.
    pub card: Rgb,
    /// The alternate row of a striped grid.
    pub stripe: Rgb,
    /// A button at rest.
    pub raised: Rgb,
    /// A button under the pointer or keyboard focus.
    pub hover: Rgb,
    /// A button held down.
    pub pressed: Rgb,
    /// An open combo box or menu button.
    pub open: Rgb,
    /// Separators.
    pub rule: Rgb,
    /// The edge of a control at rest (a check box, a text field, a light
    /// theme's button): at least 3:1 against every surface, the WCAG floor
    /// for a control's boundary.
    pub outline: Rgb,
    /// Body text.
    pub ink: Rgb,
    /// Secondary text.
    pub ink_dim: Rgb,
    /// Hints and disabled text: still text a player reads (the key
    /// reminder, a drop hint, a folder line), so it holds the WCAG AA 4.5:1
    /// on every surface text sits on, and stays visibly dimmer than
    /// [`Self::ink_dim`].
    pub ink_faint: Rgb,
    /// The title gold: headings, links, the selection outline.
    pub accent: Rgb,
    /// The primary button's fill.
    pub accent_fill: Rgb,
    /// The primary button's fill under focus.
    pub accent_fill_focus: Rgb,
    /// Text on an accent fill.
    pub on_accent: Rgb,
    /// A selected row's fill.
    pub selection: Rgb,
    /// A selected row's outline.
    pub selection_stroke: Rgb,
    /// The outline of a widget under the pointer.
    pub hover_stroke: Rgb,
    /// Success lines.
    pub good: Rgb,
    /// Warnings.
    pub warn: Rgb,
    /// Errors (a failed open, a failed export).
    pub error: Rgb,
    /// Key names and informational lines.
    pub info: Rgb,
}

/// The dark palette: the launcher's.
pub const DARK: Palette = Palette {
    ground: Rgb(14, 15, 24),
    stage: Rgb(46, 50, 74),
    band: Rgb(22, 24, 38),
    card: Rgb(28, 31, 46),
    stripe: Rgb(28, 31, 46),
    raised: Rgb(38, 41, 61),
    hover: Rgb(51, 59, 92),
    pressed: Rgb(70, 62, 40),
    open: Rgb(46, 50, 74),
    rule: Rgb(61, 66, 92),
    outline: Rgb(110, 118, 150),
    ink: Rgb(237, 237, 245),
    ink_dim: Rgb(153, 163, 189),
    ink_faint: Rgb(128, 136, 160),
    accent: Rgb(255, 209, 92),
    accent_fill: Rgb(224, 179, 66),
    accent_fill_focus: Rgb(255, 214, 102),
    on_accent: Rgb(26, 20, 10),
    selection: Rgb(120, 92, 30),
    selection_stroke: Rgb(255, 209, 92),
    hover_stroke: Rgb(200, 160, 70),
    good: Rgb(128, 219, 133),
    warn: Rgb(255, 148, 92),
    error: Rgb(255, 120, 120),
    info: Rgb(179, 217, 255),
};

/// The light palette: the same hues on white, the accent darkened to read
/// on it.
pub const LIGHT: Palette = Palette {
    ground: Rgb(255, 255, 255),
    stage: Rgb(196, 200, 214),
    band: Rgb(244, 244, 248),
    card: Rgb(250, 250, 252),
    stripe: Rgb(234, 235, 242),
    raised: Rgb(231, 231, 239),
    hover: Rgb(214, 217, 230),
    pressed: Rgb(246, 226, 180),
    open: Rgb(222, 224, 234),
    rule: Rgb(200, 203, 215),
    outline: Rgb(128, 132, 148),
    ink: Rgb(24, 26, 36),
    ink_dim: Rgb(80, 86, 104),
    ink_faint: Rgb(100, 106, 122),
    accent: Rgb(140, 93, 10),
    accent_fill: Rgb(224, 179, 66),
    accent_fill_focus: Rgb(240, 196, 84),
    on_accent: Rgb(26, 20, 10),
    selection: Rgb(246, 214, 140),
    selection_stroke: Rgb(110, 72, 0),
    hover_stroke: Rgb(190, 140, 40),
    good: Rgb(30, 120, 40),
    warn: Rgb(170, 80, 10),
    error: Rgb(176, 32, 32),
    info: Rgb(30, 90, 170),
};

/// The palette for a dark or a light surface.
pub const fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// Spacing, in logical points (egui) or stage pixels (the launcher and the
/// play window, whose stages are scaled up whole).
pub mod spacing {
    /// Inside a button: horizontal, vertical.
    pub const BUTTON_PADDING: [f32; 2] = [8.0, 3.0];
    /// Between neighbouring widgets: horizontal, vertical.
    pub const ITEM_SPACING: [f32; 2] = [8.0, 5.0];
    /// Between a section's heading and its body, and between sections.
    pub const SECTION_GAP: f32 = 6.0;
    /// The play window's chrome: the inset of its bottom-left lines from the
    /// stage edge, and the gap between stacked lines (stage pixels).
    pub const CHROME_INSET: i32 = 6;
    pub const CHROME_LINE_GAP: i32 = 4;
}

/// A colour by what it is for, so a surface drawn in either palette names
/// the role and resolves it at draw time ([`Palette::get`]). The launcher's
/// software canvas and the play window's chrome draw by role; Explorer maps
/// the same palette onto egui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Ground,
    Band,
    Card,
    Raised,
    Hover,
    Rule,
    Ink,
    InkDim,
    InkFaint,
    Accent,
    AccentFill,
    AccentFillFocus,
    OnAccent,
    Good,
    Warn,
    Info,
}

impl Palette {
    /// The colour `role` takes in this palette.
    pub const fn get(&self, role: Role) -> Rgb {
        match role {
            Role::Ground => self.ground,
            Role::Band => self.band,
            Role::Card => self.card,
            Role::Raised => self.raised,
            Role::Hover => self.hover,
            Role::Rule => self.rule,
            Role::Ink => self.ink,
            Role::InkDim => self.ink_dim,
            Role::InkFaint => self.ink_faint,
            Role::Accent => self.accent,
            Role::AccentFill => self.accent_fill,
            Role::AccentFillFocus => self.accent_fill_focus,
            Role::OnAccent => self.on_accent,
            Role::Good => self.good,
            Role::Warn => self.warn,
            Role::Info => self.info,
        }
    }
}

/// The play window's own chrome over the game (its `F12` panel and its
/// notice lines), in either palette: light chrome sits on a light scrim.
pub mod chrome {
    use super::{DARK, LIGHT, Palette, Role};

    /// Headings and prompts (*Press Esc again to quit*, the key hint).
    pub const HEADING: Role = Role::Accent;
    /// Key names in the help panel, and a toggle's notice line.
    pub const KEY: Role = Role::Info;
    /// Body text of the help panel.
    pub const TEXT: Role = Role::Ink;
    /// A setting's state the original game does not have, in the help panel;
    /// the launcher draws such a value in the same green.
    pub const CHANGED: Role = Role::Good;

    /// The chrome's inks, resolved for one palette.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Inks {
        pub palette: &'static Palette,
        pub heading: [f32; 4],
        pub key: [f32; 4],
        pub text: [f32; 4],
        pub changed: [f32; 4],
        pub faint: [f32; 4],
        pub hover: [f32; 4],
        pub rule: [f32; 4],
        /// The plate under chrome text, RGB: the palette's ground.
        pub scrim: [f32; 3],
    }

    /// The chrome's inks on a dark or a light surface.
    pub const fn inks(dark: bool) -> Inks {
        let palette: &'static Palette = if dark { &DARK } else { &LIGHT };
        let g = palette.ground.to_f32();
        Inks {
            palette,
            heading: palette.get(HEADING).to_f32(),
            key: palette.get(KEY).to_f32(),
            text: palette.get(TEXT).to_f32(),
            changed: palette.get(CHANGED).to_f32(),
            faint: palette.ink_faint.to_f32(),
            hover: palette.hover.to_f32(),
            rule: palette.rule.to_f32(),
            scrim: [g[0], g[1], g[2]],
        }
    }

    /// How opaque the plate under a notice line or the key hint is. Those
    /// lines sit over the game itself, so the plate has to carry the text
    /// over the worst picture behind it: a light plate over a black screen
    /// (the title, a fade) greys far sooner than a dark plate over a bright
    /// one, and at the dark plate's 0.7 the light inks fell under 3:1.
    pub const fn notice_scrim_alpha(dark: bool) -> f32 {
        if dark { 0.7 } else { 0.92 }
    }

    /// The role an option row's value takes, one rule for the launcher's list
    /// and the play window's settings page so a value reads the same in
    /// both: the original game's own value dim, any other value of a row the
    /// original has green (an enhancement, a fix or a tweak in effect). A row
    /// with no original counterpart (window size, time of day, volumes) reads
    /// green when `On`, dim when `Off`, and in the accent otherwise.
    pub fn option_value(value: &str, original: Option<&str>) -> Role {
        match (value, original) {
            (v, Some(o)) if v == o => Role::InkDim,
            (_, Some(_)) => CHANGED,
            ("On", None) => CHANGED,
            ("Off", None) => Role::InkDim,
            _ => Role::Accent,
        }
    }
}

/// Which palette a program with a theme setting draws in: `forced` when the
/// player chose one, else what the window system reports (`window_dark`, from
/// winit's `Window::theme`, which is unsupported on X11), else the desktop's
/// preference ([`desktop_prefers_dark`]), else dark, as the launcher has
/// always been.
pub fn resolve_dark(forced: Option<bool>, window_dark: Option<bool>) -> bool {
    forced
        .or(window_dark)
        .or_else(desktop_prefers_dark)
        .unwrap_or(true)
}

/// The desktop's light / dark preference where the window system does not
/// carry it (Linux, X11 above all): `GTK_THEME`, then GNOME's `color-scheme`,
/// then KDE's colour scheme. `None` when none says. Read once; a program
/// picks its palette up at start.
pub fn desktop_prefers_dark() -> Option<bool> {
    static CACHED: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *CACHED.get_or_init(linux_desktop_dark)
}

fn linux_desktop_dark() -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    if let Ok(t) = std::env::var("GTK_THEME") {
        let t = t.to_ascii_lowercase();
        if let Some(dark) = theme_name_dark(&t) {
            return Some(dark);
        }
    }
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(o) = out
        && o.status.success()
    {
        let s = std::string::String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
        if s.contains("dark") {
            return Some(true);
        }
        if s.contains("light") {
            return Some(false);
        }
        // 'default' on a desktop that also sets the older theme name.
        let name = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
            .stderr(std::process::Stdio::null())
            .output();
        if let Ok(n) = name {
            let n = std::string::String::from_utf8_lossy(&n.stdout).to_ascii_lowercase();
            return theme_name_dark(&n);
        }
    }
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    let kde = std::fs::read_to_string(home.join("kdeglobals")).ok()?;
    let line = kde.lines().find(|l| l.starts_with("ColorScheme="))?;
    theme_name_dark(&line.to_ascii_lowercase())
}

/// Whether a theme name says dark or light; `None` when it says neither.
fn theme_name_dark(name: &str) -> Option<bool> {
    if name.contains("dark") {
        Some(true)
    } else if name.contains("light") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_values_read_alike_in_both_lists() {
        assert_eq!(chrome::option_value("Off", Some("Off")), Role::InkDim);
        assert_eq!(chrome::option_value("On", Some("Off")), Role::Good);
        assert_eq!(chrome::option_value("On", None), Role::Good);
        assert_eq!(chrome::option_value("Off", None), Role::InkDim);
        assert_eq!(chrome::option_value("Auto", None), Role::Accent);
    }

    /// Every role an egui or canvas surface draws text or a fill under.
    fn check(p: &Palette, name: &str) {
        // The striped grid's alternate row carries the same text as a card.
        let body = [p.ground, p.band, p.card, p.stripe];
        for s in body {
            assert!(p.ink.contrast(s) >= 7.0, "{name}: ink on {s:?}");
            assert!(p.ink_dim.contrast(s) >= 4.5, "{name}: ink_dim on {s:?}");
            assert!(p.accent.contrast(s) >= 4.5, "{name}: accent on {s:?}");
            assert!(p.good.contrast(s) >= 4.5, "{name}: good on {s:?}");
            assert!(p.warn.contrast(s) >= 4.5, "{name}: warn on {s:?}");
            assert!(p.error.contrast(s) >= 4.5, "{name}: error on {s:?}");
            assert!(p.outline.contrast(s) >= 3.0, "{name}: outline on {s:?}");
            assert!(p.info.contrast(s) >= 4.5, "{name}: info on {s:?}");
            assert!(p.ink_faint.contrast(s) >= 4.5, "{name}: ink_faint on {s:?}");
            // A hint still reads as a hint: a step below the secondary ink.
            assert!(
                p.ink_dim.contrast(s) >= p.ink_faint.contrast(s) * 1.2,
                "{name}: ink_faint too close to ink_dim on {s:?}"
            );
        }
        assert!(p.ink.contrast(p.raised) >= 7.0, "{name}: button text");
        // A link or heading on a button or a striped row.
        assert!(
            p.accent.contrast(p.raised) >= 4.5,
            "{name}: accent on a button"
        );
        assert!(
            p.ink.contrast(p.hover) >= 4.5,
            "{name}: hovered button text"
        );
        assert!(
            p.ink.contrast(p.selection) >= 4.5,
            "{name}: selected row text"
        );
        for f in [p.accent_fill, p.accent_fill_focus] {
            assert!(
                p.on_accent.contrast(f) >= 7.0,
                "{name}: primary button text"
            );
        }
    }

    #[test]
    fn both_palettes_meet_text_contrast() {
        check(&DARK, "dark");
        check(&LIGHT, "light");
    }

    #[test]
    fn the_player_choice_outranks_the_window_and_the_desktop() {
        assert!(!resolve_dark(Some(false), Some(true)));
        assert!(resolve_dark(Some(true), Some(false)));
        assert!(!resolve_dark(None, Some(false)));
        assert_eq!(theme_name_dark("adwaita:dark"), Some(true));
        assert_eq!(theme_name_dark("breezelight"), Some(false));
        assert_eq!(theme_name_dark("'default'"), None);
    }

    /// `top` at `alpha` over `under`, blended in sRGB as the play window's
    /// chrome is.
    fn over(top: Rgb, under: Rgb, alpha: f32) -> Rgb {
        let mix = |t: u8, u: u8| (t as f32 * alpha + u as f32 * (1.0 - alpha)).round() as u8;
        Rgb(
            mix(top.0, under.0),
            mix(top.1, under.1),
            mix(top.2, under.2),
        )
    }

    #[test]
    fn notice_lines_read_over_the_worst_picture_behind_them() {
        // A dark plate is weakest over white, a light one over black.
        for (dark, palette, behind) in [
            (true, &DARK, Rgb(255, 255, 255)),
            (false, &LIGHT, Rgb(0, 0, 0)),
        ] {
            let plate = over(palette.ground, behind, chrome::notice_scrim_alpha(dark));
            for role in [chrome::HEADING, chrome::KEY] {
                let ratio = palette.get(role).contrast(plate);
                assert!(
                    ratio >= 4.5,
                    "{role:?} on the {} notice plate: {ratio:.2}:1",
                    if dark { "dark" } else { "light" }
                );
            }
        }
        // The light plate at the dark one's alpha is the defect this holds
        // off: the heading ink under 3:1 over a black screen.
        let weak = over(LIGHT.ground, Rgb(0, 0, 0), chrome::notice_scrim_alpha(true));
        assert!(LIGHT.get(chrome::HEADING).contrast(weak) < 3.0);
    }

    #[test]
    fn chrome_inks_follow_the_palette() {
        let (d, l) = (chrome::inks(true), chrome::inks(false));
        assert_eq!(d.text, DARK.ink.to_f32());
        assert_eq!(l.text, LIGHT.ink.to_f32());
        assert!(d.scrim[0] < 0.2 && l.scrim[0] > 0.9);
    }

    #[test]
    fn contrast_matches_the_wcag_reference_points() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert!((black.contrast(white) - 21.0).abs() < 0.05);
        assert!((white.contrast(white) - 1.0).abs() < 1e-4);
        // #777 on white is the textbook 4.48.
        assert!((Rgb(0x77, 0x77, 0x77).contrast(white) - 4.48).abs() < 0.02);
    }

    #[test]
    fn launcher_values_survive_the_round_trip() {
        // The launcher's palette was written as floats; the tokens are those
        // floats to the nearest 8-bit step.
        let close = |c: Rgb, f: [f32; 3]| {
            let g = c.to_f32();
            (0..3).all(|i| (g[i] - f[i]).abs() <= 0.5 / 255.0 + 1e-6)
        };
        assert!(close(DARK.accent, [1.0, 0.82, 0.36]));
        assert!(close(DARK.ground, [0.055, 0.06, 0.095]));
        assert!(close(DARK.card, [0.11, 0.12, 0.18]));
        assert!(close(DARK.ink, [0.93, 0.93, 0.96]));
        assert!(close(DARK.accent_fill, [0.88, 0.70, 0.26]));
        assert_eq!(palette(true), &DARK);
        assert_eq!(palette(false), &LIGHT);
    }
}
