//! The keyboard-to-pad binding table, read from the engine instead of typed
//! into the page.
//!
//! Every browser page that takes input needs a `KeyboardEvent.code -> PSX pad
//! bit` table. There used to be two of them, hand-written in JS, and neither
//! agreed with the engine's own default layout
//! (`legaia_engine_system::input::Mapping::default`) or with each other:
//!
//! | key | native window | play page | boot / title |
//! |---|---|---|---|
//! | `X` | Square | Circle | Circle |
//! | `S` | Circle | Down | Down |
//! | `A` | Triangle | Left | - |
//! | `W` | R1 | Up | Up |
//! | `RShift` | Select | - | - |
//! | `Q` / `1` / `2` | L1 / L2 / R2 | - | - |
//!
//! Nothing about that is visible in a diff, because no file holds two of the
//! columns. The fix is not to correct the tables - it is to delete them: the
//! pages call [`pad_bindings_json`] and use whatever the engine binds, so a
//! rebind lands on every host at once and a disagreement can no longer be
//! written down.
//!
//! The layout served here *starts* at `Mapping::web_default`, not
//! `Mapping::default`. The browser page binds `WASD` to the d-pad and the
//! desktop layout spends those keys on Triangle / Circle / R1, so the two
//! cannot be one table - a `HashMap<key, button>` holds either `S -> Down` or
//! `S -> Circle`, never both. They are two *named layouts in the engine*
//! rather than one layout plus a page-side override, which keeps the
//! single-source-of-truth property that made the tables above deletable.
//!
//! "Starts at", because the table is now **editable**: the options screen's
//! Key Config screen ([`legaia_engine_menus::key_rebind::KeyRebindSession`],
//! reached through [`legaia_engine_menus::options::OptionsSession`]) commits
//! binds into it, and [`store_mapping`] round-trips the result through
//! `localStorage` under [`BINDINGS_STORAGE_KEY`] - the browser twin of the
//! native window's `legaia-input.toml`. A page that folded
//! [`pad_bindings_json`] into a JS object at load notices a rebind through
//! [`pad_bindings_revision`].
//!
//! A free function rather than a [`crate::runtime::LegaiaRuntime`] method
//! because the title screen's key loop runs before a runtime exists, and it
//! needs the same table.

use crate::runtime::LegaiaRuntime;
use legaia_engine_system::input::Mapping;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

/// `localStorage` key the page's edited binding table round-trips through -
/// the browser twin of the native window's `legaia-input.toml`.
///
/// Same store, same shape and the same "absent or unparseable falls back to
/// the default layout" rule as `legaia.options` beside it, so the two
/// persisted settings behave identically.
pub const BINDINGS_STORAGE_KEY: &str = "legaia.bindings";

thread_local! {
    /// The page's **live** binding table, plus a revision counter.
    ///
    /// A module-level cell rather than a [`LegaiaRuntime`] field because the
    /// title screen's key loop runs before a runtime exists and reads the same
    /// table (which is why [`pad_bindings_json`] is a free function), and
    /// because the minigames page has no `LegaiaRuntime` at all. wasm32 is
    /// single-threaded, so there is exactly one of these per page.
    ///
    /// The revision is what lets a page notice a rebind: a page folds this
    /// table into a JS object once at load, and nothing about
    /// [`pad_bindings_json`] returning something new would reach it otherwise.
    static LIVE_BINDINGS: RefCell<(Option<Mapping>, u32)> = const { RefCell::new((None, 0)) };
}

thread_local! {
    /// The controller pressed last (its labels and index space), or `None`
    /// while a key was: what the prompts name ([`last_pad`]). Free-standing
    /// for the same reason as [`LIVE_BINDINGS`]: the title loop folds the
    /// controller before a runtime exists.
    static LAST_PAD: std::cell::Cell<Option<legaia_engine_system::gamepad::PadStyle>> =
        const { std::cell::Cell::new(None) };
}

/// The live table, loading the persisted one (or the default layout) on first
/// use.
pub fn live_mapping() -> Mapping {
    with_live_mapping(Mapping::clone)
}

/// Run `f` over the live table without cloning it (the per-frame
/// controller fold).
pub fn with_live_mapping<R>(f: impl FnOnce(&Mapping) -> R) -> R {
    LIVE_BINDINGS.with(|c| {
        let mut c = c.borrow_mut();
        if c.0.is_none() {
            c.0 = Some(load_persisted_mapping());
        }
        f(c.0.as_ref().expect("just seeded"))
    })
}

/// The controller pressed last, while it was pressed after any key.
pub fn last_pad() -> Option<legaia_engine_system::gamepad::PadStyle> {
    LAST_PAD.with(|c| c.get())
}

/// A key was pressed: prompts name keys again.
pub fn note_keyboard() {
    LAST_PAD.with(|c| c.set(None));
}

/// A controller with `Gamepad.id` `id`, reporting in `space`, was pressed.
pub fn note_pad(space: legaia_engine_system::gamepad::ButtonSpace, id: &str) {
    let style = legaia_engine_system::gamepad::PadStyle {
        glyphs: legaia_engine_system::gamepad::GlyphSet::from_name(id),
        space,
    };
    LAST_PAD.with(|c| c.set(Some(style)));
}

/// Fold one `navigator.getGamepads()` pad through the live controller
/// table into a PSX pad word - the fold the native window runs gilrs
/// through (`GamepadMapping::fold_pad`).
///
/// `standard` is `Gamepad.mapping === "standard"`; a pad without it folds
/// through the table's raw half, with a d-pad read off a hat found from
/// `rest` (the pad's axes as first seen, nothing held). `buttons` is each
/// button's value (`1` for a pressed digital button), `axes` the axes now,
/// `id` the pad's `Gamepad.id` (whose maker labels the prompts once it is
/// pressed), `dead_zone` the stick's in percent (`stick_dead_zone`).
#[wasm_bindgen]
pub fn gamepad_fold(
    standard: bool,
    buttons: &[f32],
    axes: &[f32],
    rest: &[f32],
    id: &str,
    dead_zone: u8,
) -> u16 {
    use legaia_engine_system::gamepad::ButtonSpace;
    let space = if standard {
        ButtonSpace::Standard
    } else {
        ButtonSpace::Raw
    };
    let dz = legaia_engine_menus::options::OptionsState {
        stick_dead_zone: dead_zone,
        ..Default::default()
    }
    .stick_dead_zone_fraction();
    let word = with_live_mapping(|m| m.gamepad.fold_pad(space, buttons, axes, rest, dz));
    if word != 0 {
        note_pad(space, id);
    }
    word
}

/// What a prompt names for pad button `button`: the live table's key, or
/// the last pressed controller's button ([`Mapping::prompt_label`]).
pub fn prompt_label(button: &str) -> Option<String> {
    let pad = last_pad();
    with_live_mapping(|m| m.prompt_label(button, pad))
}

/// [`prompt_label`] for a page's printed hints (`""` when nothing is bound):
/// the page asks it once the controller or the keyboard was pressed last.
#[wasm_bindgen]
pub fn pad_prompt_label(button: &str) -> String {
    prompt_label(button).unwrap_or_default()
}

/// A key was pressed on a page that hands no key to the menu
/// (`LegaiaRuntime::play_menu_key` notes it itself): prompts name keys again.
#[wasm_bindgen]
pub fn pad_note_keyboard() {
    note_keyboard();
}

/// Adopt `mapping` as the live table and persist it - the browser twin of the
/// native window writing `legaia-input.toml` when the options screen's Key
/// Config row commits a bind.
pub fn store_mapping(mapping: &Mapping) {
    LIVE_BINDINGS.with(|c| {
        let mut c = c.borrow_mut();
        c.0 = Some(mapping.clone());
        c.1 = c.1.wrapping_add(1);
    });
    #[cfg(target_arch = "wasm32")]
    if let Some(store) = bindings_storage()
        && let Ok(json) = serde_json::to_string(mapping)
        && store.set_item(BINDINGS_STORAGE_KEY, &json).is_err()
    {
        crate::console_log("bindings: localStorage write failed");
    }
}

/// Read the persisted table, falling back to [`Mapping::web_default`] when
/// nothing is stored, the store is unreachable (private mode, non-browser
/// target) or the stored JSON no longer parses.
fn load_persisted_mapping() -> Mapping {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(store) = bindings_storage()
            && let Ok(Some(raw)) = store.get_item(BINDINGS_STORAGE_KEY)
            && let Ok(m) = serde_json::from_str::<Mapping>(&raw)
            && !m.bindings.is_empty()
        {
            return m;
        }
    }
    Mapping::web_default()
}

#[cfg(target_arch = "wasm32")]
fn bindings_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// How many times the live table has changed since the page loaded. A page
/// polls this and re-reads [`pad_bindings_json`] when it moves.
#[wasm_bindgen]
pub fn pad_bindings_revision() -> u32 {
    LIVE_BINDINGS.with(|c| c.borrow().1)
}

/// Forget the persisted table and go back to [`Mapping::web_default`] - the
/// escape hatch for a player who bound themselves out of the menu.
#[wasm_bindgen]
pub fn pad_bindings_reset() {
    let d = Mapping::web_default();
    store_mapping(&d);
    #[cfg(target_arch = "wasm32")]
    if let Some(store) = bindings_storage() {
        let _ = store.remove_item(BINDINGS_STORAGE_KEY);
    }
}

/// The engine's default keyboard layout as `{ "<KeyboardEvent.code>": <bit> }`.
///
/// Bits are the PSX digital-pad masks
/// (`legaia_engine_system::input::PadButton`) the runtime's `set_pad` /
/// `*_input` entry points already take, so a page folds held or pressed codes
/// straight into a pad word with no second table.
///
/// Keys the browser has no `code` for are absent rather than guessed at - see
/// `Mapping::dom_code_bindings`. Ordering is stable across calls.
#[wasm_bindgen]
pub fn pad_bindings_json() -> String {
    let mapping = live_mapping();
    let mut out = String::from("{");
    for (i, (code, bit)) in mapping.dom_code_bindings().into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{code}\":{bit}"));
    }
    out.push('}');
    out
}

/// Pad bits by button name, for the handful of places a page needs to name a
/// button rather than a key ("is confirm pressed?").
///
/// The title screen used to test `pulse.has('KeyZ') || pulse.has('Space')`,
/// which hardcodes both the layout and the assumption that confirm is a key
/// rather than a button. With this a page asks for `Cross` and stays correct
/// through a rebind.
#[wasm_bindgen]
pub fn pad_buttons_json() -> String {
    use legaia_engine_system::input::PadButton::*;
    let all = [
        Select, L3, R3, Start, Up, Right, Down, Left, L2, R2, L1, R1, Triangle, Circle, Cross,
        Square,
    ];
    let body = all
        .iter()
        .map(|b| format!("\"{}\":{}", b.name(), b.mask()))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

/// The controller layout (`legaia_engine_system::gamepad`) as
/// `{"buttons":[<PSX button name per W3C standard index>],"stick_dead_zone":..,"press_threshold":..}`.
///
/// The pages read `navigator.getGamepads()` and fold each standard-mapped
/// controller through this table, which is the one the native window folds
/// gilrs through - so a page never writes the button order down itself.
#[wasm_bindgen]
pub fn gamepad_layout_json() -> String {
    legaia_engine_system::gamepad::layout_json()
}

/// Runtime-method forwarders.
///
/// The free functions above are the real definitions; these exist because a
/// page that already holds a `LegaiaRuntime` should not have to reach for the
/// module namespace as well, and because `typeof rt.pad_bindings_json ===
/// 'function'` is the guard the pages already use to tell a fresh bundle from
/// a cached one.
#[wasm_bindgen]
impl LegaiaRuntime {
    /// See [`pad_bindings_json`].
    pub fn pad_bindings_json(&self) -> String {
        pad_bindings_json()
    }

    /// See [`pad_buttons_json`].
    pub fn pad_buttons_json(&self) -> String {
        pad_buttons_json()
    }

    /// See [`gamepad_layout_json`].
    pub fn gamepad_layout_json(&self) -> String {
        gamepad_layout_json()
    }

    /// See [`pad_bindings_revision`].
    ///
    /// A forwarder for the same reason the two above are, and it earns its
    /// keep: the page's `legaiaSyncPadBindings(src)` is handed a
    /// `LegaiaRuntime`, and a free function on the module namespace is not
    /// reachable through it. Without this the sync call answered
    /// `typeof !== 'function'` and silently did nothing, so a rebind stayed
    /// invisible to the running page - which is exactly the shape a browser
    /// check catches and a green cargo test cannot.
    pub fn pad_bindings_revision(&self) -> u32 {
        pad_bindings_revision()
    }

    /// See [`pad_bindings_reset`].
    pub fn pad_bindings_reset(&self) {
        pad_bindings_reset();
    }

    /// See [`pad_prompt_label`].
    pub fn pad_prompt_label(&self, button: &str) -> String {
        pad_prompt_label(button)
    }

    /// See [`pad_note_keyboard`].
    pub fn pad_note_keyboard(&self) {
        pad_note_keyboard();
    }

    /// See [`gamepad_fold`].
    pub fn gamepad_fold(
        &self,
        standard: bool,
        buttons: &[f32],
        axes: &[f32],
        rest: &[f32],
        id: &str,
        dead_zone: u8,
    ) -> u16 {
        gamepad_fold(standard, buttons, axes, rest, id, dead_zone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exported table must be parseable JSON carrying the whole default
    /// layout - a page that gets `{}` falls back to no input at all, silently.
    #[test]
    fn the_exported_table_is_json_and_carries_every_default_binding() {
        let json = pad_bindings_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON object");
        let obj = v.as_object().expect("object");
        let want = legaia_engine_system::input::Mapping::web_default().dom_code_bindings();
        assert_eq!(obj.len(), want.len());
        for (code, bit) in want {
            assert_eq!(obj.get(code).and_then(|b| b.as_u64()), Some(u64::from(bit)));
        }
    }

    /// The page's fold runs the live table: a standard pad by position, a
    /// raw pad through its own half and hat, the stick at the dead zone
    /// asked for, and a press makes the prompts name that pad's buttons.
    #[test]
    fn the_page_fold_is_the_shared_fold() {
        use legaia_engine_system::input::PadButton;
        note_keyboard();
        let cross = PadButton::Cross.mask();
        assert_eq!(gamepad_fold(true, &[1.0], &[0.0, 0.0], &[], "x", 50), cross);
        assert_eq!(
            gamepad_fold(true, &[], &[0.3, 0.0], &[], "x", 50),
            0,
            "inside the dead zone"
        );
        assert_eq!(
            gamepad_fold(true, &[], &[0.3, 0.0], &[], "x", 20),
            PadButton::Right.mask()
        );
        let rest = [0.0; 6];
        let mut axes = rest;
        axes[5] = -1.0;
        assert_eq!(
            gamepad_fold(
                false,
                &[0.0, 1.0],
                &axes,
                &rest,
                "054c-0268-PLAYSTATION(R)3",
                50
            ),
            PadButton::Circle.mask() | PadButton::Up.mask()
        );
        assert_eq!(prompt_label("Cross").as_deref(), Some("Btn 1"));
        note_keyboard();
        assert_eq!(prompt_label("Cross").as_deref(), Some("Z"));
    }

    /// The stored table carries the controller half, so a page reload keeps
    /// a controller rebind (`legaia.bindings`).
    #[test]
    fn the_stored_json_carries_the_controller_table() {
        use legaia_engine_system::gamepad::ButtonSpace;
        use legaia_engine_system::input::PadButton;
        let mut m = Mapping::web_default();
        m.gamepad.bind(ButtonSpace::Raw, 4, PadButton::Start);
        let json = serde_json::to_string(&m).unwrap();
        let back: Mapping = serde_json::from_str(&json).unwrap();
        assert_eq!(back.gamepad, m.gamepad);
        let old: Mapping = serde_json::from_str(r#"{"bindings":{"KeyZ":"Cross"}}"#).unwrap();
        assert_eq!(old.gamepad, Default::default());
    }

    #[test]
    fn the_button_table_names_all_sixteen_pad_bits() {
        let v: serde_json::Value = serde_json::from_str(&pad_buttons_json()).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 16);
        assert_eq!(obj.get("Cross").and_then(|b| b.as_u64()), Some(0x4000));
        assert_eq!(obj.get("Square").and_then(|b| b.as_u64()), Some(0x8000));
        assert_eq!(obj.get("Select").and_then(|b| b.as_u64()), Some(0x0001));
    }
}
