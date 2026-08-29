-- autorun_cast_hook_live.lua
--
-- Live test of the Delilas cast-route QUEUE HOOK: RAM-inject the SCUS-gap
-- stub + the 0898 applier-call redirect (both read from files, since a
-- savestate restores retail RAM) into the standard queued-attack battle
-- state, press X to Begin, and watch whether the turn proceeds. BPs log
-- entry into the stub and into the retail applier.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local STUB_FILE = probe.getenv("LEGAIA_STUB_BIN", "")
local STUB_VA = 0x80077728
local REDIRECT_VA = 0x801EF9AC
local REDIRECT_WORD = 0x0C01DDCA
local APPLIER_VA = 0x801EF9E4

local ACTOR_TABLE = 0x801C9370
local CTX_PTR = 0x8007BD24

local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ctx()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end

local injected = false
local stub_hits, applier_hits = 0, 0
local last = ""

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== cast hook live test ==")
        probe.arm_breakpoint(STUB_VA, "Exec", 4, "stub", function()
            stub_hits = stub_hits + 1
            if stub_hits <= 6 then
                local r = PCSX.getRegisters()
                local n = r.GPR and r.GPR.n or {}
                local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
                PCSX.log(string.format("[STUB %d] a0=%08X a1=%08X ra=%08X sp=%08X",
                    stub_hits, tou32(n.a0), tou32(n.a1), tou32(n.ra), tou32(n.sp)))
            end
        end)
        probe.arm_breakpoint(APPLIER_VA, "Exec", 4, "applier", function()
            applier_hits = applier_hits + 1
            if applier_hits <= 6 then
                local r = PCSX.getRegisters()
                local n = r.GPR and r.GPR.n or {}
                local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
                PCSX.log(string.format("[APPLIER %d] a0=%08X a1=%08X ra=%08X",
                    applier_hits, tou32(n.a0), tou32(n.a1), tou32(n.ra)))
            end
        end)
        return {}
    end,
    on_capture = function(c, elapsed)
        if not injected and elapsed >= 2 then
            injected = true
            local fh = io.open(STUB_FILE, "rb")
            if fh == nil then
                PCSX.log("[FATAL] cannot open stub file " .. STUB_FILE)
                return
            end
            local blob = fh:read("*a")
            fh:close()
            for i = 1, #blob do
                probe.write_u8(STUB_VA + i - 1, string.byte(blob, i))
            end
            probe.write_u8(REDIRECT_VA + 0, REDIRECT_WORD % 0x100)
            probe.write_u8(REDIRECT_VA + 1, math.floor(REDIRECT_WORD / 0x100) % 0x100)
            probe.write_u8(REDIRECT_VA + 2, math.floor(REDIRECT_WORD / 0x10000) % 0x100)
            probe.write_u8(REDIRECT_VA + 3, math.floor(REDIRECT_WORD / 0x1000000) % 0x100)
            PCSX.log(string.format("[inject t%d] stub %d bytes @%08X + redirect @%08X",
                elapsed, #blob, STUB_VA, REDIRECT_VA))
        end

        probe.pad_release(probe.BTN.CROSS)
        if elapsed < 400 then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        local cx = ctx()
        if cx == nil then return end
        local a0 = u32(ACTOR_TABLE)
        local line = string.format("st7=0x%02X cat=%02X spirit=%02X",
            u8(cx + 7), a0 >= 0x80000000 and u8(a0 + 0x1DE) or 0xFF,
            a0 >= 0x80000000 and u8(a0 + 0x1DF) or 0xFF)
        if line ~= last then
            PCSX.log(string.format("[t%d] %s stub_hits=%d applier_hits=%d",
                elapsed, line, stub_hits, applier_hits))
            last = line
        end
    end,
})
