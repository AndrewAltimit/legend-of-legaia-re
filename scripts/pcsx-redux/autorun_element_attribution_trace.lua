-- autorun_element_attribution_trace.lua
--
-- Live verification of the damage-pipeline ELEMENT ATTRIBUTION law:
-- element is resolved per attacker battle slot, never per spell.
--   slot < 3  -> party char table 0x801F547F[char_id]
--   slot >= 3 -> monster record ptr table 0x801C9348[slot-3] + 0x1D
--   slot == 7 -> 0x801C9358 + 0x1D (the streamed cast-body actor record
--                installed by FUN_801F19EC)
--
-- Arms Exec breakpoints at the three damage-chain kernels:
--   FUN_801DD0AC (roll)    - logs (power_idx, attacker, defender) args
--   FUN_801DD864 (scale)   - logs args + both resolved element bytes
--   FUN_801DDB30 (finisher)- logs args + attacker element the resist
--                            ladder / Earth-Jewel check would read
-- plus a Write watchpoint on 0x801C9358 (cast-body record install) and,
-- per hit, the installed body's element byte and its attack-name string.
--
-- Optional experiment knobs:
--   LEGAIA_POKE_MAGIC_ID=0x5C  rewrite the slot-3 enemy's monster-record
--       magic-attack byte (+0x21) after state load, so its next magic
--       cast becomes that global id (0x5C = Bloody Horns). Lets us watch
--       the Xain magic path in any battle without a Xain save state.
--   LEGAIA_TAP_X=N   tap Cross every N vsyncs (drive a parked battle
--       menu forward). 0 = off (default).
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_element_attribution_trace.lua \
--     --scenario gimard_summon_start --frames 2400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local TAP_X  = probe.getenv_num("LEGAIA_TAP_X", 0)
local POKE_MAGIC_ID = probe.getenv_num("LEGAIA_POKE_MAGIC_ID", 0)

if probe.getenv("LEGAIA_CORE", ""):match("^dynarec$") or probe.getenv("LEGAIA_CORE", ""):match("^interpreter%-nodebug$") then
    PCSX.log("[element-trace] REFUSING --fast/--timing launch: Lua breakpoints need the debugger hook")
    PCSX.quit(3)
    return
end

local FN_ROLL   = 0x801DD0AC
local FN_SCALE  = 0x801DD864
local FN_FINISH = 0x801DDB30
local CTX_PTR   = 0x8007BD24 -- -> battle ctx (0x800EB654)
local GMODE     = 0x8007B83C -- battle = 0x15
local ACTORS    = 0x801C9370 -- live actor ptr table, +slot*4
local RECORDS   = 0x801C9348 -- monster record ptr table, +(slot-3)*4
local BODY_SLOT = 0x801C9358 -- == RECORDS[4]: the slot-7 cast body record
local SEAT_CHAR = 0x8007BD10 -- per-seat char id bytes
local CHAR_ELEM = 0x801F547F -- + char_id (1-based) -> element

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function u8(a)  return probe.read_u8(a)  or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80800000 end
local function ctx() return u32(CTX_PTR) end

-- Resolve an attacker/defender element exactly the way FUN_801DD864 /
-- FUN_801DDB30 do. Returns element, record_ptr (record_ptr nil for party).
local function slot_element(slot)
    if slot < 3 then
        local charid = u8(SEAT_CHAR + slot)
        return u8(CHAR_ELEM + charid), nil
    end
    local rec = u32(RECORDS + (slot - 3) * 4)
    if not in_ram(rec) then return 0xFF, rec end
    return u8(rec + 0x1D), rec
end

-- The installed cast-body record's name string (FUN_801F19EC rewrites the
-- +0x00 name offset into an absolute pointer at install time).
local function body_name()
    local rec = u32(BODY_SLOT)
    if not in_ram(rec) then return "" end
    local np = u32(rec)
    if not in_ram(np) then return "" end
    local out = {}
    for i = 0, 23 do
        local b = u8(np + i)
        if b == 0 then break end
        if b >= 0x20 and b < 0x7F then out[#out + 1] = string.char(b) end
    end
    return table.concat(out)
end

local g_elapsed = 0
local csv = probe.csv_open(probe.out_path("element_attribution.csv"),
    "vsync,hook,a0,a1,a2,seat,seat_move_id,atk_slot,atk_elem,atk_rec,def_slot,def_elem,body_ptr,body_elem,body_name,ctx7")

local hits = { roll = 0, scale = 0, finish = 0, install = 0 }

local function log_hit(hook, atk, def, a0, a1, a2)
    local c = ctx()
    local seat, ctx7, move_id = 0xFF, 0xFF, 0xFF
    if in_ram(c) then
        seat = u8(c + 0x13)
        ctx7 = u8(c + 7)
        local actor = u32(ACTORS + seat * 4)
        if in_ram(actor) then move_id = u8(actor + 0x1DF) end
    end
    local atk_elem, atk_rec = slot_element(atk)
    local def_elem, _ = slot_element(def)
    local body = u32(BODY_SLOT)
    local body_elem = in_ram(body) and u8(body + 0x1D) or 0xFF
    csv:row("%d,%s,0x%02X,0x%02X,0x%02X,%d,0x%02X,%d,%d,0x%08X,%d,%d,0x%08X,%d,%s,0x%02X",
        g_elapsed, hook, a0, a1, a2, seat, move_id,
        atk, atk_elem, atk_rec or 0, def, def_elem,
        body, body_elem, body_name(), ctx7)
    PCSX.log(string.format(
        "[element-trace] %s vsync=%d atk_slot=%d atk_elem=%d def_slot=%d def_elem=%d move=0x%02X seat=%d body_elem=%d body='%s'",
        hook, g_elapsed, atk, atk_elem, def, def_elem, move_id, seat, body_elem, body_name()))
end

local function arm_all()
    probe.arm_breakpoint(FN_ROLL, "Exec", 4, "dmg_roll", function()
        local r = PCSX.getRegisters()
        local a0 = tou32(r.GPR.n.a0) % 0x100
        local a1 = tou32(r.GPR.n.a1) % 0x100
        local a2 = tou32(r.GPR.n.a2) % 0x100
        hits.roll = hits.roll + 1
        log_hit("roll", a1, a2, a0, a1, a2)
    end)
    probe.arm_breakpoint(FN_SCALE, "Exec", 4, "dmg_scale", function()
        local r = PCSX.getRegisters()
        local a0 = tou32(r.GPR.n.a0) % 0x100
        local a1 = tou32(r.GPR.n.a1) % 0x100
        hits.scale = hits.scale + 1
        log_hit("scale", a0, a1, a0, a1, 0)
    end)
    probe.arm_breakpoint(FN_FINISH, "Exec", 4, "dmg_finish", function()
        local r = PCSX.getRegisters()
        local a0 = tou32(r.GPR.n.a0) % 0x100
        local a1 = tou32(r.GPR.n.a1) % 0x100
        hits.finish = hits.finish + 1
        log_hit("finish", a0, a1, a0, a1, 0)
    end)
    probe.arm_breakpoint(BODY_SLOT, "Write", 4, "body_install", function()
        local r = PCSX.getRegisters()
        hits.install = hits.install + 1
        local pc = tou32(r.pc)
        csv:row("%d,install,0,0,0,255,0xFF,255,255,0x%08X,255,255,0x%08X,255,,0xFF",
            g_elapsed, pc, u32(BODY_SLOT))
        PCSX.log(string.format(
            "[element-trace] body-record INSTALL vsync=%d pc=0x%08X new_ptr=0x%08X",
            g_elapsed, pc, u32(BODY_SLOT)))
    end)
    return { "roll", "scale", "finish", "install" }
end

local poked = false
local tapping = 0

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = arm_all,
    on_capture     = function(_, v)
        g_elapsed = v
        -- One-shot record poke, deferred until the battle ctx looks
        -- initialized (a pre-init state memcpys the whole ctx and the
        -- records shortly after load, erasing an earlier poke).
        if POKE_MAGIC_ID ~= 0 and not poked and v >= 10 then
            local c2 = ctx()
            if not (in_ram(c2) and u8(c2 + 0x13) < 8 and u8(c2 + 7) <= 0x14) then return end
            local rec = u32(RECORDS) -- slot 3 enemy record
            if in_ram(rec) then
                local old = u8(rec + 0x21)
                probe.write_u8(rec + 0x21, POKE_MAGIC_ID)
                PCSX.log(string.format(
                    "[element-trace] POKED slot-3 record 0x%08X +0x21: 0x%02X -> 0x%02X",
                    rec, old, POKE_MAGIC_ID))
                poked = true
            end
        end
        if TAP_X > 0 then
            if tapping > 0 then
                tapping = tapping - 1
                if tapping == 0 then pad.release(pad.BTN.CROSS) end
            elseif v % TAP_X == 0 then
                pad.force(pad.BTN.CROSS)
                tapping = 3
            end
        end
    end,
    on_summary     = function()
        PCSX.log(string.format(
            "[element-trace] hits: roll=%d scale=%d finish=%d install=%d",
            hits.roll, hits.scale, hits.finish, hits.install))
        csv:close()
    end,
}
