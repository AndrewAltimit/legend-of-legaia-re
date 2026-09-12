-- autorun_oscillating_ap_roll.lua
--
-- Runtime check for the **oscillating AP costs** disc patch: does the
-- injected per-battle roll (SLOT6, detoured from the battle loader's setup
-- site 0x80051A20) actually run when a real battle starts, and does it leave
-- the 16-byte side table filled?
--
-- Recipe: run the emulator on the PATCHED disc, resume the retail-captured
-- `karisto_sol_pre_encounter` state (one step from a random encounter), apply
-- the `legaia-patcher scus-pokes` list so the resident SCUS byte-matches the
-- disc, hold RIGHT until the encounter rolls. Exec breakpoints watch the roll
-- entry and the setup site's resume word; at the resume the probe reads the
-- side table + its counter. Later hits on the guard / debit / damage routines
-- (which need the player to commit an art) are counted if they happen.
--
-- Env: LEGAIA_POKES (required), LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_WALK_FROM.
-- Output: <OUT_DIR>/oscillating_ap_roll.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local pad   = require("probe.pad")

local HOOK_VA   = 0x80051A20
local RESUME_VA = 0x80051A28
local ROLL_VA   = 0x80078A88
local GUARD_VA  = 0x8007AE40
local DEBIT_VA  = 0x8007AE6C
local DMG_VA    = 0x80077728
local BITS_VA   = 0x800777C4
local CNT_VA    = 0x800777D4

local WALK_FROM  = probe.getenv_num("LEGAIA_WALK_FROM", 8)
local POKES_PATH = probe.getenv("LEGAIA_POKES", "")

local report = {}
local function say(fmt, ...)
    local line = select("#", ...) > 0 and string.format(fmt, ...) or fmt
    report[#report + 1] = line
    PCSX.log("[osc] " .. line)
end

local function read_pokes(path)
    local out = {}
    local f = io.open(path, "r")
    if not f then return nil end
    for line in f:lines() do
        local a, w = line:match("^%s*0x(%x+)%s*:%s*0x(%x+)")
        if a then out[#out + 1] = { addr = tonumber(a, 16), word = tonumber(w, 16) } end
    end
    f:close()
    return out
end

local function bits_hex()
    return mem.bytes_to_hex(mem.read_bytes(BITS_VA, 16))
end

local hits = { roll = { n = 0 }, resume = { n = 0 }, guard = { n = 0 }, debit = { n = 0 }, dmg = { n = 0 } }
local verdict = nil
local resume_frame = nil
local last_v = 0

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3000),
    boot_delay     = 90,
    quit_delay     = 10,

    on_arm = function(ctx)
        bp.arm(ROLL_VA, "Exec", 4, "osc_roll", function()
            hits.roll.n = hits.roll.n + 1
            if hits.roll.n == 1 then say("roll entered at %08X; table before = %s", ROLL_VA, bits_hex()) end
        end)
        bp.arm(RESUME_VA, "Exec", 4, "osc_resume", function()
            hits.resume.n = hits.resume.n + 1
            if hits.resume.n == 1 then
                say("setup resumed at %08X; counter = %d; table after = %s",
                    RESUME_VA, mem.read_u32(CNT_VA), bits_hex())
                local nonzero = 0
                for i = 0, 15 do if mem.read_u8(BITS_VA + i) ~= 0 then nonzero = nonzero + 1 end end
                verdict = (hits.roll.n >= 1) and (mem.read_u32(CNT_VA) == 16) and (nonzero >= 8)
                resume_frame = last_v
            end
        end)
        bp.arm(GUARD_VA, "Exec", 4, "osc_guard", function() hits.guard.n = hits.guard.n + 1 end)
        bp.arm(DEBIT_VA, "Exec", 4, "osc_debit", function() hits.debit.n = hits.debit.n + 1 end)
        bp.arm(DMG_VA, "Exec", 4, "osc_dmg", function() hits.dmg.n = hits.dmg.n + 1 end)
        return {
            { addr = ROLL_VA, name = "roll entry", hits_ref = hits.roll },
            { addr = RESUME_VA, name = "setup resume", hits_ref = hits.resume },
            { addr = GUARD_VA, name = "guard", hits_ref = hits.guard },
            { addr = DEBIT_VA, name = "debit", hits_ref = hits.debit },
            { addr = DMG_VA, name = "damage", hits_ref = hits.dmg },
        }
    end,

    on_capture = function(ctx, v)
        last_v = v
        if v == 2 then
            local pokes = POKES_PATH ~= "" and read_pokes(POKES_PATH) or nil
            if not pokes or #pokes == 0 then
                say("FAIL: no pokes read from %q - resident SCUS is unpatched", POKES_PATH)
                ctx.request_quit = true
                return
            end
            for _, p in ipairs(pokes) do mem.write_u32(p.addr, p.word) end
            say("applied %d SCUS pokes; hook word now %08X, roll word %08X, table = %s",
                #pokes, mem.read_u32(HOOK_VA), mem.read_u32(ROLL_VA), bits_hex())
        elseif v == WALK_FROM then
            pad.force(pad.BTN.RIGHT)
            say("holding RIGHT to roll an encounter")
        elseif v > WALK_FROM and v % 240 == 0 then
            pad.release(pad.BTN.RIGHT)
            pad.force(pad.BTN.DOWN)
        elseif v > WALK_FROM and v % 240 == 30 then
            pad.release(pad.BTN.DOWN)
            pad.force(pad.BTN.RIGHT)
        end
        -- Once the roll has run, give the battle ~10 s to settle, then quit.
        if resume_frame and v > resume_frame + 600 then
            ctx.request_quit = true
        end
    end,

    on_done = function(ctx)
        pad.release(pad.BTN.RIGHT)
        pad.release(pad.BTN.DOWN)
        say("hits: roll=%d resume=%d guard=%d debit=%d damage=%d",
            hits.roll.n, hits.resume.n, hits.guard.n, hits.debit.n, hits.dmg.n)
        say("final table = %s counter = %d", bits_hex(), mem.read_u32(CNT_VA))
        if verdict == true then
            say("PASS: the roll ran at battle load and filled the side table")
        elseif verdict == false then
            say("FAIL: the roll ran but the table is not as expected")
        else
            say("INCONCLUSIVE: no battle load observed in the capture window")
        end
        local path = probe.out_path("oscillating_ap_roll.txt")
        local f = io.open(path, "w")
        if f then f:write(table.concat(report, "\n"), "\n"); f:close(); PCSX.log("[osc] wrote " .. path) end
    end,
})
