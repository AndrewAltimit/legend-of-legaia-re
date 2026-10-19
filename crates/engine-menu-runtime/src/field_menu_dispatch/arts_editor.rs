//! The Tactical Arts chain editor as the pause menu opens and projects it.

use super::*;

// ---------------------------------------------------------------------------
// Tactical Arts chain editor - engine-extension entry point + view
// ---------------------------------------------------------------------------

/// Pad button that opens the Tactical Arts chain editor from the Status
/// screen. Retail's status panel (`FUN_801D33D8`) reads Left / Right /
/// L1 / R1 / Circle / Start only, so Triangle is unclaimed there and the
/// extension costs no retail input.
pub const ARTS_EDITOR_OPEN_BUTTON: PadButton = PadButton::Triangle;

/// Phase tag of an [`ArtsEditorView`], mirroring
/// [`legaia_engine_battle::tactical_arts_editor::EditorPhase`] without the payloads so
/// renderer crates can match on it without depending on `engine-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsEditorPhaseTag {
    Browsing,
    Editing,
    Naming,
}

/// Everything the Tactical Arts editor screen draws, projected out of a
/// live [`ChainEditor`] plus the [`World`] it belongs to.
///
/// Both hosts (the native `play-window` and the browser play page) build
/// their `engine-ui` draw args from this one projection, so the character
/// name lookup, the pretty-printed sequences, the phase mapping and the
/// "+ New" room check cannot drift apart between them.
#[derive(Debug, Clone)]
pub struct ArtsEditorView {
    /// Roster name of the character whose library is being edited.
    pub character_name: String,
    pub phase: ArtsEditorPhaseTag,
    /// One `(name, pretty_sequence)` pair per saved chain, in library order.
    pub saved: Vec<(String, String)>,
    /// Cursor row in the browse list (meaningful in `Browsing`).
    pub browse_cursor: u8,
    /// Pretty-printed working sequence (`Editing` / `Naming`).
    pub editing_pretty: String,
    pub editing_len: usize,
    pub min_len: usize,
    pub max_len: usize,
    /// Name being picked in the `Naming` phase.
    pub naming_name: String,
    /// `true` while the library has room for one more chain - the browse
    /// list only shows its trailing "+ New" row then.
    pub can_add_new: bool,
}

/// Project a live [`ChainEditor`] into the renderer-agnostic
/// [`ArtsEditorView`] both hosts draw from.
///
/// The editor's `library_view` is the authoritative saved-chain list until
/// the engine calls [`apply_arts_outcome`] - it is the snapshot the editor
/// took at construction, so the screen stays consistent with the edits in
/// flight rather than with the world's not-yet-updated records.
pub fn arts_editor_view(editor: &ChainEditor, world: &World) -> ArtsEditorView {
    use legaia_engine_battle::tactical_arts_editor::EditorPhase;

    let char_slot = editor.char_slot();
    let character_name = world
        .party
        .roster_names()
        .get(char_slot as usize)
        .cloned()
        .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

    let library = editor.library_view();
    let saved: Vec<(String, String)> = library
        .iter()
        .map(|c| (c.name.clone(), c.pretty_sequence()))
        .collect();

    let pretty_of = |working: &[legaia_art::queue::Command]| -> String {
        working
            .iter()
            .map(|c| match c {
                legaia_art::queue::Command::Left => "L",
                legaia_art::queue::Command::Right => "R",
                legaia_art::queue::Command::Up => "U",
                legaia_art::queue::Command::Down => "D",
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let (phase, browse_cursor, editing_pretty, editing_len, naming_name) = match editor.phase() {
        EditorPhase::Browsing { cursor } => (
            ArtsEditorPhaseTag::Browsing,
            *cursor,
            String::new(),
            0usize,
            String::new(),
        ),
        EditorPhase::Editing { working } => (
            ArtsEditorPhaseTag::Editing,
            0,
            pretty_of(working),
            working.len(),
            String::new(),
        ),
        EditorPhase::Naming { working, name } => (
            ArtsEditorPhaseTag::Naming,
            0,
            pretty_of(working),
            working.len(),
            name.clone(),
        ),
        // Terminal state: the host is about to drain the outcome, so draw
        // the browse frame rather than a half-torn editing one.
        EditorPhase::Done(_) => (
            ArtsEditorPhaseTag::Browsing,
            0,
            String::new(),
            0usize,
            String::new(),
        ),
    };

    ArtsEditorView {
        character_name,
        phase,
        can_add_new: saved.len() < ChainLibrary::MAX_SLOTS,
        saved,
        browse_cursor,
        editing_pretty,
        editing_len,
        min_len: ChainLibrary::MIN_LEN,
        max_len: ChainLibrary::MAX_LEN,
        naming_name,
    }
}

/// Engine-extension entry point for the Tactical Arts chain editor.
///
/// Retail's pause menu is the seven rows in [`FieldMenuRow::ALL`] and has
/// no Arts row, so the editor needs an entry that does not invent an
/// eighth: pressing [`ARTS_EDITOR_OPEN_BUTTON`] while the **Status**
/// screen is up swaps that sub-session for a [`ChainEditor`] on the
/// character the panel is currently showing. Status is the retail surface
/// that lists a character's arts, which is why
/// [`FieldMenuSubsession::row`] already parks the Arts resume cursor
/// there.
///
/// Hosts call this once per frame *before* [`FieldMenuSubsession::tick_pad_edge`]
/// and skip that tick when it returns `true`, so the same edge does not
/// also drive the screen it just replaced. Returns `false` (and leaves
/// `sub` alone) for every other sub-session and every other button.
pub fn try_open_arts_editor(sub: &mut FieldMenuSubsession, pressed: u16, world: &World) -> bool {
    if pressed & ARTS_EDITOR_OPEN_BUTTON.mask() == 0 {
        return false;
    }
    let FieldMenuSubsession::Status(status) = sub else {
        return false;
    };
    // The status panel filters out unclaimed roster slots, so its cursor
    // indexes the *shown* list - map back through the snapshot's own slot
    // so the editor edits the right character's library.
    let char_slot = status
        .current()
        .map(|snap| snap.slot)
        .unwrap_or_else(|| status.cursor());
    let library = world.chain_library();
    *sub = FieldMenuSubsession::Arts(ChainEditor::new(char_slot, &library));
    true
}
