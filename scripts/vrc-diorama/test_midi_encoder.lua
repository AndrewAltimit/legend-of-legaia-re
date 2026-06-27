-- test_midi_encoder.lua  -- offline validation of the battle-diorama MIDI encoder.
--
-- Run:  luajit scripts/vrc-diorama/test_midi_encoder.lua
-- (run codegen.py first so generated/registers.lua exists).
-- No emulator / no PCSX: drives the encoder with synthetic BattleState tables.

local HERE = (arg[0]:match("(.*/)") or "./")
package.path = package.path
    .. ";" .. HERE .. "?.lua"
    .. ";" .. HERE .. "generated/?.lua"

local enc = require("midi_encoder")
local REG = require("registers")

local fails = 0
local function check(cond, msg)
    if cond then print("  ok  " .. msg)
    else print("  FAIL " .. msg); fails = fails + 1 end
end

-- Index a message list by (ch,cc) -> value (last wins).
local function index(msgs)
    local m = {}
    for _, x in ipairs(msgs) do m[x.ch * 256 + x.cc] = x.val end
    return m
end
local function get(idx, ch, cc) return idx[ch * 256 + cc] end

-- Build a synthetic BattleState in the shape probe.battle_state.read() returns.
local function make_state(opts)
    opts = opts or {}
    local st = {
        render_mode = opts.render_mode or 0x15,
        game_state = 0,
        ctx = 0x80120000,
        active_slot = opts.active_slot or 0,
        party_action_id = 0xFF,
        monster_action_id = 0,
        turn = 1,
        in_battle = (opts.in_battle ~= false),
        region_id = opts.region_id or 0,
        slots = {},
    }
    for slot = 0, 7 do
        st.slots[slot] = {
            slot = slot, role = (slot < 3) and "party" or "enemy",
            present = false, hp = 0, hp_max = 0, mp = 0, mp_max = 0,
            action = 0, status = 0, x = 0, z = 0, size = 0, monster_id = 0,
        }
    end
    for _, s in ipairs(opts.slots or {}) do
        local t = st.slots[s.slot]
        for k, v in pairs(s) do t[k] = v end
    end
    return st
end

-- A battle: Vahn (slot0, hp 128, acting) + Gobu Gobu (slot3, id 4, hp 76).
local battle = make_state({
    active_slot = 0,
    slots = {
        { slot = 0, present = true, hp = 128, hp_max = 128, mp = 27, action = 15 },
        { slot = 3, present = true, hp = 76,  hp_max = 76,  mp = 15, action = 9,
          monster_id = 4, status = 0x2000 },
    },
})

print("== pack() ==")
local p = enc.pack({ ch = 3, cc = 0x04, val = 100 })
check(#p == 3, "packed message is 3 bytes")
check(p:byte(1) == 0xB0 + 3, "status byte = 0xB0|channel")
check(p:byte(2) == 0x04 and p:byte(3) == 100, "cc + value bytes")

print("== full() sweep ==")
local e = enc.new()
e:tick()
local msgs = e:full(battle)
local idx = index(msgs)
-- meta
check(get(idx, REG.META_CHANNEL, REG.META.schema_version.cc) == 1, "meta schema_version = 1")
check(get(idx, REG.META_CHANNEL, REG.META.game_mode.cc) == 0x15, "meta game_mode = 0x15")
check(get(idx, REG.META_CHANNEL, REG.META.battle_phase.cc) == REG.BATTLE_PHASE.active, "meta phase = active")
check(get(idx, REG.META_CHANNEL, REG.META.heartbeat.cc) == 1, "meta heartbeat = 1 (after one tick)")
check(get(idx, REG.META_CHANNEL, REG.META.refresh_marker.cc) == 1, "refresh_marker bumped to 1 on full")
-- enemy slot 3 wide HP 76 -> hi 0, lo 76; id 4 -> hi 0 lo 4
check(get(idx, 3, REG.SLOT.hp.hi) == 0 and get(idx, 3, REG.SLOT.hp.lo) == 76, "enemy hp 76 -> (0,76)")
check(get(idx, 3, REG.SLOT.id.hi) == 0 and get(idx, 3, REG.SLOT.id.lo) == 4, "enemy id 4 -> (0,4)")
check(get(idx, 3, REG.SLOT.action_id.cc) == 9, "enemy action 9")
-- party slot0 HP 128 -> hi 1, lo 0  (exercises MSB)
check(get(idx, 0, REG.SLOT.hp.hi) == 1 and get(idx, 0, REG.SLOT.hp.lo) == 0, "party hp 128 -> (1,0) MSB")
-- flags: slot0 present(0)+acting(2) = 0b101 = 5 ; slot3 present only = 1
check(get(idx, 0, REG.SLOT.flags.cc) == 5, "slot0 flags present+acting = 5")
check(get(idx, 3, REG.SLOT.flags.cc) == 1, "slot3 flags present = 1")
-- status: confuse bit set on slot3
check(get(idx, 3, REG.SLOT.status.cc) == (2 ^ REG.STATUS_BITS.confuse), "slot3 status confuse bit")
-- absent slot (e.g. 4) present=false -> flags 0
check(get(idx, 4, REG.SLOT.flags.cc) == 0, "absent slot4 flags = 0")

print("== commit + MSB-before-LSB ordering ==")
-- Verify within enemy ch3: hi cc index appears before lo, commit last.
local function positions(msgs, ch)
    local pos = {}
    for i, m in ipairs(msgs) do if m.ch == ch then pos[#pos + 1] = m.cc end end
    return pos
end
local ch3 = positions(msgs, 3)
local last_cc = ch3[#ch3]
check(last_cc == 0x7F, "channel 3 ends with commit (0x7F)")
-- find index of hp.hi and hp.lo
local function first_at(seq, cc) for i, c in ipairs(seq) do if c == cc then return i end end end
check(first_at(ch3, REG.SLOT.hp.hi) < first_at(ch3, REG.SLOT.hp.lo), "hp hi emitted before lo")

print("== delta(): no change emits nothing ==")
local d0 = e:delta(battle)
check(#d0 == 0, "delta with no change = 0 messages")

print("== delta(): HP drop ==")
local battle2 = make_state({
    active_slot = 0,
    slots = {
        { slot = 0, present = true, hp = 128, hp_max = 128, mp = 27, action = 15 },
        { slot = 3, present = true, hp = 50,  hp_max = 76,  mp = 15, action = 9,
          monster_id = 4, status = 0x2000 },
    },
})
local d1 = e:delta(battle2)
local di = index(d1)
check(get(di, 3, REG.SLOT.hp.lo) == 50, "delta emits new hp lo = 50")
-- only channel 3 should appear (+ its commit); meta heartbeat unchanged (no tick)
local touched = {}
for _, m in ipairs(d1) do touched[m.ch] = true end
check(touched[3] and not touched[0] and not touched[REG.META_CHANNEL],
      "delta touches only the changed enemy channel")
check(di[3 * 256 + 0x7F] == 0, "delta includes commit on channel 3")

print("== field mode (not in battle) ==")
local field = make_state({ render_mode = 0x03, in_battle = false })
local e2 = enc.new()
local fmsgs = e2:full(field)
local fi = index(fmsgs)
check(get(fi, REG.META_CHANNEL, REG.META.battle_phase.cc) == REG.BATTLE_PHASE.none, "phase = none in field")
check(get(fi, 0, REG.SLOT.flags.cc) == 0 and get(fi, 3, REG.SLOT.flags.cc) == 0, "all slots flags 0 in field")

print("== budget sanity (PRD: full sweep ~ one frame of a virtual port) ==")
check(#msgs < 150, string.format("full sweep = %d messages (< 150)", #msgs))

print(fails == 0 and "ALL PASS" or (fails .. " FAILURES"))
os.exit(fails == 0 and 0 or 1)
