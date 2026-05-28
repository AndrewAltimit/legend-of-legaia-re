-- autorun_clut_upload_hook.lua
--
-- Captures the battle-form party CLUT band source by hooking the VRAM
-- upload routine FUN_80059bd4 (the LoadImage-equivalent, pinned via a
-- read-watchpoint on Vahn's CLUT source + the Ghidra dump). Signature:
--   FUN_80059bd4(a0 = RECT*, a1 = src_ptr)
--   RECT: [+0]=x (short), [+2]=y (short), [+4]=w, [+6]=h  (VRAM dest)
-- It DMAs / FIFO-copies a1 -> VRAM(x,y,w,h). So when the dest y is in
-- the character CLUT band (rows 488..499), a1 is the live source buffer
-- of that palette — dumped here before it is freed.
--
-- Run from a band-absent field state and walk into a battle so the
-- party CLUTs upload fresh:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate4 \
--   LEGAIA_FRAMES=2400 LEGAIA_HOLD=DOWN LEGAIA_HOLD_FRAMES=1800 \
--   LEGAIA_OUT_DIR=/tmp/clutprobe/uphook \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_clut_upload_hook.lua \
--       timeout --kill-after=30s 700s bash scripts/pcsx-redux/run_probe.sh
--
-- (slot 5 = "battle initiating" also works if the upload is still ahead
--  of that frame; slot 4 + walk guarantees a fresh field->battle load.)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local UPLOAD = 0x80059BD4
local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "DOWN")
local HOLD_FR = probe.getenv_num("LEGAIA_HOLD_FRAMES", 1800)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/clutprobe/uphook")
local LO = probe.getenv_num("LEGAIA_ROW_LO", 488)
local HI = probe.getenv_num("LEGAIA_ROW_HI", 499)

os.execute(string.format("mkdir -p %q", OUT_DIR))
local HOLD_BTN = pad.BTN[HOLD_NAME] or pad.BTN.DOWN

local csv = probe.csv_open(OUT_DIR .. "/uploads.csv",
    "tick,dst_x,dst_y,w,h,src_ptr,ra")

local function rd_u16(addr)
    local b = probe.read_bytes(addr, 2)
    if b == nil then return -1 end
    local s = tostring(b)
    return s:byte(1) + s:byte(2) * 256
end

local function dump_bytes(path, addr, len)
    if not probe.in_ram(addr, 1) then return false end
    local fh = io.open(path, "wb")
    if not fh then return false end
    local off = 0
    while off < len do
        local n = math.min(0x4000, len - off)
        local chunk = probe.read_bytes(addr + off, n)
        if chunk == nil then break end
        fh:write(tostring(chunk))
        off = off + n
    end
    fh:close()
    return true
end

local tick = 0
local hits = 0

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FR,
    out_path       = OUT_DIR .. "/uploads.csv",

    on_arm = function()
        probe.arm_breakpoint(UPLOAD, "Exec", 4, "vram_upload", function()
            local r = PCSX.getRegisters()
            local rect = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local src  = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            local ra   = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            if not probe.in_ram(rect, 8) then return end
            local x = rd_u16(rect + 0)
            local y = rd_u16(rect + 2)
            local w = rd_u16(rect + 4)
            local h = rd_u16(rect + 6)
            if y < LO or y > HI then return end       -- only the char-CLUT band
            hits = hits + 1
            csv:row("%d,%d,%d,%d,%d,0x%08X,0x%08X", tick, x, y, w, h, src, ra)
            PCSX.log(string.format(
                "[uphook] VRAM upload dst=(%d,%d) %dx%d src=0x%08X ra=0x%08X",
                x, y, w, h, src, ra))
            -- dump the source palette (w*h halfwords, capped) + context
            if probe.in_ram(src, 1) then
                local n = math.max(512, math.min(w * h * 2, 0x2000))
                dump_bytes(string.format("%s/upload_y%d_x%d_src%08X.bin", OUT_DIR, y, x, src),
                    src, n)
            end
        end)
        return {}
    end,

    on_capture = function(_ctx, elapsed) tick = elapsed end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== clut_upload_hook: %d band upload(s) captured ===", hits))
    end,
})
