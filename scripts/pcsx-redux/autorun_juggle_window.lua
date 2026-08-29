-- autorun_juggle_window.lua
--
-- Pin the juggle window live: who writes battle-actor +0x1F7 (the byte the
-- damage kernel FUN_801EC3E4 tests to grow the juggle counter ctx+0x0A) and
-- what value it carries against the defender's current clip.
--
-- Arms a 1-byte Write watch on +0x1F7 of every populated slot of the
-- battle-action actor-pointer array (&DAT_801c9370)[0..11] and logs every
-- TRANSITION (0->1 / 1->0) with the writer pc plus the anim tick's live
-- registers (FUN_80047430: s5 = render node, s3 = committed action record,
-- s2 = actor): the 12.4 cursor node+0x68, the record's first event frame
-- record+0x10, its rate byte +0x78, the actor's speed scale +0x21D, the
-- committed clip id +0x1D9 and the cached light-flinch entry +0x1EF.
-- Also an Exec breakpoint on the juggle store `sb v0,0xa(v1)` at 0x801ECA80
-- logging the counter value + defender slot, so each hit's juggle can be
-- read against the defender's +0x1F7 timeline.
--
-- No input: use an auto-playing battle state (battle_noa_miracle_art_combo
-- for a party combo on a monster; rim_elm_queen_bee_battle for a monster
-- hitting the party). LEGAIA_FRAMES bounds the capture.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local PRESS_X = probe.getenv("LEGAIA_PRESS_X", "") ~= ""

local ACTOR_ARRAY = 0x801C9370
local JUGGLE_STORE = 0x801ECA80
local CTX_PTR = 0x8007BD24

local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end

local armed = false
local last = {}
local frame = 0

local function arm_actor(slot, actor)
    local field = actor + 0x1F7
    last[slot] = u8(field)
    PCSX.log(string.format("  slot %2d actor 0x%08X hp=%d +1F7=%d flinch(+1EF)=%d",
        slot, actor, u16(actor + 0x14C), last[slot], u8(actor + 0x1EF)))
    probe.arm_breakpoint(field, "Write", 1, "j1f7_" .. slot, function()
        local r = PCSX.getRegisters()
        local pc = tou32(r.pc)
        -- The two retail writers: 0x80047E50 stores zero, 0x80047E54 stores 1.
        local val
        if pc == 0x80047E50 then val = 0 elseif pc == 0x80047E54 then val = 1 else val = -1 end
        if val == last[slot] then return end
        last[slot] = val
        local node = tou32(r.GPR.n.s5)
        local rec = tou32(r.GPR.n.s3)
        local cursor = u16(node + 0x68)
        PCSX.log(string.format(
            "[1F7] t=%d slot=%d pc=0x%08X val=%d cursor=%d.%02d/16 beat(+10)=%d list=%02X %02X %02X %02X rate(+78)=%d tag=%d spd(+21D)=%d cur(+1D9)=%d flinch(+1EF)=%d hp=%d",
            frame, slot, pc, val, math.floor(cursor / 16), cursor % 16,
            u8(rec + 0x10), u8(rec + 0x10), u8(rec + 0x11), u8(rec + 0x12), u8(rec + 0x13),
            u8(rec + 0x78), u8(rec), u8(actor + 0x21D), u8(actor + 0x1D9),
            u8(actor + 0x1EF), u16(actor + 0x14C)))
    end)
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        frame = elapsed
        if elapsed == 2 and not armed then
            armed = true
            PCSX.log(string.format("== juggle window probe == gmode=0x%02X ctx=0x%08X",
                u8(0x8007B83C), u32(CTX_PTR)))
            for slot = 0, 11 do
                local p = u32(ACTOR_ARRAY + slot * 4)
                if ok_ptr(p) and u16(p + 0x14C) > 0 then arm_actor(slot, p) end
            end
            probe.arm_breakpoint(JUGGLE_STORE, "Exec", 4, "juggle_store", function()
                local r = PCSX.getRegisters()
                local j = tou32(r.GPR.n.v0)
                local def = tou32(r.GPR.n.s4) % 256
                local a = u32(ACTOR_ARRAY + def * 4)
                PCSX.log(string.format(
                    "[JUGGLE] t=%d defender_slot=%d juggle=%d def+1F7=%d def+1D9=%d hp=%d",
                    frame, def, j, u8(a + 0x1F7), u8(a + 0x1D9), u16(a + 0x14C)))
            end)
        end
        -- Optional: a state parked on a confirm prompt (party_basic_attack_vs_gobu_gobu)
        -- needs one X press to run the turn.
        if PRESS_X then
            if elapsed >= 30 and elapsed < 34 then probe.pad_force(probe.BTN.CROSS)
            elseif elapsed == 34 then probe.pad_release(probe.BTN.CROSS) end
        end
        if elapsed >= FRAMES - 5 then c.request_quit = true end
    end,
})
