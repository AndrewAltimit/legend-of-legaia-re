-- autorun_chest_give_trace.lua
--
-- Settles whether a randomizer chest-item patch reaches the running game.
--
-- The chest randomizer rewrites the field-VM GIVE_ITEM (op 0x39) inline operand
-- byte in a scene's MAN. The clean-room engine + the offline re-decode both prove
-- the patched bytes are what the scene loader reads. But on real disc images a
-- patched chest was still granting its ORIGINAL item, so this probe captures, at
-- runtime, exactly what the retail give-item path reads and from where.
--
-- Breakpoints (addresses from docs/subsystems/script-vm.md + functions.md):
--   0x801E0450  the `lbu a0,0(s6)` in FUN_801DE840's op-0x39 case: s6 = the RAM
--               address of the chest's item-id operand, mem[s6] = the value the
--               game is about to grant. This is THE decisive read: if the loaded
--               MAN is patched, mem[s6] is the new id; if not, it's the original.
--   0x800421D4  the inventory add-by-id primitive (a0 = id, a1 = count): the
--               actually-granted item, across the whole give path.
--
-- It pulses CROSS (X) every ~30 vsyncs to open the chest the player is standing
-- in front of and advance the announcement dialogue until the give fires, then
-- quits on the first add.
--
-- Run with a state standing in front of a chest, on a randomizer-PATCHED disc
-- (so any re-read streams patched bytes); needs a display (PCSX-Redux is not
-- headless). Point LEGAIA_ISO at the patched image:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
--   LEGAIA_ISO="/path/to/patched.bin" \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_chest_give_trace.lua \
--   LEGAIA_FRAMES=900 \
--       timeout --kill-after=30s 300s bash scripts/pcsx-redux/run_probe.sh
--
-- NOTE: blind X-pulsing here may not open every chest cleanly; if no inv_add
-- hit lands, drive the open interactively while the BPs are armed.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("chest_give_trace.csv")

local OP39_OPERAND_READ = 0x801E0450 -- lbu a0,0(s6) : s6 = operand RAM addr
local INV_ADD           = 0x800421D4 -- FUN_800421D4(id=a0, count=a1)
local SCENE_NAME_BUF    = 0x80084548 -- active CDNAME scene name (16 bytes)

local function read_u8(addr)
    return bit.band(probe.read_u32(addr) or 0, 0xFF)
end

local function read_cstr(addr, max)
    local out = {}
    for i = 0, max - 1 do
        local b = read_u8(addr + i)
        if b == 0 then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local csv = probe.csv_open(OUT_PATH, "tick,event,pc,operand_addr,operand_val,add_id,add_count")
local op39_hits = 0
local add_hits = 0
local last_operand = nil

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function(ctx)
        local scene = read_cstr(SCENE_NAME_BUF, 16)
        PCSX.log(string.format("[chest] active scene = %q", scene))

        -- op-0x39 operand read: capture s6 (operand RAM addr) + the byte there,
        -- plus a window of MAN bytes around it for context.
        probe.arm_breakpoint(OP39_OPERAND_READ, "Exec", 4, "op39_operand", function()
            op39_hits = op39_hits + 1
            local r = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
            local s6 = bit.band(tonumber(r.GPR.n.s6) or 0, 0xFFFFFFFF)
            local val = probe.in_ram(s6, 1) and read_u8(s6) or -1
            last_operand = val
            csv:row("%d,op39_operand,0x%08X,0x%08X,%d,,", op39_hits, pc, s6, val)
            PCSX.log(string.format(
                "[chest] OP39 #%d operand @0x%08X = %d (0x%02X)", op39_hits, s6, val, val))
            if probe.in_ram(s6 - 8, 24) then
                local w = probe.read_bytes(s6 - 8, 24)
                local hx = {}
                for i = 1, #w do hx[#hx + 1] = string.format("%02X", w:byte(i)) end
                PCSX.log("[chest]   MAN window [-8..+16): " .. table.concat(hx, " "))
            end
        end)

        -- Inventory add: the actually-granted item id.
        probe.arm_breakpoint(INV_ADD, "Exec", 4, "inv_add", function()
            add_hits = add_hits + 1
            local r = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
            local id = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFF)
            local n  = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            csv:row("%d,inv_add,0x%08X,,,%d,%d", add_hits, pc, id, n)
            PCSX.log(string.format(
                "[chest] ADD  #%d FUN_800421D4(id=%d 0x%02X, count=%d) ra=0x%08X",
                add_hits, id, id, n, ra))
            -- Quit shortly after the give fires (let the row flush).
            ctx.request_quit = true
        end)

        return {
            { addr = OP39_OPERAND_READ, name = "op39_operand" },
            { addr = INV_ADD, name = "inv_add" },
        }
    end,

    -- Pulse CROSS to open the chest + advance its announcement dialogue.
    on_capture = function(_ctx, elapsed)
        local phase = elapsed % 30
        if phase < 5 then
            probe.pad_force(probe.BTN.CROSS)
        else
            probe.pad_release(probe.BTN.CROSS)
        end
    end,
})

PCSX.log(string.format("[chest] DONE op39_hits=%d add_hits=%d last_operand=%s",
    op39_hits, add_hits, tostring(last_operand)))
