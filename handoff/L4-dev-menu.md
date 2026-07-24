# Lane L4 handoff (debug menu / dev menu / PROT 0977)

## URGENT - the shared `git stash` stack is unsafe across worktrees

`git stash` is **repository-global**: every lane's worktree shares one stack.
While checking whether a test failure was pre-existing this lane ran one
`git stash push`/`pop` pair; the pop restored a *different* lane's work into
this worktree and this lane's own entry was dropped by a concurrent lane.
Both were recovered:

- Another lane's **field-locomotion** work (`field_audio_release.rs`,
  `world/field_movement.rs`, `field_actor_reflect.rs`, `field_ledge_hop_arc.rs`,
  `field_state_pick.rs`, `engine-vm/src/lib.rs`, `docs/subsystems/field-locomotion.md`,
  `motion-vm.md`, `script-vm.md`) is back on the stash stack, labelled
  `recovered: field-locomotion lane work popped into
  worktree-agent-a9e6a9e16b1604389 by mistake; restored untouched`.
  A copy of the same diff is at
  `/tmp/claude-1000/-home-mikunpc-Documents-repos-legend-of-legaia-re/OTHER_LANE_RECOVERED.patch`.
- A **baka_fighter** lane's work sits in the stash as
  `recovered: WIP on worktree-agent-a06d4881a31fb277c (popped into the wrong
  worktree by lane L2; restored)` - lane L2 hit the same trap earlier.

**No lane should run `git stash` in this wave.** The lane brief should say so.

## Findings for other lanes' files (not edited here)

1. `scripts/ghidra-analysis/worklist-classification.csv` row `801d26cc` reads
   `identified: debug-menu(971) overlay widget SM ... dev-tool overlay,
   outside the retail menu port surface`. **Falsified.** The body is the
   fishing overlay's hooked-fish handler: it is byte-identical in
   `overlay_fishing.bin`, `overlay_slot_machine.bin` and
   `overlay_debug_menu.bin`, the fishing dump resolves its string references
   to `hit_type %d` / `Type 1` / `Type 2` / `Type 3`, and
   `locate-entry-image.py 0x801d26cc` frames it in PROT **0972 (fishing)** and
   in no other based image. Same for `801d2050`, `801d2278`, `801d4948`,
   `801d5c2c` - all five are 0972 entries already documented in
   `docs/subsystems/minigame-fishing.md`.

2. `docs/tooling/overlay-capture.md` says of the hub captures: "they
   **VA-alias** - they are distinct files sharing a library core, so a given
   address hosts a *different* function per minigame; always read the
   overlay-qualified dump." At the five VAs above the three captures are
   **byte-identical**, and `locate-entry-image.py` finds a frame in 0972 only.
   The likely mechanism: PROT 0971 (`debug_menu`) is far shorter than the
   capture window, so everything above its footprint in the `debug_menu`
   capture is the *previous* slot-A occupant (fishing). The page should say
   the aliasing claim is per-address, not per-image, and that an
   `overlay_debug_menu_` prefix above 0971's footprint is stale RAM.

3. `docs/subsystems/minigame-fishing.md` is correct but incomplete on three
   points now read out of the disassembly:
   - `FUN_801D5C2C` does not stop at the depth clip: it **projects** both
     endpoints (`sx = x * ((proj << 12) / z) >> 12 + 0xA0`, `sy` likewise
     `+ 0x78`) into the two output pairs. And its two clip arms are
     asymmetric - the far-endpoint arm interpolates with `1 - t` rather than
     `t`, so only that endpoint's `z` really lands on the bound.
   - `FUN_801D4948`'s sub-state list (`0` arm / `1` attach / `2` track) is
     missing state **`4`**, the catch celebration, which is the bulk of the
     routine (burst tiers at score `> 200 / 600 / 800 / 0x4B0` with cues
     `0x25 / 0x26 / 0x27` and a silent top tier; stage frames `0x72 / 0x87 /
     0xD2` on the actor's `+0x22` timer).
   - `FUN_801D26CC`'s bite-interval ladder is six `slti`/`bne` pairs written
     in **ascending** threshold order over one register, so the four
     intermediate cadences (200 / 350 / 400 / 500) are unreachable: only
     `> 200 -> 1000`, `== 200 -> 512` and `< 200 -> 2000 (bias -100)` survive.
     A debug override (`_DAT_8007B9B0` set **and** held pad bit `0x2`) forces
     the cadence to `0x20`.

4. `docs/subsystems/cutscene.md` section "MDECin DMA-callback hook" has a
   sibling it does not name: `FUN_801CFE20` / `FUN_801CFE5C` are the **MDEC
   in/out DMA sync** entries, also byte-identical in PROT 0970 and 0971. Each
   takes one argument - `0` blocks (`FUN_801D0100` / `FUN_801D0198`, a
   `0x100000`-spin countdown that logs `"MDEC in sync"` / `"MDEC out sync"`
   and returns `-1` on timeout), non-zero polls a single bit of the status
   word `FUN_801D0230` returns. The out-side **poll** reads the *in*-side
   word (bit `0x18`); only its blocking arm reads the out-side pointer.

5. Pre-existing failing test, untouched by this lane and reproduced on a clean
   tree: `legaia-engine-core --lib
   world::battle::casting::capture_bypass_tests::the_bypass_wrapper_weights_the_defence_terms_more_heavily`.

6. `crates/engine-vm/src/world_map_overlay.rs` gained eight new
   "tagged `NOT WIRED` but analysed live" rows as a side effect of this lane's
   wiring, and only one of them is a genuine stale tag. `dev_equip_commit`'s
   `commit_equip` is now reached from the dev-menu host, and it calls that
   file's `resolve_equip_slot`, so **`801e5b4c` really is live now** and its
   disclosure is stale. The other seven (`801ead98`, `801eca08` x2,
   `801ed710` x3, plus the `801e5b4c` module row) are the module-anchor
   over-report: the file's `//!` header carries five address tags at once, so
   one live symbol marks all five live. Whoever owns that file should move
   those five tags onto the functions and types that implement them, the way
   `world_map_dev_menu.rs`, `dev_menu_list.rs` and `debug_char_editor.rs` were
   changed here. Nothing in this lane edited `world_map_overlay.rs`.
