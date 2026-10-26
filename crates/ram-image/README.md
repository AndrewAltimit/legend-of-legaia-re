# legaia-ram-image

Emulator-neutral analysis of a PSX main-RAM image taken from a save state.
Every function takes the 2 MiB of main RAM as a byte slice (index `0` is
virtual address `0x80000000`) and does not care which emulator captured it;
what it needs to know about the game comes in as a `GameProfile`.

| Module | Covers |
|---|---|
| `profile` | `GameProfile`: the per-game parameters (anchor strings, identity globals, manifest defaults) the generic readers take; `GenericGame` for a title nobody has described. |
| `extract` | Locate main RAM in a raw payload by anchor string (any game's, given its executable); slice a virtual-address window out of it. |
| `game_anchors` | Legaia's globals read from the image (scene name, game mode, player pointer and position), state identification, and the `Legaia` profile. |
| `diff` | Pairwise main-RAM diff between two states: the watchpoint-equivalent "what changed". |
| `bisect` | Given a known-good and a known-bad state, the intermediate states that isolate a write. |
| `source_hunt` | Hunt for where a value comes from across states. |
| `prim_pool`, `prim_dispatch` | Decode the GPU primitive pool and the primitive dispatch table resident in the image. |
| `scenarios` | The capture-scenario manifest [`scripts/scenarios.toml`](../../scripts/scenarios.toml). |

See [`mednafen-automation.md`](../../docs/tooling/mednafen-automation.md) for
the workflow these serve.

`diff`, `bisect`, `ram_slice` and `prim_pool` take any game's 2 MiB image.
The three places game knowledge enters are parameters:

- **Anchor RAM search.** `extract::main_ram_via_exe_anchors(payload, exe,
  strings)` matches the caller's strings in the caller's executable and reads
  the load address from its `PS-X EXE` header; `main_ram_via_profile` takes
  the strings from a profile. The older `main_ram_via_anchor` /
  `main_ram_via_anchor_with_scus` are the same search with Legaia's strings
  (`game_anchors::EXE_ANCHORS`, still re-exported as `extract::ANCHORS`).
- **State identity.** `GameProfile::identify` reads a state's scene / mode /
  player position; `GenericGame` returns `None` rather than reading Legaia's
  addresses out of another game's RAM.
- **Scenario manifest.** `ScenarioManifest::parse_toml_for(text, profile)`
  fills a `[defaults]` field the text leaves out (`filename_pattern`,
  `mcs_dir_env`) from the profile; `parse_toml` / `from_path` keep Legaia's
  defaults, so the existing callers read `scripts/scenarios.toml` exactly as
  before.

`prim_dispatch` and `source_hunt` stay Legaia's; see
[reusing for other PS1 games](../../docs/tooling/reusing-for-other-ps1-games.md#emulator-save-states-legaia-mednafen-legaia-pcsx-redux-legaia-ram-image).

## Layering

A leaf: `anyhow`, `serde` and `toml` only. The two container readers,
[`legaia-mednafen`](../mednafen/README.md) (`.mc*` states) and
[`legaia-pcsx-redux`](../pcsx-redux/README.md) (PCSX-Redux `.sstate`), sit on this crate
and hand it the RAM they unwrap. It must not depend on either of them, nor on
any format or engine crate.
