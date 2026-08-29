# Key function directory

A directory of the Ghidra-traced functions that matter for understanding Legaia's runtime. It is a lookup table split across the per-subsystem pages listed below: **grep `docs/reference/functions/` for your address**, read the row, then go to the dump or the linked subsystem page for the real detail.

## How to use this page

Every row is `| Address | Role |`, grouped into sections by subsystem. Conventions worth knowing before you search:

- **A bare hex address is a function entry point** (`8001A55C`, no `0x` prefix). Rows for things that are *not* function entries carry a `0x` prefix and say what they are - `0x8007326C` (data), `0x801DDCCC` (instruction), `0x801DFC3C` (branch target). Search on the digits alone: prose elsewhere in the corpus writes the same address as `FUN_8001a55c`, in the other case.
- **Each entry has a Ghidra dump** under `ghidra/scripts/funcs/<addr>.txt` - read it for the canonical disassembly and decompiled C. The dumps are gitignored (they are Sony-derived) and regenerable from the Ghidra project; see [`tooling/ghidra.md`](../tooling/ghidra.md).
- **Functions at `0x801C0000+` are RAM-loaded overlays**, dumped as `overlay_<label>_<addr>.txt`. This matters: several overlays share that address window, so one address can name a *different function* depending on which overlay was resident when the dump was taken. If a row's description does not match what you are reading, check you have the right overlay before concluding the row is wrong.
- **A row whose write-up outgrew its cell** keeps a one-liner plus a **[details ↓]** link into the Function details section at the bottom of its page.
- **Some addresses in the dump corpus are not addresses at all.** A dump can print a real body beside a virtual address no runtime image ever used, because the printed VA is a property of the import's load base and footprint. Before treating an unfamiliar `0x801E…`/`0x8020…` address as a new function, check [`overlay-va-aliases.md`](overlay-va-aliases.md) - it carries the measured deltas and the re-keying for the known cases.

Not finding an address here does not mean it is unknown - this directory covers the functions that anchor an explanation. [`tooling/port-catalog.md`](../tooling/port-catalog.md) tracks per-function dumped / documented / ported status across the whole corpus.

## Pages

| Page | Covers |
|---|---|
| [`functions/asset-loading.md`](functions/asset-loading.md) | Asset loading + dispatch; per-stage asset table machinery; disc / loader chain; scene / stage init (mode-0x02 loader callees). |
| [`functions/runtime-libs.md`](functions/runtime-libs.md) | PSX runtime / standard libraries (libgte, BIOS veneers); CD / file-system (libcd-style); helpers; static actor templates (the 24-byte records whose tick pointer is a routine's only reference); stub helpers. |
| [`functions/game-modes.md`](functions/game-modes.md) | Input + debug subsystem; move / animation subsystem; game-mode state machine; title overlay. |
| [`functions/battle.md`](functions/battle.md) | Battle subsystem; on-screen elements (HUD + 2D sprite/effect list); per-frame draw; sparring-tutorial overlay (PROT 0967); command-block persistence + target menu (overlay 0898); field->battle transition overlay; unreferenced SCUS entry points. |
| [`functions/script-vms.md`](functions/script-vms.md) | Script VMs; field-locomotion math helpers. |
| [`functions/renderer.md`](functions/renderer.md) | Renderer; renderer / GPU primitives; ANM animation container; MES / dialog text interpreter; dialog-overlay actor-frame helpers. |
| [`functions/audio.md`](functions/audio.md) | Audio - the libsnd / libspu stack, SsAPI sequencer, SPU transfer engine, XA streaming. |
| [`functions/menus.md`](functions/menus.md) | Records / stats screen; field-overlay status / equip panels (overlay 0897); inventory / spell list; shop screen panels; menu / HUD globals; menu-overlay callees (PROT 0899). |
| [`functions/world-map.md`](functions/world-map.md) | World map - controller, dev menu, render pipeline. |
| [`functions/minigames-debug.md`](functions/minigames-debug.md) | Minigames; debug-menu overlay (PROT 0971, mode-0 CONFIG); other-game minigame overlay (PROT 0977). |

## See also

- [`docs/subsystems/script-vm.md`](../subsystems/script-vm.md) - the field/event VM dispatcher (`FUN_801DE840`) anchoring many of these entries.
- [`docs/subsystems/battle-action.md`](../subsystems/battle-action.md) - the battle-action SM (`FUN_801E295C`) and its helpers.
- [`docs/reference/memory-map.md`](memory-map.md) - the RAM globals these functions read and write.
- [`docs/tooling/ghidra.md`](../tooling/ghidra.md) - how the dumps backing this directory are produced.
- [`docs/reference/overlay-va-aliases.md`](overlay-va-aliases.md) - the measured VA-offset aliases in the dump corpus, and which printed addresses are phantoms.
