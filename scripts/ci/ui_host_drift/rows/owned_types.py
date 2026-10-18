"""Tier 8 rows: engine types both hosts must own, and the session type that
may own them on a host's behalf.

Analysis: `ui_host_drift.tiers.owned_types`.
"""

from ui_host_drift.roots import REPO

# --------------------------------------------------------------------------
# Tier 8 - ownership: does each host OWN the engine type, or only read it?
# --------------------------------------------------------------------------
#
# Every tier above asks whether a host *reaches* something - a builder, a
# constant, a kernel, a gate. None of them can see the shape where a host
# reaches an engine type's outputs without ever holding the type.
#
# That shipped, and it produced four different-looking symptoms at once. The
# browser play page framed the field with a spherical orbit of its own while
# the native window consumed `engine_core::camera::Camera`; read as "two
# projections" it is a rendering difference, and it was not. The page held no
# `Camera`, so nothing on that host routed the op-`0x45` Configure beats into
# a controller, advanced the mover, wrote the follow focus back into the
# retail camera globals or reset them on scene entry - and the azimuth it fed
# the locomotion compass came from its own orbit yaw. One absence, a
# projection difference, a simulation difference and two missing screens.
#
# Tier 1 cannot see it (no builder is missing), tier 2 cannot (no constant is
# paired), tier 3 cannot (an injection site that does not exist cannot
# diverge from one that does), tier 7 cannot (both surfaces name their own
# kernels). What distinguishes the state is **ownership**: a field declaration
# or a construction, in that host's own shipped sources.
#
# Declared rather than derived, for the same reason [`CONSTANT_PAIRS`] and
# [`SIM_PAIRS`] are: "which engine types must a host own" is a judgement about
# the architecture. The derived version of the question (every `engine-core`
# type one host constructs and the other only names) reports 26 rows over this
# tree, and every one is a type BOTH hosts use where one names a constructor
# (`PadButton::from_name`) or holds a session the other reaches through its
# runtime - noise, not a Camera-shaped absence. A declared row is a pinned
# juncture; it does not claim to be a census.
#
# Scope, as narrowly as the tiers above:
#
# * it DOES prove each named type is constructed or held as a field by both
#   hosts' shipped code, and that the type still exists in `engine-core`;
# * it does NOT prove the two hosts drive it the same way, tick it at all, or
#   read the same outputs off it.

OWNED_TYPES: list[dict[str, object]] = [
    {
        "type": "Camera",
        "defined_in": "crates/engine-field/src/camera/state.rs",
        "why": "which camera owns a frame and what its retail GTE inputs are "
        "is one engine question; a host that renders a field without holding "
        "the type re-implements the mover, the compass azimuth and the "
        "op-0x45 cutscene beats in its own projection, and every other tier "
        "stays green while it does",
    },
    {
        "type": "SceneHost",
        "defined_in": "crates/engine-scene-host/src/scene_host.rs",
        "why": "the scene host is what drains the per-frame queues the field "
        "VM fills - the minigame door warp, the staged menu warp, the BGM "
        "route. A host that keeps a bare `World` instead reaches the same "
        "world state and none of the drains",
    },
    {
        "type": "MenuRuntime",
        "defined_in": "crates/engine-menu-runtime/src/menu_runtime.rs",
        "why": "the save/load menu's disk-facing runtime. A host that opens "
        "the pause menu without one has the screens and no slot state behind "
        "them",
    },
]


# The session type both hosts hold. A host that holds one owns every type its
# fields hold - the scene host and the camera are the session's, and a host
# reaches them through it.
SESSION_TYPE = "BootSession"
SESSION_SRC = REPO / "crates/engine-session/src/session.rs"
# Where the session crate declares aliases of the session type over one
# audio output (`NativeSession`). A host that holds a field of an alias
# holds the session.
SESSION_ALIAS_SRC = REPO / "crates/engine-session/src/native.rs"
