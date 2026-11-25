-- autorun_dance_howto_film.lua
--
-- The dance hall's how-to lesson (mode 2), one row a vsync: the hall's state
-- `DAT_801D5334`, the load gate `DAT_801D5830`, the countdown hold
-- `DAT_801D6080`, the song hold `DAT_801D5138`, the frames danced
-- `DAT_801D5150`, the READY counter `DAT_801D5158`, the beat clock words
-- `DAT_801D581C` / `DAT_801D5820` / `DAT_801D5824`, the Disco King actor's
-- step `+0x9C` (the actor whose handler `+0x0C` is `FUN_801D0750`), the
-- frame pacer's ring cursor `0x8007B758` and step `0x1F800393`, and the cue
-- ring `0x8007B6D8`. The retail side of the count-in release the port's
-- `dance::CountIn` and `dance_tutorial` model. Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_PAD_SCRIPT  `A-B:BTN` spans in vsyncs from the load.
-- Output: dance_howto.csv.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3000)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")

local TUTORIAL = 0x801D0750

local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%w+)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    end
end

local function s32(a)
    local v = probe.read_u32(a) or 0
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

-- The Disco King's actor: the list entry whose handler is the lesson script.
local function king_step()
    for k = 0, 8 do
        local n, count = probe.read_u32(0x8007C34C + 4 * k), 0
        while probe.in_ram(n) and count < 512 do
            count = count + 1
            if probe.read_u32(n + 0x0C) == TUTORIAL then
                return s16(n + 0x9C)
            end
            n = probe.read_u32(n)
        end
    end
    return -1
end

local held = {}
local fh = assert(io.open(probe.out_path("dance_howto.csv"), "w"))
fh:write("vsync,mode,state,gate,hold_timer,song_hold,danced,ready,clock0,clock1,clock2,king,ring_cur,step,cues\n")

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_dance_howto_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT })
        return {}
    end,
    on_capture = function(_, el)
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
        fh:write(string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d;%d;%d;%d\n",
            el, (probe.read_u32(0x8007B83C) or 0) % 256,
            s32(0x801D5334), s32(0x801D5830), s32(0x801D6080), s32(0x801D5138),
            s32(0x801D5150), s32(0x801D5158),
            s32(0x801D581C), s32(0x801D5820), s32(0x801D5824),
            king_step(),
            (probe.read_u32(0x8007B758) or 0) % 16,
            probe.read_scratch_u8(0x1F800393) or 0,
            s16(0x8007B6D8), s16(0x8007B6DA), s16(0x8007B6DC), s16(0x8007B6DE)))
        fh:flush()
    end,
})
