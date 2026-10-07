-- Who reads a character's equipped-item bytes, and when: a read watchpoint on
-- one equipment byte of the character record (default Noa's weapon, +0x199)
-- plus exec breakpoints on the player-file loader entry points
-- (`FUN_80052770` assembler SM, `FUN_800558FC` dual-mode open, `FUN_8003E8A8`
-- TOC resolver). Answers "does this battle re-stream PLAYER1..4 at all" -
-- a forced encounter does not; the assembled party meshes stay resident.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE = 0x8007B83C
local CHAR_REC  = 0x80084708
local REC_STRIDE = 0x414
local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local CHAR     = tonumber(probe.getenv("LEGAIA_CHAR", "1"))
local EQUIP_OFF = tonumber(probe.getenv("LEGAIA_EQUIP_OFF", "0x199"))
local PRESS_DOWN_AT = probe.getenv_num("LEGAIA_PRESS_DOWN_AT", -1)
local PRESS_DOWN_FRAMES = probe.getenv_num("LEGAIA_PRESS_DOWN_FRAMES", 60)

local byte_addr = CHAR_REC + CHAR * REC_STRIDE + EQUIP_OFF
local readers, nread, elapsed_now, last_mode = {}, 0, 0, -1
local opens = {}

local function regs()
    local r = PCSX.getRegisters()
    local n = r.GPR and r.GPR.n or {}
    return tonumber(r.pc) or 0, tonumber(n.ra) or 0, tonumber(n.a0) or 0, tonumber(n.a3) or 0
end

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("[readers] watching byte 0x%08X (char %d +0x%X)", byte_addr, CHAR, EQUIP_OFF))
        probe.arm_breakpoint(byte_addr, "Read", 1, "equip_read", function()
            local pc, ra = regs()
            nread = nread + 1
            if not readers[pc] then
                readers[pc] = 0
                PCSX.log(string.format("[readers] t%d read by pc=0x%08X ra=0x%08X mode=0x%X", elapsed_now, pc, ra, probe.read_u16(GAME_MODE) or 0))
            end
            readers[pc] = readers[pc] + 1
        end)
        probe.arm_breakpoint(0x80052770, "Exec", 4, "assembler_sm", function()
            opens.sm = (opens.sm or 0) + 1
        end)
        probe.arm_breakpoint(0x800558FC, "Exec", 4, "dual_open", function()
            local _, _, _, a3 = regs()
            local idx = a3 % 0x10000
            if idx >= 0x361 and idx <= 0x364 then
                PCSX.log(string.format("[readers] t%d PLAYER%d file opened (toc 0x%X) mode=0x%X", elapsed_now, idx - 0x360, idx, probe.read_u16(GAME_MODE) or 0))
            end
            opens.open = (opens.open or 0) + 1
        end)
        probe.arm_breakpoint(0x8003E8A8, "Exec", 4, "toc_resolve", function()
            local _, _, a0 = regs()
            if a0 >= 0x361 and a0 <= 0x364 then
                PCSX.log(string.format("[readers] t%d toc resolve PLAYER%d", elapsed_now, a0 - 0x360))
            end
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        elapsed_now = elapsed
        local mode = probe.read_u16(GAME_MODE) or -1
        if mode ~= last_mode then PCSX.log(string.format("[readers] t%d mode 0x%X", elapsed, mode)); last_mode = mode end
        if PRESS_DOWN_AT >= 0 then
            if elapsed == PRESS_DOWN_AT then pad.force(pad.BTN.DOWN) end
            if elapsed == PRESS_DOWN_AT + PRESS_DOWN_FRAMES then pad.release(pad.BTN.DOWN) end
        end
    end,
    on_done = function()
        PCSX.log(string.format("[readers] done reads=%d sm_entries=%d opens=%d", nread, opens.sm or 0, opens.open or 0))
        for pc, n in pairs(readers) do PCSX.log(string.format("[readers]   pc=0x%08X x%d", pc, n)) end
    end,
})
