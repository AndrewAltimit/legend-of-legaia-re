-- autorun_seru_overlay_shop_slice.lua
--
-- Validates the SHOP-TRIGGERED custom-overlay path's full logic on the emulator,
-- without gameplay. A vanilla save state's RAM has no patched code, so we inject
-- the patched detour + stub (from `<disc>.rampatch`, emitted by the
-- overlay_slice_bin example = the Rust assemblers), FlushCache them coherent,
-- then fire a synthetic op-0x49 sub-op 0 (a merchant): we call FlushCache and
-- return (ra) straight into the detour site, which `j`s to the stub. The stub
-- gates on the sub-op (*s6==0), loads the overlay off the patched disc,
-- FlushCaches, runs it (writes the sentinel), replays the displaced pair, and
-- `j`s to the op-0x49 return (0x801E09B0) where we catch it and read the
-- sentinel. Proves: detour routing + sub-op gate + the stub chain (incl. its own
-- FlushCache) + correct return.
--
-- Requires the PATCHED disc (--iso) + its sidecar at <iso>.rampatch.
--
-- Run:
--   timeout --kill-after=30s 300s bash scripts/pcsx-redux/run_probe.sh \
--     --iso /tmp/legaia_slice.bin \
--     --lua scripts/pcsx-redux/autorun_seru_overlay_shop_slice.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local RAMPATCH = probe.getenv("LEGAIA_RAMPATCH", "/tmp/legaia_slice.bin.rampatch")

local BIOS_A      = 0x000000A0  -- BIOS A-table dispatcher
local FLUSH_CACHE = 0x44        -- A-func: FlushCache
local SHOP_HOOK   = 0x801E09A8  -- op-0x49 arm-edge detour site
local SHOP_RETURN = 0x801E09B0  -- where the stub j's back
local SENTINEL_ADDR = 0x8007AF20
local SENTINEL    = 0x5E2D7ADE
local SUBOP_CELL  = 0x8007AF3C  -- a scratch byte we set to 0 (= shop sub-op)
local S0_BASE     = 0x80080000  -- so `sw s6,-0x4bb0(s0)` hits _DAT_8007b450

local function w32(addr, v)
    probe.write_u16(addr, v % 0x10000)
    probe.write_u16(addr + 2, math.floor(v / 0x10000) % 0x10000)
end

local function apply_rampatch(path)
    local f = assert(io.open(path, "r"), "rampatch not found: " .. path)
    local n = 0
    for line in f:lines() do
        local a, w = line:match("(%x+)%s+(%x+)")
        if a and w then
            w32(tonumber(a, 16), tonumber(w, 16))
            n = n + 1
        end
    end
    f:close()
    return n
end

local st = { phase = 0, returned = false, got = nil }

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function()
        local n = apply_rampatch(RAMPATCH)
        w32(SENTINEL_ADDR, 0)         -- clear sentinel
        probe.write_u8(SUBOP_CELL, 0) -- sub-op = 0 (shop)
        PCSX.log(string.format("[shop] applied %d ram-patch words; armed", n))
        -- Catch the stub's return (j SHOP_RETURN) and read the sentinel there.
        probe.arm_breakpoint(SHOP_RETURN, "Exec", 4, "shop_return", function()
            if not st.returned then
                st.returned = true
                st.got = probe.read_u32(SENTINEL_ADDR)
                PCSX.log(string.format(
                    "[shop] returned to 0x%08X; sentinel=%08X", SHOP_RETURN, st.got))
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if st.phase == 0 and elapsed >= 2 then
            -- Fire it: FlushCache (coherent the injected detour+stub), returning
            -- into the detour site, with a shop sub-op set up.
            local r = PCSX.getRegisters()
            r.GPR.n.t1 = FLUSH_CACHE      -- A-func number for the dispatcher
            r.GPR.n.s6 = SUBOP_CELL       -- operand ptr (sub-op byte = 0 = shop)
            r.GPR.n.s0 = S0_BASE          -- base for the displaced `sw s6,..(s0)`
            r.GPR.n.ra = SHOP_HOOK        -- FlushCache returns into the detour
            r.pc = BIOS_A                 -- call the BIOS A-dispatcher (FlushCache)
            st.phase = 1
            PCSX.log("[shop] phase1: FlushCache -> detour (synthetic shop op-0x49)")
        elseif st.phase == 1 and st.returned then
            st.phase = 2
            ctx.request_quit = true
        end
    end,
    on_done = function()
        local pass = st.got == SENTINEL
        PCSX.log("=== seru overlay SHOP slice ===")
        PCSX.log(string.format("  returned: %s", tostring(st.returned)))
        PCSX.log(string.format("  sentinel: %s (%s)", tostring(pass),
            st.got and string.format("%08X", st.got) or "nil"))
        PCSX.log(string.format("  RESULT:   %s", pass and "PASS" or "FAIL"))
        PCSX.log("=== end ===")
    end,
})
