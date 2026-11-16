-- autorun_audio_timeline.lua
--
-- Where the music stands against the picture, vsync by vsync, from a state
-- that plays on with no input (an opening or credits chain, a scripted
-- beat). One row per vsync to timeline.csv:
--
--   v, mode (0x8007B83C), scene (0x8007050C), frame (gp+0x480 = 0x8007B798),
--   word (0x8007BAC8), req / loaded (0x8007BAB8 / 0x8007BA9C),
--   flags (0x8007B750), then for the field-BGM source 0x8007052C and the
--   ending source 0x800705BC: open (+0x8), the libsnd sequence (+0xA), its
--   play flags (+0x98), tick wait (+0x90), the read cursor's byte offset from
--   the score start (+0x00 - +0x04) and volume (+0x58); then the sounding
--   voice records split by owner (0x21 = the cue slot), the frame step
--   (0x1F800393) and the op-0x35 sub-op 5 timed release (FUN_800267A8):
--   armed (gp+0x808), deadline (gp+0x814), elapsed (gp+0x81C).
--
-- A scene change shows as a run of mode 0x02 (the field MAIN INIT) between
-- two runs of mode 0x03, and the score's cursor offset on each side of it
-- says how far the music moved while the picture held.
--
-- Env:
--   LEGAIA_SSTATE   state (run_probe.sh --scenario sets it)
--   LEGAIA_FRAMES   vsyncs to run (default 12000)
--   LEGAIA_CROSS_EVERY  tap Cross every N vsyncs (default 0 = never)
--
-- Vsync-driven, no breakpoints: run with --fast. Output is retail
-- RAM-derived: never commit it.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 12000)
local CROSS_EVERY = probe.getenv_num("LEGAIA_CROSS_EVERY", 0)

local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(0x8007050C + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function src(base)
    local seq = probe.read_u16(base + 0xA) or 0
    local rec = probe.read_u32(0x801CD2C0 + (seq % 32) * 4) or 0
    local flags, wait, off, vol = -1, -1, -1, -1
    if ok_ptr(rec) then
        flags = probe.read_u32(rec + 0x98) or -1
        wait = probe.read_u32(rec + 0x90) or -1
        off = (probe.read_u32(rec) or 0) - (probe.read_u32(rec + 4) or 0)
        vol = probe.read_u16(rec + 0x58) or -1
    end
    return string.format("%d,%d,%d,%d,%d,%d", probe.read_u16(base + 8) or 0, seq, flags, wait, off, vol)
end

local out
local held = nil

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        out = probe.csv_open(probe.out_path("timeline.csv"),
            "v,mode,scene,frame,word,req,loaded,flags,f_open,f_seq,f_flags,f_wait,f_off,f_vol,"
            .. "e_open,e_seq,e_flags,e_wait,e_off,e_vol,music,cue,step,rel_arm,rel_deadline,rel_elapsed")
        return {}
    end,
    on_capture = function(_, v)
        if held and v >= held then
            probe.pad.release(probe.BTN.CROSS)
            held = nil
        end
        if CROSS_EVERY > 0 and not held and v % CROSS_EVERY == 0 then
            probe.pad.force(probe.BTN.CROSS)
            held = v + 4
        end
        local music, cue = 0, 0
        for i = 0, 23 do
            local r = 0x801CDB50 + i * 0x36
            if (probe.read_u16(r + 6) or 0) ~= 0 then
                if (probe.read_u16(r + 0x10) or 0) == 0x21 then cue = cue + 1 else music = music + 1 end
            end
        end
        out:row("%d,%d,%s,%d,%d,%d,%d,%d,%s,%s,%d,%d,%d,%d,%d,%d", v, probe.read_u8(0x8007B83C) or 0, scene_name(),
            probe.read_u32(0x8007B798) or 0, probe.read_u32(0x8007BAC8) or 0,
            probe.read_u32(0x8007BAB8) or 0, probe.read_u32(0x8007BA9C) or 0,
            probe.read_u32(0x8007B750) or 0, src(0x8007052C), src(0x800705BC), music, cue,
            probe.read_scratch_u8(0x1F800393) or 0, probe.read_u32(0x8007BB20) or 0,
            probe.read_u32(0x8007BB2C) or 0, probe.read_u32(0x8007BB34) or 0)
    end,
    on_done = function()
        if out then out:close() end
        PCSX.quit(0)
    end,
})
