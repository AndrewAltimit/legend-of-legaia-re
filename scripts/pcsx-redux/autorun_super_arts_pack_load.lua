-- autorun_super_arts_pack_load.lua
--
-- Runtime check for the **Super Arts Pack** disc patch (by ZetaPhoenix):
-- does the injected battle-load stub actually stream his 3764-byte block to
-- `0x801FD000` when a real battle starts?
--
-- Everything else about the feature is provable off the bytes (the disc oracle
-- `crates/patcher/tests/super_arts_pack_real.rs`) or in the in-crate
-- interpreter (the disc-gated unit tests that run the *patched* retail applier
-- over the block). The one link neither of those can reach is the CD read
-- itself: `FUN_8005E4D4(2, <annex LBA>, 0x801FD000)` on real hardware timing,
-- called from battle init. That is what this probe watches.
--
-- Recipe (why it is shaped this way):
--   * Run the emulator on the **patched** disc, but resume the retail-captured
--     `karisto_sol_pre_encounter` state - an overworld field frame parked one
--     step from a random encounter, where holding RIGHT for about a second
--     rolls a battle. A save state carries the RAM of the disc that booted it,
--     so the resident SCUS is the unpatched one...
--   * ...which is what LEGAIA_POKES is for: the `legaia-patcher scus-pokes`
--     output (`0xADDR:0xWORD` lines) is applied after the state load, making
--     the resident SCUS byte-match the disc under test. The battle then loads
--     through the patched code path, and the CD read comes off the patched
--     disc's own annex sectors.
--   * RIGHT is then held until the encounter rolls. Two exec breakpoints watch
--     the hook site and the stub's return; at the return the probe reads
--     `0x801FD000` and compares it against the block's expected head bytes and
--     its first name string.
--
-- Env vars:
--   LEGAIA_POKES     path to the scus-pokes output (required)
--   LEGAIA_SSTATE    save state (run_probe.sh --scenario sets this)
--   LEGAIA_OUT_DIR   output dir for the report
--   LEGAIA_WALK_FROM   first capture vsync to start holding RIGHT (default 8)
--
-- Output: <OUT_DIR>/super_arts_pack_load.txt  (PASS/FAIL + the observed bytes)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local pad   = require("probe.pad")

local BLOCK_VA   = 0x801FD000
local NAME_VA    = 0x801FD400          -- first 16-byte name: "Ultra Elbow"
local HOOK_VA    = 0x80055DBC          -- battle init: `j STUB_VA`
local STUB_VA    = 0x8007AE00          -- the injected loader stub
local RET_VA     = 0x80055DC4          -- where the stub returns
-- The block's first row is Vahn's Tri-Somersault find pattern, which is also
-- the retail row - so a match proves the sectors landed, not merely that
-- something was written.
local HEAD       = { 0x08, 0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27 }
local WANT_NAME  = "Ultra Elbow"

local WALK_FROM  = probe.getenv_num("LEGAIA_WALK_FROM", 8)
local POKES_PATH = probe.getenv("LEGAIA_POKES", "")

local report = {}
local function say(fmt, ...)
    local line = select("#", ...) > 0 and string.format(fmt, ...) or fmt
    report[#report + 1] = line
    PCSX.log("[pack] " .. line)
end

-- Read the `0xADDR:0xWORD` poke list emitted by `legaia-patcher scus-pokes`.
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

local function block_head_matches()
    for i, want in ipairs(HEAD) do
        if mem.read_u8(BLOCK_VA + i - 1) ~= want then return false end
    end
    return true
end

local function block_name()
    local s = {}
    for i = 0, 15 do
        local b = mem.read_u8(NAME_VA + i)
        if b == 0 then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

-- The driver's default summary reads `d.hits_ref.n`, so keep the counters in
-- the shape it expects.
local hits_hook = { n = 0 }
local hits_ret  = { n = 0 }
local verdict = nil

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3000),
    boot_delay     = 90,
    quit_delay     = 10,

    on_arm = function(ctx)
        bp.arm(HOOK_VA, "Exec", 4, "pack_hook", function()
            hits_hook.n = hits_hook.n + 1
            if hits_hook.n == 1 then say("battle-init hook reached (%08X)", HOOK_VA) end
        end)
        bp.arm(RET_VA, "Exec", 4, "pack_ret", function()
            hits_ret.n = hits_ret.n + 1
            if verdict ~= nil then return end
            local ok_head = block_head_matches()
            local name = block_name()
            say("stub returned; block head %s, name at %08X = %q",
                ok_head and "MATCHES" or "DIFFERS", NAME_VA, name)
            verdict = ok_head and (name == WANT_NAME)
            ctx.request_quit = true
        end)
        return {
            { addr = HOOK_VA, name = "battle-init hook", hits_ref = hits_hook },
            { addr = RET_VA, name = "stub return", hits_ref = hits_ret },
        }
    end,

    on_capture = function(ctx, v)
        if v == 2 then
            -- Make the resident SCUS match the disc under test.
            local pokes = POKES_PATH ~= "" and read_pokes(POKES_PATH) or nil
            if not pokes or #pokes == 0 then
                say("FAIL: no pokes read from %q - resident SCUS is unpatched", POKES_PATH)
                ctx.request_quit = true
                return
            end
            for _, p in ipairs(pokes) do mem.write_u32(p.addr, p.word) end
            say("applied %d SCUS pokes; hook word now %08X, stub word %08X",
                #pokes, mem.read_u32(HOOK_VA), mem.read_u32(STUB_VA))
            -- Baseline: the block's landing zone must start empty, so a later
            -- match cannot be something that was already resident.
            local pre = block_head_matches()
            say("baseline at %08X: %s", BLOCK_VA,
                pre and "ALREADY MATCHES (probe is not discriminating!)" or "clear")
            if pre then verdict = false; ctx.request_quit = true end
        elseif v == WALK_FROM then
            -- One step of walking rolls the encounter (see the state's note).
            pad.force(pad.BTN.RIGHT)
            say("holding RIGHT to roll an encounter")
        elseif v > WALK_FROM and v % 240 == 0 then
            -- Nudge the direction periodically in case of a wall.
            pad.release(pad.BTN.RIGHT)
            pad.force(pad.BTN.DOWN)
        elseif v > WALK_FROM and v % 240 == 30 then
            pad.release(pad.BTN.DOWN)
            pad.force(pad.BTN.RIGHT)
        end
    end,

    on_done = function(ctx)
        pad.release(pad.BTN.RIGHT)
        pad.release(pad.BTN.DOWN)
        say("hook hits=%d, stub-return hits=%d", hits_hook.n, hits_ret.n)
        if verdict == true then
            say("PASS: ZetaPhoenix's block is resident at %08X after the battle load", BLOCK_VA)
        elseif verdict == false then
            say("FAIL: the block did not land as expected")
        else
            say("INCONCLUSIVE: no battle load observed in the capture window")
        end
        local path = probe.out_path("super_arts_pack_load.txt")
        local f = io.open(path, "w")
        if f then
            f:write(table.concat(report, "\n"), "\n")
            f:close()
            PCSX.log("[pack] wrote " .. path)
        end
    end,
})
