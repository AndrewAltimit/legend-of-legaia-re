-- autorun_model_pack_registrar.lua
--
-- Does the party-model registrar walk a buffer nothing filled this pass?
--
-- `FUN_8001E890` decompresses PROT 0874's three sections and then registers
-- section 0's pack into the model pool. The two halves are gated differently:
--
--   0x8001EA54  bne v1, 2 -> 0x8001EAFC    the three `FUN_8001A55C` calls run
--                                          only when the mode word
--                                          `0x8007B83C` is 2 ...
--   0x8001EAFC  lw a0, 0x6bc(gp)           ... and this is the branch TARGET,
--   0x8001EB34  lw a0, 0x6bc(gp)           so the registrar loop runs on every
--   0x8001EB40  lw v0, 4(v0)               path, over whatever the buffer holds,
--   0x8001EB4C  jal 0x80026b4c             with an unclamped count and an
--                                          unbounded per-member word offset.
--
-- The buffer is allocated once (`FUN_80017888(0, *(gp+0x69C))` at 0x8001E2E0,
-- the only writer of `gp+0x6BC`) and the save corpus shows it holding the
-- 5-member pack in every field state and garbage in every battle one - so the
-- question this probe answers is whether the registrar is ever ENTERED while
-- it holds the garbage.
--
-- Rows are written at the registrar's entry, before it reads anything, so a
-- row whose `count` is not the field pack's is the wild walk itself.
--
-- Outputs (probe.out_path):
--   registrar.csv  one row per entry to 0x8001EAFC: mode word, pack pointer,
--                  the count word, the first four member offsets, and `ra`.
--
-- Env:
--   LEGAIA_HOLD_BUTTON / LEGAIA_HOLD  pad bit + vsyncs to hold (walk into an
--                                     encounter from a pre-encounter state)
--   LEGAIA_PRESS_UNTIL                keep tapping CROSS until this vsync (0 = off)
--   LEGAIA_LABEL                      free-text label for manifest.txt
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 1800)
local HOLD_BTN   = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_N     = probe.getenv_num("LEGAIA_HOLD", 0)
local PRESS_TILL = probe.getenv_num("LEGAIA_PRESS_UNTIL", 0)
local LABEL      = probe.getenv("LEGAIA_LABEL", "registrar")

local REGISTRAR = 0x8001EAFC
local DECOMP    = 0x8001EA64      -- the section-0 decompress call site
local PACK_PTR  = 0x8007B9D4      -- gp+0x6BC
local MODE_W    = 0x8007B83C      -- the `== 2` gate
local BANK      = 0x8007B6F8      -- scene model-bank base the registrar writes

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local csv
local elapsed_now, n_reg, n_dec = 0, 0, 0

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    hold_button = HOLD_BTN ~= 0 and HOLD_BTN or nil,
    hold_frames = HOLD_N,
    on_arm = function()
        PCSX.log(string.format("== model pack registrar == label=%s", LABEL))
        probe.env.write_manifest("autorun_model_pack_registrar.lua", {
            label = LABEL, sstate = SSTATE, frames = FRAMES,
            hold_button = HOLD_BTN, hold = HOLD_N,
        })
        csv = probe.csv_open(probe.out_path("registrar.csv"),
            "vsync,kind,mode,pack,count,off0,off1,off2,off3,bank,ra")
        local function row(kind)
            local n = PCSX.getRegisters()
            n = (n.GPR and n.GPR.n) or {}
            local p = u32(PACK_PTR)
            local cnt, o = -1, {-1, -1, -1, -1}
            if p >= 0x80000000 and p < 0x80200000 then
                cnt = u32(p)
                for i = 1, 4 do o[i] = u32(p + i * 4) end
            end
            csv:row("%d,%s,%d,0x%08X,%d,%d,%d,%d,%d,%d,0x%08X",
                elapsed_now, kind, u16(MODE_W), p, cnt,
                o[1], o[2], o[3], o[4], u32(BANK), tou32(n.ra))
        end
        probe.arm_breakpoint(REGISTRAR, "Exec", 4, "registrar", function()
            n_reg = n_reg + 1
            row("enter")
        end)
        probe.arm_breakpoint(DECOMP, "Exec", 4, "decomp0", function()
            n_dec = n_dec + 1
            row("decomp")
        end)
        return {}
    end,
    on_capture = function(_, elapsed)
        elapsed_now = elapsed
        if PRESS_TILL > 0 then
            probe.pad_release(probe.BTN.CROSS)
            local sub = elapsed % 60
            if elapsed < PRESS_TILL and sub >= 30 and sub < 34 then
                probe.pad_force(probe.BTN.CROSS)
            end
        end
    end,
    on_done = function()
        PCSX.log(string.format("[registrar] entries=%d decompresses=%d", n_reg, n_dec))
    end,
})
