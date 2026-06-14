-- autorun_arts_build_reader.lua
--
-- Catch the arts-bar BUILD-time weapon-class consult. Loads a state saved the
-- instant arts command-input BEGINS (gauge not yet drawn) for Gala holding the
-- off-class Nail Glove, then runs forward while the gauge builds, read-watching
-- Gala's equipped-weapon byte (char+0x198 = 0x800850C8). Every reader pc that
-- fires during the build names the arm-width consumer; the code around it indexes
-- the per-weapon-class data the randomizer needs. Unlike the mid-input arts_bar
-- states, this one is armed BEFORE the build, so the one-shot weapon read is in
-- the capture window.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_arts_build_reader.lua \
--     --scenario arts_input_start_gala_nail --frames 300

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 300)

local GMODE = 0x8007B83C
-- Read-watch the whole active-character equip block, not just the weapon, in case
-- the class is read via a neighbouring slot. Gala = party slot 2.
--   equip block char+0x196.. ; weapon = +0x198. Record base 0x80084708+2*0x414.
local GALA = 0x80084708 + 2 * 0x414
local WATCH = {
    { name = "Gala.body",   addr = GALA + 0x196 },
    { name = "Gala.weapon", addr = GALA + 0x198 },
}

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function gmode() return probe.read_u8(GMODE) or 0xFF end

local readers = {}   -- pc -> {count, first_frame, gm, ra, who, val}
local total = 0
local last_gm = -1

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== arts-bar BUILD weapon-class reader trace ==")
        PCSX.log(string.format("[init] gmode=0x%02X  Gala.weapon[0x%08X]=0x%02X",
            gmode(), WATCH[2].addr, probe.read_u8(WATCH[2].addr) or 0xff))
        for _, w in ipairs(WATCH) do
            probe.arm_breakpoint(w.addr, "Read", 1, "rd_" .. w.name, function()
                local r = PCSX.getRegisters()
                local pc = tou32(r.pc)
                local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
                total = total + 1
                local e = readers[pc]
                if e == nil then
                    e = { count = 0, gm = gmode(), ra = ra, who = w.name,
                          val = probe.read_u8(w.addr) or 0 }
                    readers[pc] = e
                    PCSX.log(string.format(
                        "[READER new] pc=0x%08X ra=0x%08X %s val=0x%02X gmode=0x%02X",
                        pc, ra, w.name, e.val, e.gm))
                end
                e.count = e.count + 1
            end)
        end
        return {}
    end,
    on_capture = function(c, elapsed)
        local gm = gmode()
        if gm ~= last_gm then
            PCSX.log(string.format("[gmode] 0x%02X -> 0x%02X at t%d (readers=%d)",
                last_gm < 0 and 0 or last_gm, gm, elapsed, total))
            last_gm = gm
        end
        if elapsed % 60 == 0 then
            local n = 0 for _ in pairs(readers) do n = n + 1 end
            PCSX.log(string.format("[diag t%d] gmode=0x%02X total_reads=%d distinct_pcs=%d",
                elapsed, gm, total, n))
        end
    end,
})
