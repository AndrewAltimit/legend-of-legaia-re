-- autorun_magic_wait_producers.lua
--
-- Groundwork for the "Gaza 2 magic softlock" hunt: during a HEALTHY magic /
-- summon cast, identify the PRODUCERS of every done-signal the battle-action
-- SM's magic band (FUN_801E295C states 0x28..0x2E) waits on:
--
--   ctx+0x249 / ctx+0x24C / ctx+0x24D   effect/damage completion counters
--   actor+0x1FA                          spell-cast iteration counter
--   actor+0x1D9 / +0x1DA                 current / queued anim id
--   actor+0x21B                          hit-counter script bound
--
-- Arms Write watchpoints on all of them (writer pc + ra land in the CSV) plus
-- ctx+7 (the SM state cursor itself - every state transition with its writer),
-- and polls a per-vsync timeline of the cursor, all six signals, and the
-- battle camera yaw _DAT_8007B792 (the endless-orbit symptom variable).
--
-- The output is (a) the writer-function addresses for each signal - the dumps
-- we're missing - and (b) a healthy-cast reference timeline to diff a future
-- captured softlock against.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_magic_wait_producers.lua \
--     --scenario gimard_summon_start --frames 2400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local watch = require("probe.watch")
local pad   = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
-- Optional: rewrite the slot-3 enemy record's magic-attack byte (+0x21) so
-- its next magic pick becomes this global id (0x5C = Bloody Horns). Same
-- experiment knob as autorun_element_attribution_trace.lua.
local POKE_MAGIC_ID = probe.getenv_num("LEGAIA_POKE_MAGIC_ID", 0)
-- Tap Cross every N vsyncs to drive parked battle menus (0 = off).
local TAP_X = probe.getenv_num("LEGAIA_TAP_X", 0)

if probe.getenv("LEGAIA_CORE", "") == "dynarec" then
    PCSX.log("[wait-producers] REFUSING --fast launch: Lua breakpoints never fire under the recompiler")
    PCSX.quit(3)
    return
end

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local CAM_YAW = 0x8007B792

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80800000 end

local g_elapsed = 0
local hits_csv = probe.csv_open(probe.out_path("signal_writes.csv"),
    "tick,label,addr,pc,ra,value")
local w = watch.new{
    csv         = hits_csv,
    detail_path = probe.out_path("signal_writes.detail.txt"),
    max_detail  = 24,
    elapsed     = function() return g_elapsed end,
}
local timeline = probe.csv_open(probe.out_path("timeline.csv"),
    "vsync,ctx7,seat,move_id,c249,c24c,c24d,a1fa,a1d9,a1da,a21b,cam_yaw")

local armed = false
local poked = false
local tapping = 0
local watched_actor = 0
local last_row = ""

local function arm_watches()
    local c = u32(CTX_PTR)
    if not in_ram(c) then return false end
    local seat = u8(c + 0x13)
    local actor = u32(ACTORS + seat * 4)
    w:arm(c + 7,     1, "ctx7_cursor")
    w:arm(c + 0x249, 1, "ctx_249")
    w:arm(c + 0x24C, 1, "ctx_24c")
    w:arm(c + 0x24D, 1, "ctx_24d")
    if in_ram(actor) then
        watched_actor = actor
        w:arm(actor + 0x1FA, 1, "actor_1fa")
        w:arm(actor + 0x1D9, 1, "actor_1d9")
        w:arm(actor + 0x1DA, 1, "actor_1da")
        w:arm(actor + 0x21B, 1, "actor_21b")
    end
    -- Damage-roll entry: log the attacker-slot argument + the resolved
    -- attacker element, so a poked enemy magic cast answers the
    -- attribution question in the same capture.
    probe.arm_breakpoint(0x801DD0AC, "Exec", 4, "dmg_roll", function()
        local r = PCSX.getRegisters()
        local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100
        local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100
        local a2 = (tonumber(r.GPR.n.a2) or 0) % 0x100
        local elem, rec = 0xFF, 0
        if a1 < 3 then
            elem = u8(0x801F547F + u8(0x8007BD10 + a1))
        else
            rec = u32(0x801C9348 + (a1 - 3) * 4)
            elem = u8(rec + 0x1D)
        end
        local cc = u32(CTX_PTR)
        PCSX.log(string.format(
            "[wait-producers] dmg_roll vsync=%d a0=0x%02X atk_slot=%d def_slot=%d atk_elem=%d atk_rec=0x%08X seat=%d ctx7=0x%02X",
            g_elapsed, a0, a1, a2, elem, rec,
            in_ram(cc) and u8(cc + 0x13) or 255,
            in_ram(cc) and u8(cc + 7) or 255))
    end)
    PCSX.log(string.format(
        "[wait-producers] armed: ctx=0x%08X seat=%d actor=0x%08X", c, seat, actor))
    return true
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = function() return { "deferred" } end,
    on_capture     = function(_, v)
        g_elapsed = v
        -- Arm on the first post-load vsync: ctx / actor pointers are only
        -- valid once the save state is in.
        if not armed and v >= 2 then
            armed = arm_watches()
        end
        -- Poke only once the battle ctx looks initialized (a state captured
        -- pre-init memcpys the whole ctx at ~vsync 235, erasing any earlier
        -- poke): sane acting seat + an early SM state.
        if POKE_MAGIC_ID ~= 0 and not poked then
            local c2 = u32(CTX_PTR)
            if in_ram(c2) and u8(c2 + 0x13) < 8 and u8(c2 + 7) <= 0x14 then
                local rec = u32(0x801C9348)
                if in_ram(rec) then
                    local old = u8(rec + 0x21)
                    probe.write_u8(rec + 0x21, POKE_MAGIC_ID)
                    PCSX.log(string.format(
                        "[wait-producers] POKED slot-3 record 0x%08X +0x21: 0x%02X -> 0x%02X (vsync %d)",
                        rec, old, POKE_MAGIC_ID, v))
                    poked = true
                end
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
        local c = u32(CTX_PTR)
        if not in_ram(c) then return end
        local seat = u8(c + 0x13)
        local actor = watched_actor ~= 0 and watched_actor or u32(ACTORS + seat * 4)
        local move_id = in_ram(actor) and u8(actor + 0x1DF) or 0xFF
        local row = string.format("0x%02X,%d,0x%02X,%d,%d,%d,%d,%d,%d,%d",
            u8(c + 7), seat, move_id,
            u8(c + 0x249), u8(c + 0x24C), u8(c + 0x24D),
            in_ram(actor) and u8(actor + 0x1FA) or 255,
            in_ram(actor) and u8(actor + 0x1D9) or 255,
            in_ram(actor) and u8(actor + 0x1DA) or 255,
            in_ram(actor) and u8(actor + 0x21B) or 255)
        -- Only write timeline rows when something changed (plus the yaw,
        -- which changes every frame and is appended after the dedup key).
        if row ~= last_row then
            last_row = row
            timeline:row("%d,%s,%d", v, row, u16(CAM_YAW))
        end
    end,
    on_summary     = function()
        PCSX.log(string.format("[wait-producers] total signal writes: %d", w:total()))
        hits_csv:close()
        timeline:close()
    end,
}
