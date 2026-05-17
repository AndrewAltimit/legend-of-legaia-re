-- autorun_slot4_dispatcher_args.lua
--
-- Capture the original arguments to FUN_80043390 (cluster A's TMD-style
-- dispatcher) at its prologue, before the kind handlers clobber a1/a2.
--
-- The previously-captured consumer-PC CSV records register state at LW
-- sites *inside* the kind handlers, where `a1` and `a2` have been re-
-- loaded with intermediate values (vertex-pool / command-stream
-- pointers). That CSV cannot answer "what cmd_flags / fade_flags did
-- the caller pass?" because both args are clobbered by the time those
-- PCs are reached.
--
-- This probe arms a single Exec breakpoint at the dispatcher entry
-- (0x80043390) and records:
--   ra        : caller return address (which feeder code path)
--   a0        : descriptor pointer (param_1 -- TMD-group array or
--               slot-4-aligned pointer or working-buffer mesh struct)
--   a1        : packed cmd_flags  -- param_2
--   a2        : fade_flags         -- param_3
--   kind      : (a0 -> cmd_word) >> 17  &  0x7FFF  -- the first kind
--               byte the dispatcher will route to
--   count     : (a0 -> cmd_word) &  0xFFFF         -- batch count
--
-- The cmd_flags bank selection per FUN_80043390 disasm:
--   if (a2 != 0):
--       bank = 0x50                            ; baseline
--       if (a1 & 0x04000000): bank = 0xA0
--       if (a1 & 0x20000000): bank = 0xF0       ; sequential ifs, last wins
--   else: bank = 0x00
-- so four banks exist (0x00, 0x50, 0xA0, 0xF0), not three.
--
-- Run:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/<your-drake-on-map01-save>.sstate \
--   LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=60 \
--   LEGAIA_FRAMES=1800 \
--   LEGAIA_OUT=captures/slot4_dispatcher/drake.csv \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_slot4_dispatcher_args.lua \
--       timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local OUT_PATH    = probe.out_path("slot4_dispatcher_args.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local MAX_HITS    = probe.getenv_num("LEGAIA_DISP_CAP", 200000)

local csv = probe.csv_open(OUT_PATH,
    "ra,a0,a1,a2,kind,count,bank")

local function bank_of(a1, a2)
    if a2 == 0 then return 0x00 end
    local b = 0x50
    if bit.band(a1, 0x04000000) ~= 0 then b = 0xA0 end
    if bit.band(a1, 0x20000000) ~= 0 then b = 0xF0 end
    return b
end

local function read_u32(addr)
    local mem = PCSX.getMemoryAsFile()
    local buf = mem:readAt(4, bit.band(addr, 0x1FFFFFFF))
    if buf == nil then return 0 end
    local s = tostring(buf)
    return string.byte(s, 1)
        + string.byte(s, 2) * 0x100
        + string.byte(s, 3) * 0x10000
        + string.byte(s, 4) * 0x1000000
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function()
        local d = {
            addr     = 0x80043390,
            name     = "FUN_80043390_entry",
            hits_ref = { n = 0, capped = false },
        }
        probe.arm_breakpoint(0x80043390, "Exec", 4, d.name, function()
            d.hits_ref.n = d.hits_ref.n + 1
            if d.hits_ref.n > MAX_HITS then
                if not d.hits_ref.capped then
                    PCSX.log(string.format(
                        "[s4disp] cap reached at %d hits", MAX_HITS))
                    d.hits_ref.capped = true
                end
                return
            end
            local r  = PCSX.getRegisters()
            local ra = tonumber(r.GPR.n.ra) or 0
            local a0 = tonumber(r.GPR.n.a0) or 0
            local a1 = tonumber(r.GPR.n.a1) or 0
            local a2 = tonumber(r.GPR.n.a2) or 0
            -- a0 is a struct ptr; descriptor format (see FUN_80043390 disasm):
            --   *(a0+4) = puVar1  (command-stream pointer)
            --   *(a0+0x14) != 0   gates the inner block
            --   *(puVar1)         = command word
            -- We read puVar1 then the cmd word it points at.
            local cmd_word_ptr = read_u32(a0 + 0x10)
            local cmd_word     = 0
            if cmd_word_ptr ~= 0 and cmd_word_ptr >= 0x80000000 and cmd_word_ptr < 0x80200000 then
                cmd_word = read_u32(cmd_word_ptr)
            end
            local kind  = bit.band(bit.rshift(cmd_word, 17), 0x7FFF)
            local count = bit.band(cmd_word, 0xFFFF)
            local bank  = bank_of(a1, a2)
            csv:row("0x%08X,0x%08X,0x%08X,0x%08X,%d,%d,0x%02X",
                ra, a0, a1, a2, kind, count, bank)
            if d.hits_ref.n <= 3 then
                PCSX.log(string.format(
                    "[s4disp] hit %d: ra=0x%08X a0=0x%08X a1=0x%08X a2=0x%08X kind=%d count=%d bank=0x%02X",
                    d.hits_ref.n, ra, a0, a1, a2, kind, count, bank))
            end
        end)
        PCSX.log("[s4disp] dispatcher-entry probe armed at 0x80043390")
        return { d }
    end,

    on_done = function(_, descs)
        csv:close()
        PCSX.log("[s4disp] CSV closed: " .. OUT_PATH)
        if descs and descs[1] then
            PCSX.log(string.format(
                "[s4disp] total dispatcher hits: %d (capped=%s)",
                descs[1].hits_ref.n,
                tostring(descs[1].hits_ref.capped)))
        end
    end,
})
