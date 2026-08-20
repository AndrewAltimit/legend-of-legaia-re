# Lane C wiring — enemy-anim mirror (`lane/enemy-anim-mirror`)

Everything below is implemented and disc-tested on this branch; only the
call site into `apply_delilas_party` is left to the coordinator (the lane
was forbidden from editing `crates/patcher/src/delilas_party.rs`).

## What shipped

- `crates/asset/src/party_swap/enemy_anim.rs` (new) — shared bake context
  (`monster_bake_ctx`), the hero→monster clip retarget (`HeroRetarget`),
  in-place monster-block entry rewriting (`rebuild_block_entries` +
  `mirror_block_animations`), and the bake-parity affine-fit instrument
  (`monsterize_fit_report`).
- `crates/asset/src/party_swap.rs` — `monsterize_player` now bakes
  through `playerize::bake_frames` (whole-rig re-face + minimal swing)
  over a terminal-normalized rest, both read from the shared context.
  This is goal 1 (bake parity) and needs no wiring: it is live for every
  existing `swap_into_block` caller.
- `crates/patcher/src/enemy_anim_mirror.rs` (new) —
  `apply_enemy_anim_mirror(patcher, mapping, &RetailSources)` plus the
  per-sibling `staged_entries` table and the budget drop-ladder.
- `crates/patcher/tests/enemy_anim_mirror_real.rs` (new, disc-gated).

## The one call the coordinator adds

At the END of `apply_delilas_party` (inside the `if report.changed {}`
block, after the signature-art / moveset loop — i.e. after every pass
that touches the monster slots, the player files or readef), add:

```rust
let retail = crate::enemy_anim_mirror::RetailSources {
    archive: &archive,                 // already captured pre-model-loop
    players: [
        &retail_players[0],            // already captured pre-model-loop
        &retail_players[1],
        &retail_players[2],
    ],
    readef: &retail_readef,            // NEW capture, see below
};
report
    .notes
    .extend(crate::enemy_anim_mirror::apply_enemy_anim_mirror(
        patcher, mapping, &retail,
    )?);
```

and capture readef next to the existing `retail_players` capture at the
top of the function (BEFORE any patching):

```rust
let retail_readef = patcher
    .read_entry_footprint(READEF_ENTRY)
    .context("read retail readef.DAT")?;
```

## Ordering constraints the pass was designed for (do not violate)

1. **After the model loop.** The pass rewrites the *swapped* blocks in
   place and refuses (by block-name gate) to run on a disc whose target
   blocks are not yet named after the mapped heroes. Running it before
   `swap_into_block`/`rename_block` errors cleanly.
2. **Retail inputs must be pre-patch captures.** `apply_delilas_party`
   itself rewrites the player files (playerize), the readef ME slots
   (`reskin_signature_art`, `--delilas-moves delilas`) and the monster
   archive. The `RetailSources` images must be read before ANY of that.
   The existing `archive` / `retail_players` captures in
   `apply_delilas_party` already satisfy this; `readef` needs the one
   extra capture shown above.
3. **Idempotent + skip-safe.** Entry content is a pure function of the
   retail sources, so re-running on an already-mirrored disc reproduces
   it byte for byte. If `apply_delilas_party` skipped a pairing as
   already-applied, running the mirror again is a no-op for that block.
4. **Lane A contract respected.** The cast-module code and its staged
   indices are untouched; only staged-entry CONTENT changes; entry count
   and index space are preserved; every module-staged entry (chain +
   close) keeps ≥ 23 keyframes (`MIN_STAGED_FRAMES`). When lane A
   delivers per-module gate facts, the floor can be relaxed per module by
   threading a per-block minimum into `mirror_block_animations`.

## Behaviour summary (for the user-facing note / docs)

Per swapped block: idle ← hero idle; walk ← hero walk (retail root
motion kept); flinch/knockdown/get-up/block ← the hero's same-tag
clips; AI-rollable attack entries ← the hero's default-equipment weapon
swings (round-robin); the staged signature chain ← the hero's 50-AP
Hyper (Burning Flare / Vulture Blade / Explosive Fist) split
wind-up→strike across the module's staged entries, duration-preserving
(`frames * 8 / rate` invariant); the closing staged entry (also the
monster's tag-0x22 victory pose) ← the hero's base-ME victory flourish.
Everything is rigidly re-anchored on the block's own rest (torso x/z +
deepest-ankle floor). Frame-indexed head fields (event beats, effect
gates, loop windows) rescale with the new stream lengths; sound cues,
AGL costs, tags, effect indices and root motion stay retail. On slot
overflow a ladder drops attacks → walk → reactions before failing
(never the idle or the staged chain); no ladder step was needed on the
tested mappings.

Docs: `docs/tooling/randomizer.md` § "Delilas party swap" should gain a
paragraph on the mirror once the wiring lands (left to the coordinator
so the docs describe wired behaviour).
