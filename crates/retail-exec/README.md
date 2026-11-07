# psx-retail-exec

A user-mode R3000 interpreter: integer instructions, `hi` / `lo`, unaligned
loads and stores, RAM and the scratchpad, with no GTE and no COP0. The crate's
disc-gated tests load `SCUS_942.54` and the overlays off the user's disc, run a
retail routine over generated inputs and compare what it wrote with the Rust
kernel whose `// PORT:` tag names it.

The method and how to add a routine: [`docs/tooling/retail-kernel-differential.md`](../../docs/tooling/retail-kernel-differential.md).

No Sony bytes live here. Without `LEGAIA_DISC_BIN` the disc-gated tests skip.
