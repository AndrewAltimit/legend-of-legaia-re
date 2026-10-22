"""Tier 12 rows: the crates whose `pub fn`s count as the engine API, and the
method names that do not.

Analysis: `ui_host_drift.tiers.frame_content`.
"""

# --------------------------------------------------------------------------
# Tier 12 - content: do two PAIRED frame kernels call the same engine?
#
# Tier 11 pairs a frame path's kernels by NAME (or by an alias row). That
# answers "does each host take this step", and says nothing about what the
# step does on each side - which is the whole of the question the pairing
# invites a reader to assume it answered. A kernel whose native body is
# `{}` pairs perfectly with a browser twin that drains a cue queue and
# advances every clip player.
#
# This tier asks the next question with the only evidence a source scan can
# carry: for each paired kernel, the set of ENGINE functions each host's body
# reaches. Engine = the four wgpu-free crates both hosts link
# (`engine-core` / `engine-vm` / `engine-ui` / `engine-audio`); a host's own
# helpers are followed transitively, so a step spelled as five private
# methods is compared against a twin that inlines them.
#
# What it proves: two paired bodies reach the same engine surface, or the
# difference is written down with a reason. What it does NOT prove: that they
# call it with the same arguments, in the same order, or under the same
# guard. Those are tier 3's question and the audits' - a difference this tier
# cannot see is not evidence of agreement.
#
# The join is by NAME, which carries one deliberate blind spot: nothing here
# can tell `world.clear()` from `Vec::clear()`. Names that are also ordinary
# std / collection / iterator methods are therefore excluded wholesale
# ([`STD_METHOD_NAMES`]) rather than guessed at, and a genuinely divergent
# engine call that happens to be spelled `insert` is invisible. Stating the
# hole is the point: the alternative is a report where two thirds of every
# row is `len`, which is a report nobody reads.
# `engine-session` is engine surface too: both play hosts hold its
# `BootSession` and tick it, so a call into it is a call both can make.
# `engine-screens` likewise: the shop-family composition both hosts call.
# `engine-battle` holds the `World`-free battle kernels,
# `engine-battle-vm` the battle VM kernels, `engine-cast` the cast-module
# kernels, `engine-effects` the effect
# kernels and `engine-mode` the mode seat. The hosts name each of these
# crates directly, so the list is every engine crate a host can call into.
ENGINE_API_CRATES = (
    "engine-core",
    "engine-menu-runtime",
    "engine-scene-host",
    "engine-world-views",
    "engine-battle",
    "engine-effects",
    "engine-field",
    "engine-scene",
    "engine-menus",
    "engine-title",
    "engine-battle-menus",
    "engine-mode",
    "engine-dialog",
    "engine-system",
    "engine-minigames",
    "engine-fishing",
    "engine-minigame-scenes",
    "engine-vm",
    "engine-battle-vm",
    "engine-cast",
    "engine-motion-vm",
    "engine-ui",
    "render-kernels",
    "engine-audio",
    "engine-session",
    "engine-screens",
)

# Names a `.name(` call cannot be attributed to the engine by name alone.
# Ordinary std / core / collection / iterator methods that an engine type
# also happens to define.
STD_METHOD_NAMES = {
    "abs", "add", "all", "and_then", "any", "append", "as_deref", "as_mut",
    "as_ref", "as_slice", "as_str", "bytes", "chain", "chars", "checked_add",
    "checked_sub", "chunks", "clamp", "clear", "clone", "cloned", "cmp",
    "collect", "contains", "contains_key", "copied", "count", "default",
    "deref", "div", "drain", "encode", "ends_with", "entry", "enumerate",
    "eq", "expect", "extend", "fill", "filter", "filter_map", "find",
    "first", "flat_map", "flatten", "fmt", "format", "from", "from_le_bytes",
    "get", "get_mut", "hash", "index", "index_mut", "insert", "into",
    "into_iter", "is_empty", "is_none", "is_some", "iter", "iter_mut",
    "join", "keys", "last", "len", "lines", "load", "map", "max", "min",
    "mul", "ne", "neg", "new", "next", "not", "ok", "ok_or", "or_else",
    "or_insert", "or_insert_with", "parse", "partial_cmp", "pop", "position",
    "print", "println", "push", "read", "rem", "remove", "replace",
    "reserve", "resize", "retain", "rev", "saturating_add", "saturating_sub",
    "set", "slice", "sort", "sort_by", "sort_by_key", "splice", "split",
    "starts_with", "step_by", "store", "sub", "sum", "swap", "swap_remove",
    "take", "to_le_bytes", "to_lowercase", "to_owned", "to_string",
    "to_uppercase", "to_vec", "trim", "truncate", "unwrap", "unwrap_or",
    "unwrap_or_default", "unwrap_or_else", "values", "values_mut",
    "windows", "with_capacity", "wrapping_add", "wrapping_sub", "write",
    "zip",
}


# Host fns the walk does not enter. The play page's `audio_director` builds
# the shared `AudioDirector` the first time anything needs it and stages
# its resident banks against the disc - the native session's boot staging,
# which no native frame kernel reaches either. Following it would charge
# that one-time install to every frame step that fires a cue.
CONTENT_WALK_STOPS = {"audio_director"}
