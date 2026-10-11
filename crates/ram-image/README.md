# legaia-ram-image

Emulator-neutral analysis of a PSX main-RAM image taken from a Legend of
Legaia save state. Every function takes the 2 MiB of main RAM as a byte
slice (index `0` is virtual address `0x80000000`) and does not care which
emulator captured it.

| Module | Covers |
|---|---|
| `extract` | Locate main RAM in a raw payload by anchor string; slice a virtual-address window out of it. |
| `game_anchors` | The game's own globals read from the image (scene name, game mode, player pointer and position) and state identification. |
| `diff` | Pairwise main-RAM diff between two states: the watchpoint-equivalent "what changed". |
| `bisect` | Given a known-good and a known-bad state, the intermediate states that isolate a write. |
| `source_hunt` | Hunt for where a value comes from across states. |
| `prim_pool`, `prim_dispatch` | Decode the GPU primitive pool and the primitive dispatch table resident in the image. |
| `scenarios` | The capture-scenario manifest [`scripts/scenarios.toml`](../../scripts/scenarios.toml). |

See [`mednafen-automation.md`](../../docs/tooling/mednafen-automation.md) for
the workflow these serve.

## Layering

A leaf: `anyhow`, `serde` and `toml` only. The two container readers,
[`legaia-mednafen`](../mednafen/README.md) (`.mc*` states) and
[`legaia-pcsx-redux`](../pcsx-redux/README.md) (PCSX-Redux `.sstate`), sit on this crate
and hand it the RAM they unwrap. It must not depend on either of them, nor on
any format or engine crate.
