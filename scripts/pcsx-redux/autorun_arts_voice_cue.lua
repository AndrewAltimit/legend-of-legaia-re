-- autorun_arts_voice_cue.lua
--
-- Traces every CD-XA streaming-clip cue that fires while a Tactical / Super
-- Art executes in battle, to answer "which sound does an art play, and what
-- selects it?" empirically instead of by ear.
--
-- Arms exec breakpoints on the SCUS XA-clip entry points and logs the
-- (clip_slot a0, chan a1, dur a2) + caller ra of each hit:
--   * FUN_8003D53C  clip-start by (slot, chan, dur)  [XA30 battle grunt path]
--   * FUN_8003EAE4  clip-start by table index (whole file, no chan filter)
--   * FUN_8004FCC8  menu cue / voice dispatcher (id>=0x100 -> voice)
-- clip slot i decodes to file XA<i+1>.XA (table 0x801C6ED8, 8-byte stride).
--
-- Run on the Vahn Tri-Somersault Super save (auto-executes ~2 s after load,
-- no input needed):
--   LEGAIA_FRAMES=1200 timeout --kill-after=20s 900s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario battle_vahn_tri_somersault_super \
--       --lua scripts/pcsx-redux/autorun_arts_voice_cue.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1200)
local OUT_PATH = probe.out_path("arts_voice_cue.csv")

local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local Q_OFF       = 0x1DF      -- action-parameter byte stream head

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end

local csv = probe.csv_open(OUT_PATH, "tick,fn,clip_slot,xa_file,chan,dur,ra,queue_head")
local tick = 0
local armed = false

-- clip slot i -> file XA<i+1>.XA
local function xa_file(slot) return string.format("XA%d.XA", (tonumber(slot) or 0) + 1) end

local function queue_head()
    local p = n32(probe.read_u32(ACTOR_TABLE)) -- party slot 0 (acting hero)
    if not probe.in_ram(p) then return "" end
    local b = probe.read_bytes(p + Q_OFF, 0x14)
    return b and probe.bytes_to_hex(b):gsub("%s+", "") or ""
end

local function log_hit(fn, a0, a1, a2)
    tick = tick + 1
    local r = PCSX.getRegisters()
    local ra = n32(r.GPR.n.ra)
    local qh = queue_head()
    -- Log FIRST so the datum survives even if the CSV sink is unavailable.
    PCSX.log(string.format(
        "[voice] #%d %s(slot=0x%02X=%s, chan=%d, dur=0x%02X) ra=0x%08X q=[%s]",
        tick, fn, a0, xa_file(a0), a1, a2, ra, qh))
    if csv then
        pcall(function()
            csv:row("%d,%s,0x%02X,%s,%d,0x%02X,0x%08X,%s",
                tick, fn, a0, xa_file(a0), a1, a2, ra, qh)
        end)
    end
end

local function arm_all()
    if armed then return end
    if not probe.in_ram(n32(probe.read_u32(ACTOR_TABLE))) then return end
    armed = true
    PCSX.log("[voice] battle actor table resolved; arming XA-clip breakpoints")
    probe.arm_breakpoint(0x8003D53C, "Exec", 4, "clip_start", function()
        local r = PCSX.getRegisters()
        log_hit("8003D53C", n32(r.GPR.n.a0), n32(r.GPR.n.a1), n32(r.GPR.n.a2))
    end)
    probe.arm_breakpoint(0x8003EAE4, "Exec", 4, "clip_start_idx", function()
        local r = PCSX.getRegisters()
        log_hit("8003EAE4", n32(r.GPR.n.a1), 0, 0) -- (unused, clip_index)
    end)
    probe.arm_breakpoint(0x8004FCC8, "Exec", 4, "menu_cue", function()
        local r = PCSX.getRegisters()
        local id = n32(r.GPR.n.a0)
        if id >= 0x100 then
            log_hit("8004FCC8", bit.rshift(id - 0x100, 3), bit.band(id, 7), 0)
        end
    end)
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),
    on_arm = function()
        PCSX.log("[voice] deferring arm until the save state loads (on_capture)")
        return {}
    end,
    on_capture = function(_hctx)
        arm_all()
    end,
    on_done = function()
        csv:close()
        PCSX.log(string.format("=== arts-voice-cue probe: armed=%s cues=%d ===",
            tostring(armed), tick))
        if not armed then
            PCSX.log("[voice] actor table 0x801C9370 never resolved -- not a battle save?")
        elseif tick == 0 then
            PCSX.log("[voice] NO XA-clip cue fired during the art -- arts voice is not "
                .. "a CD-XA stream in this build (SPU/VAG path or silent).")
        end
    end,
})
