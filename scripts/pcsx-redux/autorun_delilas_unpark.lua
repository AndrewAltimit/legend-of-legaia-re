-- autorun_delilas_unpark.lua
--
-- Resume the parked Delilas double-team round (the dome-course probe's
-- end-state sstate) and try to unstick the battle top menu (match phase
-- ctx[6] = 0x1E - the Begin/Run input wait):
--   LEGAIA_UNPARK=phase  poke ctx[6] = 0x28 at tick 60 (skip into the
--                        per-actor command cluster)
--   LEGAIA_UNPARK=pad    hold START then CROSS in long alternating pulses
--   LEGAIA_UNPARK=none   observe only
-- Logs the phase byte + game mode every 30 ticks.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE = 0x8007B83C
local CTX_PTR   = 0x8007BD24

local MODE = probe.getenv("LEGAIA_UNPARK", "none")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)

local CSV = probe.csv_open(probe.out_path("delilas_unpark.csv"), "tick,mode,note")

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function()
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        local bctx = probe.read_u32(CTX_PTR) or 0
        local phase = -1
        if bctx > 0x80000000 and bctx < 0x80200000 then
            phase = probe.read_u8(bctx + 6) or -1
        end
        if elapsed == 60 and MODE == "phase" then
            probe.write_u8(bctx + 6, 0x28)
            CSV:row("%d,0x%X,poked phase 0x1E->0x28", elapsed, mode)
        end
        if MODE == "cloneclear" then
            -- Staged unstick of the Divide-clone act-latch: the spawned copy
            -- inherits the caster's mid-cast action state (+0x1DD=9,
            -- +0x1DE=2, +0x1DF=0x50) whose completion belongs to the
            -- original's cast. Clear progressively and watch the phase.
            local s3 = probe.read_u32(0x801C9370 + 3 * 4) or 0
            local s4 = probe.read_u32(0x801C9370 + 4 * 4) or 0
            if elapsed == 60 and s4 > 0x80000000 then
                probe.write_u8(s4 + 0x1DE, 0)
                CSV:row("%d,0x%X,cleared clone +0x1DE", elapsed, mode)
            elseif elapsed == 600 and s4 > 0x80000000 then
                probe.write_u8(s4 + 0x1DD, 0)
                probe.write_u8(s4 + 0x1DF, 0)
                CSV:row("%d,0x%X,cleared clone +0x1DD/+0x1DF", elapsed, mode)
            elseif elapsed == 1200 and s3 > 0x80000000 then
                probe.write_u8(s3 + 0x1DE, 0)
                CSV:row("%d,0x%X,cleared original +0x1DE", elapsed, mode)
            end
        end
        if MODE == "poke874" and elapsed > 30 then
            -- Bypass the pad pipeline: write the accept bit straight into
            -- the processed-input word for a few consecutive frames.
            local ph = elapsed % 120
            if ph >= 0 and ph < 4 then
                probe.write_u32(0x8007B874, 0x8000)
            elseif ph >= 60 and ph < 64 then
                probe.write_u32(0x8007B874, 0x2000)
            end
        end
        if MODE == "pad" and elapsed > 30 then
            local ph = elapsed % 80
            if ph == 0 then
                pad.force(pad.BTN.START)
            elseif ph == 30 then
                pad.release(pad.BTN.START)
            elseif ph == 40 then
                pad.force(pad.BTN.CROSS)
            elseif ph == 60 then
                pad.release(pad.BTN.CROSS)
                pad.force(pad.BTN.SQUARE)
            elseif ph == 78 then
                pad.release(pad.BTN.SQUARE)
            end
        end
        if elapsed % 10 == 0 then
            local pc, ra, sp, k1 = 0, 0, 0, 0
            local ok, r = pcall(PCSX.getRegisters)
            if ok then
                local function g(n)
                    local ok2, v = pcall(function() return tonumber(r.GPR.n[n]) % 0x100000000 end)
                    return ok2 and v or 0
                end
                local ok3, v3 = pcall(function() return tonumber(r.pc) % 0x100000000 end)
                pc = ok3 and v3 or 0
                ra = g("ra"); sp = g("sp"); k1 = g("s8")
            end
            local stack = {}
            if sp > 0x80000000 and sp < 0x80200000 then
                for i = 0, 11 do
                    local wv = probe.read_u32(sp + i * 4) or 0
                    if wv > 0x80010000 and wv < 0x80200000 then
                        stack[#stack + 1] = string.format("0x%08X", wv)
                    end
                end
            end
            CSV:row("%d,0x%X,phase=0x%02X pc=0x%08X ra=0x%08X sp=0x%08X stackptrs=%s",
                elapsed, mode, phase, pc, ra, sp, table.concat(stack, " "))
        end
        if elapsed % 100 == 0 then
            -- Kernel TCB saved context (the receipt that caught the AI
            -- div-by-zero `break` park): EPC at TCB+0x88, ra at +0x84.
            -- 0xA000E1F4 physical = 0x8000E1F4 through the probe's KSEG0 map.
            CSV:row("%d,0x%X,tcb epc=0x%08X ra=0x%08X", elapsed, mode,
                probe.read_u32(0x8000E1F4 + 0x88) or 0,
                probe.read_u32(0x8000E1F4 + 0x84) or 0)
        end
    end,
    on_done = function()
        CSV:close()
    end,
})
