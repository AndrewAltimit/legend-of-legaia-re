"""Tier 14 rows: the save-rack I/O primitives and the one fn allowed to call
each, plus the page card-rack rules.

Analysis: `ui_host_drift.tiers.save_io`.
"""

# ---------------------------------------------------------------------------
# Tier 14 - save-rack I/O reaches the card only through the save screen.
#
# Retail writes and reads a save in exactly one place: the card driver behind
# the save screen (`FUN_801DAEF4` / `FUN_801DAE24` -> `FUN_801DD35C`), after the
# pill row, the card read, the block grid and the confirm. The port's hosts
# each own the bytes behind that screen (the native window's two card images,
# the browser's card rack), and each owns a commit applier
# that turns the screen's `SaveCommit` into I/O. A host that calls a rack
# primitive from anywhere else - a hotkey, a page button, a "quick save" - has
# a save path that bypasses the retail screen, and nothing in a diff of either
# host says so. This tier pins every shipped call of each primitive to its
# host's applier (or to the primitive that wraps it).
# ---------------------------------------------------------------------------

SAVE_IO_ROUTES = [
    {
        "host": "native",
        "root": "crates/engine-shell/src/window",
        # Both ports are card images. The one other caller is the port-1
        # card's one-time import of the LGSF slot files an older build wrote
        # (`import_legacy_slots`, run when the card is first mounted): a
        # format migration of saves the player already made through the
        # screen, not a save path of its own.
        "primitives": {
            "read_slot_save": {"import_legacy_slots"},
            "write_save_into_card": {"write_save_commit", "import_legacy_slots"},
            "save_at": {"apply_card_save_commit"},
        },
    },
    {
        "host": "web",
        "root": "crates/web-play/src",
        "primitives": {
            "write_session_into_card": {"apply_card_outcome", "service_card_save"},
            "load_session_from_card": {"apply_card_outcome"},
            "write_save_into_card": {"write_session_into_card"},
            "save_at": {"load_session_from_card"},
        },
    },
]


PAGE_CARD_RACK = "site/_content/play.html"
PAGE_CARD_RACK_RULES = [
    (
        "port 1 must start with the browser card (`portSaveIds = [BROWSER_CARD, ...]`)",
        r"const\s+portSaveIds\s*=\s*\[\s*BROWSER_CARD\s*,",
    ),
    (
        "the browser card must be formatted by the engine (`formatted_memory_card`)",
        r"\bformatted_memory_card\s*\(",
    ),
    (
        "the disc load and the trap recovery must both remount the rack "
        "(`remountCards()` at least twice besides its definition)",
        r"(?s)(?:\bremountCards\(\);.*){2}",
    ),
]
