#!/usr/bin/env python3
"""Tests for the `lui` pair arm of find-address-word-refs.py.

Dependency-free: importable by pytest (`test_*` functions) and runnable
standalone (`python3 test_find_address_word_refs.py`). The images are
synthetic hand-assembled words, so the tests need no disc and carry no
Sony bytes.
"""
from __future__ import annotations

import importlib.util
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
_spec = importlib.util.spec_from_file_location(
    "find_address_word_refs", HERE / "find-address-word-refs.py"
)
refs = importlib.util.module_from_spec(_spec)
sys.modules["find_address_word_refs"] = refs
_spec.loader.exec_module(refs)

BASE = 0x80010000
TARGET = 0x801CE330  # low half 0xE330 is negative: high half takes the +1


def lui(rt, imm):
    return (0x0F << 26) | (rt << 16) | imm


def itype(op, rs, rt, imm):
    return (op << 26) | (rs << 21) | (rt << 16) | (imm & 0xFFFF)


def image(*words):
    data = struct.pack(f"<{len(words)}I", *words)
    return refs.Image("synthetic", data, BASE, "scus")


def forms(img):
    return [form for _, _, form in refs.lui_pair_hits(img, TARGET)]


V0, V1, A0 = 2, 3, 4


def test_addiu_and_ori_still_found():
    img = image(lui(V0, 0x801D), itype(0x09, V0, V0, 0xE330))
    assert forms(img) == ["addiu"]
    img = image(lui(V0, 0x801C), itype(0x0D, V0, V0, 0xE330))
    assert forms(img) == ["ori"]


def test_every_memory_opcode_completes_a_pair():
    for op, name in refs.MEM_OPS.items():
        img = image(lui(V0, 0x801D), itype(op, V0, V1, 0xE330))
        assert forms(img) == [name], (name, forms(img))


def test_memory_form_takes_the_signed_high_half():
    # `lui 0x801C` + `lw -0x1cd0` is 0x801BE330, not the target.
    img = image(lui(V0, 0x801C), itype(0x23, V0, V1, 0xE330))
    assert forms(img) == []


def test_memory_form_must_use_the_lui_register_as_base():
    img = image(lui(V0, 0x801D), itype(0x23, A0, V1, 0xE330))
    assert forms(img) == []


def test_memory_form_after_an_index_is_marked():
    addu_v0_v0_a0 = (V0 << 21) | (A0 << 16) | (V0 << 11) | 0x21
    img = image(lui(V0, 0x801D), addu_v0_v0_a0, itype(0x24, V0, V1, 0xE330))
    assert forms(img) == ["lbu+index"]


def test_clobbered_base_is_not_a_pair():
    # `lw v0,0(v0)` replaces the high half with a loaded pointer.
    img = image(lui(V0, 0x801D), itype(0x23, V0, V0, 0), itype(0x23, V0, V1, 0xE330))
    assert forms(img) == []


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"ok  {name}")
