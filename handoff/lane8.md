# Lane 8 handoff — battle / move-VM / effect slice of the disclosed-inert worklist

Audit before and after are identical: `ported + live` **687**, `disclosed inert`
**239**, `undisclosed inert` 0, `stale NOT WIRED` 0. No anchor in this slice was
wireable from inside it, and the report below says per anchor why.

## For LANE 7 — overturned verdicts for `live-audit-triage.md`

Two rows in that page's `DISCLOSE` list need their reason replaced. Both are
"the disclosure named the wrong blocker", the shape the page already tracks.

**`801e36c4` / `801e373c` / `801e3ee0` / `800198e0` (`title_prim.rs`).** The
verdict (`DISCLOSE`, "no host supplies a `PrimHost`") **holds** — verified, the
only `impl PrimHost` in the workspace is inside that file's `#[cfg(test)]`
module. But the port under one of those tags was wrong in three ways and is
fixed on this branch; see the new section in `stale-not-wired-triage.md`
("What a correct disclosure still does not tell you"). No edit needed to your
page beyond, optionally, a pointer — the row itself was right.

**`80046870` `advance_gauge` / `80046898` `item_count_gate`.** Your page's
`WIRE` row for `item_count_gate` is landed and correct. What changed is the
*meaning* of the word it gates: `gp + 0x2E8` is `_DAT_8007B600` and holds
**frames of cooldown remaining**, not an inventory item count. Derivation and
evidence are in `docs/subsystems/battle-action.md` and in the two source docs.
If your page repeats the "224-slot cap" phrasing anywhere, it needs the same
correction.

## Wires that are real but need a host outside this lane's scope

Each of these is a genuine `WIRE` with every piece present except one call, and
each lands in a file this lane does not own. They are not disclosure gaps —
the disclosures are accurate — they are queued work.

| Anchor | Where the call goes | What is already built |
|---|---|---|
| `801d095c` `passive_hud_icons` / `hud_anchor_offsets` (`field_passive_hud.rs`) | `engine-shell` `window/event_handler/redraw_passes.rs`, beside the effect-billboard build | bit source `engine-core::accessory_passives`; projector `engine-render` `Camera::transform`; icon primitive `engine-ui` `PainterPictogram`. Only the pass that projects a party member's head anchor and feeds `sprite_draws_for` is missing. |
| `800468a4` `enqueue` / `80057914` `build_packet` (`vram_rect_copy.rs`) | an `engine-render` implementor of `FieldHost::op43_vram_rect_copy` (today a no-op default) | `op43_sub12_calls` is already live and hands the resolved calls to the trait method; the receiver needs a GP0-level host owning an ordering table + back-buffer flag. |

## Follow-ups this lane deliberately did not take

**Rename `item_count_gate` / `ActionValidatorHost::inventory_count`.** Both
names preserve a reading now falsified. Renaming needs three surfaces changed
together — `crates/engine-vm/src/battle_action/validator.rs`,
`docs/tooling/live-audit-triage.md` (lane 7) and
`site/_content/subsystems/battle-action.html` (no lane) — so this lane fixed
the prose in place and left the identifiers, with a source note saying why.

**The `lui 0x8008` + negative-displacement slip belongs in `ghidra.md`.** It has
now produced three wrong global names in committed source (`_DAT_80084500` and
`_DAT_8008454C` in `title_overlay.rs`, fixed here; `0x800846A8` for
`0x8007B6A8` in the pause-menu save gate, fixed earlier). It is a *disassembly*
transcription hazard rather than a decompiler artifact, so it wants its own
short entry beside `docs/tooling/ghidra.md` § decompiler artifacts. The
mechanism is written up in `crates/engine-vm/src/title_overlay.rs`'s provenance
block; someone who owns `docs/tooling/ghidra.md` should lift it.

**`crates/save/src/character.rs` has no accessor at record-relative `+0x1A7` /
`+0x1B7`.** That single gap is the whole prerequisite for
`preseed_action_queue` / `save_action_queue` (`801da34c` / `801da59c`): the
engine *does* carry saved arts-input strings (`SavedChainRecord`,
`World::saved_chains`, filled by `--player-battle`), it just cannot project them
onto retail's two slots. `crates/save/` is outside this lane.
