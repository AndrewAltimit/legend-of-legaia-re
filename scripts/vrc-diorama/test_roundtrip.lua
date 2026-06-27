-- test_roundtrip.lua  -- prove the wire protocol round-trips losslessly.
--
-- Encodes synthetic BattleStates with midi_encoder, then decodes the resulting
-- CC stream with a Lua MIRROR of the UdonSharp BattleStateDecoder (same
-- register table, same commit-latch + MSB/LSB-wide reconstruction), and asserts
-- the decoded state matches what was encoded. The UdonSharp decoder in unity/
-- is a faithful transcription of this same algorithm, so this validates the
-- protocol logic the C# side can't be unit-tested for here.
--
-- Run:  luajit scripts/vrc-diorama/test_roundtrip.lua

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

-- ---- decoder mirror (matches unity/BattleStateDecoder.cs) ----
local function new_decoder() return { pending = {}, latched = {} } end
local function feed(d, msgs)
    for _, m in ipairs(msgs) do
        if m.cc == 0x7F then
            for cc = 0, 127 do
                d.latched[m.ch * 128 + cc] = d.pending[m.ch * 128 + cc] or 0
            end
        else
            d.pending[m.ch * 128 + m.cc] = m.val
        end
    end
end
local function lat(d, ch, cc) return d.latched[ch * 128 + cc] or 0 end
local function wide(d, ch, reg) return lat(d, ch, reg.hi) * 128 + lat(d, ch, reg.lo) end
local function flagset(flags, bitpos)
    local b = 2 ^ bitpos
    return flags % (2 * b) >= b
end

-- ---- synthetic BattleState (shape of probe.battle_state.read()) ----
local function make_state(opts)
    opts = opts or {}
    local st = {
        render_mode = opts.render_mode or 0x15, game_state = 0, ctx = 0x80120000,
        active_slot = opts.active_slot or 0, party_action_id = 0xFF,
        monster_action_id = 0, turn = 1,
        in_battle = (opts.in_battle ~= false), region_id = opts.region_id or 0,
        slots = {},
    }
    for slot = 0, 7 do
        st.slots[slot] = { slot = slot, role = (slot < 3) and "party" or "enemy",
            present = false, hp = 0, hp_max = 0, mp = 0, mp_max = 0,
            action = 0, status = 0, x = 0, z = 0, size = 0, monster_id = 0 }
    end
    for _, s in ipairs(opts.slots or {}) do
        for k, v in pairs(s) do st.slots[s.slot][k] = v end
    end
    return st
end

local battle = make_state({
    active_slot = 0, region_id = 300,
    slots = {
        { slot = 0, present = true, hp = 128, hp_max = 128, action = 15 },
        { slot = 3, present = true, hp = 76, hp_max = 76, action = 9,
          monster_id = 4, status = 0x2000 },
        { slot = 4, present = true, hp = 9000, hp_max = 9999, action = 0,
          monster_id = 63 },  -- big HP to exercise the 14-bit wide path
    },
})

print("== full sweep round-trips ==")
local e = enc.new(); e:tick()
local d = new_decoder()
feed(d, e:full(battle))

check(lat(d, REG.META_CHANNEL, REG.META.schema_version.cc) == REG.SCHEMA_VERSION, "meta schema_version")
check(lat(d, REG.META_CHANNEL, REG.META.game_mode.cc) == 0x15, "meta game_mode 0x15")
check(lat(d, REG.META_CHANNEL, REG.META.battle_phase.cc) == REG.BATTLE_PHASE.active, "meta phase active")
check(wide(d, REG.META_CHANNEL, REG.META.region_id) == 300, "meta region_id 300 (wide)")
check(lat(d, REG.META_CHANNEL, REG.META.heartbeat.cc) == 1, "meta heartbeat 1")

local function slot_present(dd, ch)
    return flagset(lat(dd, ch, REG.SLOT.flags.cc), REG.FLAG_BITS.present)
end
local function slot_acting(dd, ch)
    return flagset(lat(dd, ch, REG.SLOT.flags.cc), REG.FLAG_BITS.acting)
end

check(slot_present(d, 0) and wide(d, 0, REG.SLOT.hp) == 128, "P0 present, hp 128 round-trip")
check(slot_acting(d, 0), "P0 acting flag")
check(wide(d, 0, REG.SLOT.maxhp) == 128, "P0 maxhp 128")
check(lat(d, 0, REG.SLOT.action_id.cc) == 15, "P0 action 15")
check(slot_present(d, 3) and wide(d, 3, REG.SLOT.id) == 4, "E3 present, id 4")
check(wide(d, 3, REG.SLOT.hp) == 76, "E3 hp 76")
check(flagset(lat(d, 3, REG.SLOT.status.cc), REG.STATUS_BITS.confuse), "E3 status confuse")
check(wide(d, 4, REG.SLOT.hp) == 9000 and wide(d, 4, REG.SLOT.maxhp) == 9999, "E4 14-bit hp 9000/9999 round-trip")
check(wide(d, 4, REG.SLOT.id) == 63, "E4 id 63")
check(not slot_present(d, 5), "slot5 absent")

print("== delta updates only what changed ==")
local battle2 = make_state({
    active_slot = 0, region_id = 300,
    slots = {
        { slot = 0, present = true, hp = 128, hp_max = 128, action = 15 },
        { slot = 3, present = true, hp = 50, hp_max = 76, action = 9,
          monster_id = 4, status = 0x2000 },
        { slot = 4, present = true, hp = 9000, hp_max = 9999, action = 0,
          monster_id = 63 },
    },
})
local dmsgs = e:delta(battle2)
feed(d, dmsgs)
check(wide(d, 3, REG.SLOT.hp) == 50, "E3 hp now 50 after delta")
check(wide(d, 0, REG.SLOT.hp) == 128, "P0 hp unchanged after delta")
check(wide(d, 4, REG.SLOT.hp) == 9000, "E4 hp unchanged after delta")
-- delta should not touch meta or unchanged slots
local touched = {}
for _, m in ipairs(dmsgs) do touched[m.ch] = true end
check(touched[3] and not touched[0] and not touched[4] and not touched[REG.META_CHANNEL],
      "delta touched only channel 3")

print("== field mode decodes to empty battle ==")
local e2 = enc.new()
local d2 = new_decoder()
feed(d2, e2:full(make_state({ render_mode = 0x03, in_battle = false })))
check(lat(d2, REG.META_CHANNEL, REG.META.battle_phase.cc) == REG.BATTLE_PHASE.none, "phase none in field")
local any = false
for slot = 0, 7 do if slot_present(d2, slot) then any = true end end
check(not any, "no slots present in field mode")

print("== fresh decoder reconstructs from a periodic full sweep alone ==")
-- A late joiner sees only the next full sweep; it must fully reconstruct.
local d3 = new_decoder()
feed(d3, e:full(battle2))   -- e's last-sent != d3, but full() emits everything
check(slot_present(d3, 3) and wide(d3, 3, REG.SLOT.hp) == 50, "late-joiner E3 hp 50 from full sweep")
check(wide(d3, 4, REG.SLOT.maxhp) == 9999, "late-joiner E4 maxhp from full sweep")

print(fails == 0 and "ALL PASS" or (fails .. " FAILURES"))
os.exit(fails == 0 and 0 or 1)
