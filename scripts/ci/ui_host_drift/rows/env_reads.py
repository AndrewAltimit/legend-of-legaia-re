"""Tier 15 rows: the engine crates scanned for process-environment reads and
the reads allowed there.

Analysis: `ui_host_drift.tiers.env_reads`.
"""

from ui_host_drift.roots import REPO

# --------------------------------------------------------------------------
# Engine environment reads.
# --------------------------------------------------------------------------
#
# A `wasm32` build has no process environment: `std::env::var` answers `Err`
# and `var_os` answers `None` there. A gameplay switch read from the
# environment inside an engine crate is therefore one the browser can never
# throw - the native window has it and the play page silently does not. A
# switch belongs on `WorldToggles` (or another engine field), written by each
# host: the native session from its environment / command line
# (`engine-session/src/native.rs`), the play page from a `LegaiaRuntime`
# setter.
#
# Every `crates/engine-*` crate a browser host links is scanned, plus
# `render-kernels`. Left out, each because no wasm host links it: the native
# binary `engine-shell`, the wgpu renderer `engine-render`, the `.glb`
# exporter `engine-export` and the capture tooling `engine-capture`.
ENV_KERNEL_ROOTS = [
    REPO / "crates" / name / "src"
    for name in (
        "engine-audio",
        "engine-battle",
        "engine-battle-vm",
        "engine-cast",
        "engine-camera",
        "engine-core",
        "engine-menu-runtime",
        "engine-scene-host",
        "engine-world-views",
        "engine-dialog",
        "engine-effects",
        "engine-field",
        "engine-fishing",
        "engine-menus",
        "engine-title",
        "engine-battle-menus",
        "engine-minigames",
        "engine-minigame-scenes",
        "engine-mode",
        "engine-motion-vm",
        "engine-scene",
        "engine-screens",
        "engine-session",
        "engine-system",
        "engine-ui",
        "engine-vm",
        "render-kernels",
    )
]

# The native host's half of `engine-session`: compiled for native targets
# only, and the one place a native session turns environment variables into
# world toggles. Files at or under these paths are not scanned.
ENV_NATIVE_HOST_PATHS = [
    REPO / "crates" / "engine-session" / "src" / "native.rs",
    REPO / "crates" / "engine-session" / "src" / "native",
]

# Name prefixes allowed anywhere in the scanned roots. `LEGAIA_DIAG_*` gates
# are declared and classified by tier 6 (`rows/diag_gates.py`), which is
# where an additive one is made to carry a browser twin.
ENV_ALLOWED_PREFIXES = {
    "LEGAIA_DIAG_": "diagnostic gates, declared in rows/diag_gates.py (tier 6)",
}

# Exact names allowed, each with the reason it is not a gameplay switch.
ENV_ALLOWED_NAMES = {
    "LEGAIA_DISC_BIN": (
        "an inline `#[cfg(test)]` module's disc gate; test code, never a "
        "host's switch"
    ),
}
