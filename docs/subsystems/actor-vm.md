# Actor / sprite VM

A small fixed-width VM driving the title screen's animated sprite cluster. Distinct from the much larger [field/event VM](script-vm.md). Lives in the title-screen overlay at `FUN_801D6628`; 13-opcode dispatch table at `0x801CED70`.

## Overview

The VM walks an actor list of fixed-size structs; each actor has a small amount of per-instance state and a bytecode cursor that advances over time. Opcodes are 1 byte (no operand-byte prefix), and the operand structure is per-opcode — typically zero or one byte.

## Opcodes

The 13 opcodes cover the basics every sprite-animation system needs:

- Spawn / despawn actors.
- Set / clear a per-actor flag bit (mirrors the lower script-VM banks).
- Position writes (immediate and packed).
- Motion: linear interpolation between two endpoints.
- Trigger an animation (an ANM container indexed by id).
- Wait / yield.
- Conditional skip on a flag.
- Terminator.

Full opcode table + Rust port: `crates/engine-vm/src/lib.rs`.

## Why it's separate from the field VM

The actor VM is a fixed-width 13-opcode dispatcher tailored to the title screen's sprite-walk loop. The field VM (`FUN_801DE840`) is a 43-opcode variable-length dispatcher with cross-context targeting, halt-acquire semantics, sub-dispatcher families, and far richer ctx state. They serve different layers of the engine — actors at the rendering primitive level, scripts at the gameplay-event level — and were almost certainly written by different people on the dev team.

## Connection to ANM

Opcode "trigger animation" hands off an ANM container ID to the animation runner. The container's record body (the per-record bytecode) is overlay-resident (likely in the same overlay as the actor VM) — `crates/anm` parses the container shape but the per-record interpreter still has to be reversed.
