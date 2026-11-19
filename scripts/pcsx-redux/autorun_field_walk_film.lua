-- autorun_field_walk_film.lua
--
-- The retail side of the field walk film: load a state, hold a pad schedule,
-- and write the player, the follow camera, the staging descriptor and every
-- field-ticked placement once per vsync. Its engine side is
-- LEGAIA_RC_WALK_FILM on `legaia-engine retail-compare`; scripts/ci/walk-film.py
-- pairs the two. Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_PAD_SCRIPT  `A-B:BTN` spans in vsyncs from the load.
--      LEGAIA_PAD_FILE    a timeline instead of the spans: one `vsync,0xMASK`
--                         row per change of the held word, where bit N of the
--                         mask is probe.BTN index N - the engine pad word, so
--                         a `LEGAIA_DIAG_TICK_DIGEST` run's pad replays as is.
--
-- Outputs: walk_film.csv (one row a vsync), walk_film_actors.csv (one row per
-- placement per vsync). The last three walk_film.csv columns are the frame
-- pacer's: its ring cursor (`0x8007B758`, the slot the next frame stores to),
-- the newest frame time in hblanks there (`0x80084098`, clamped to `0x2BC`)
-- and the frame step `0x1F800393` - the retail side of the field frame-cost
-- model (`legaia_engine_core::world::field_frame_cost`). After them come the
-- dialogue byte, the encounter counter and `cues`: the cue ring's four ids
-- (`0x8007B6D8`, `;`-joined, `-1` empty), whose new ids are the cues pushed.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")

local PLAYER_PTR  = 0x8007C364
local PITCH, YAW  = 0x8007B790, 0x8007B792
local H           = 0x8007B6F4
local EYE         = 0x800840B8
local FOCUS_X     = 0x80089118
local FOCUS_Z     = 0x80089120
local STAGING     = 0x801F3580
local OFFSET_EASE = 0x8007BCAC
local GAME_MODE   = 0x8007B83C
local RAND_SEED   = 0x80009010
local RING        = 0x80084098
local RING_CURSOR = 0x8007B758
local FIELD_CTL   = 0x801C6EA4 -- field-control block; +0x62 the dialogue byte
local ENC_COUNTER = 0x8007B5FC -- the region roll's step counter (FUN_801D9E1C)
local CUE_RING    = 0x8007B6D8 -- DAT_8007B6D8[4], the cue ring's ids (i16, -1 empty)

local function s16(a)
    local v = probe.read_u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = probe.read_u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%w+)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    else
        PCSX.log("[walk-film] ignoring pad item '" .. item .. "'")
    end
end

-- The timeline of LEGAIA_PAD_FILE, in vsync order, and the row in force.
local PAD_FILE = probe.getenv("LEGAIA_PAD_FILE", "")
local timeline, timeline_i, timeline_mask = {}, 1, 0
if PAD_FILE ~= "" then
    for line in io.lines(PAD_FILE) do
        local v, m = string.match(line, "^(%d+),0[xX](%x+)")
        if v then timeline[#timeline + 1] = { tonumber(v), tonumber(m, 16) } end
    end
    PCSX.log(string.format("[walk-film] %d pad rows from %s", #timeline, PAD_FILE))
end

local held = {}
local fh = assert(io.open(probe.out_path("walk_film.csv"), "w"))
fh:write("vsync,x,y,z,prev_x,prev_z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z," ..
    "st_pitch,st_yaw,st_h,st_eye_x,st_eye_y,st_eye_z,offset_ease,cursor,rate,record,mode,rng," ..
    "ring_cur,frame_cost,step,talk,enc,cues\n")

local afh = assert(io.open(probe.out_path("walk_film_actors.csv"), "w"))
afh:write("vsync,flat,x,y,z,heading,flags,model,saved,record,clip,clip_cursor,rate\n")

-- Every placement the field actor tick runs (retail_actor_facings' walk).
local function log_actors(el, player)
    local seen, seen_flat = {}, {}
    for k = 0, 8 do
        local n, count = probe.read_u32(0x8007C34C + 4 * k), 0
        while probe.in_ram(n) and count < 512 and not seen[n] do
            seen[n] = true
            count = count + 1
            if n ~= player and probe.read_u32(n + 0x0C) == 0x8003BC08 then
                local flat = probe.read_u16(n + 0x50)
                if not seen_flat[flat] then
                    seen_flat[flat] = true
                    afh:write(string.format("%d,%d,%d,%d,%d,%d,%08X,%d,%d,%d,%d,%d,%d\n", el, flat,
                        s16(n + 0x14), s16(n + 0x16), s16(n + 0x18), s16(n + 0x26),
                        probe.read_u32(n + 0x10), s16(n + 0x64),
                        -- the talk save +0x5A, the record cursor +0x9E, and
                        -- the clip: id +0x5C, cursor +0x68, rate +0x6A
                        s16(n + 0x5A), probe.read_u16(n + 0x9E),
                        s16(n + 0x5C), s16(n + 0x68), s16(n + 0x6A)))
                end
            end
            n = probe.read_u32(n)
        end
    end
    afh:flush()
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_field_walk_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT })
        return {}
    end,
    on_capture = function(_, el)
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        while timeline_i <= #timeline and timeline[timeline_i][1] <= el do
            timeline_mask = timeline[timeline_i][2]
            timeline_i = timeline_i + 1
        end
        for b = 0, 15 do
            if math.floor(timeline_mask / 2 ^ b) % 2 == 1 then want[b] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
        local p = probe.read_u32(PLAYER_PTR)
        if p == nil or not probe.in_ram(p) then return end
        fh:write(string.format(
            "%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%08x,%d,%d,%d,%d,%d,%d;%d;%d;%d\n",
            el, s16(p + 0x14), s16(p + 0x16), s16(p + 0x18),
            s16(p + 0x1C), s16(p + 0x20), s16(p + 0x26), s16(p + 0x5C),
            s16(PITCH), s16(YAW), s16(H),
            s32(EYE), s32(EYE + 4), s32(EYE + 8),
            s32(FOCUS_X), s32(FOCUS_Z),
            s16(STAGING + 0x02), s16(STAGING + 0x06), s16(STAGING + 0x26),
            s16(STAGING + 0x0E), s16(STAGING + 0x12), s16(STAGING + 0x16),
            s16(OFFSET_EASE), s16(p + 0x68), s16(p + 0x6A), s16(p + 0x5E),
            probe.read_u32(GAME_MODE) % 256, probe.read_u32(RAND_SEED),
            (probe.read_u32(RING_CURSOR) or 0) % 16,
            probe.read_u16(RING + (((probe.read_u32(RING_CURSOR) or 1) - 1) % 16) * 2) or 0,
            probe.read_scratch_u8(0x1F800393) or 0,
            probe.read_u8((probe.read_u32(FIELD_CTL) or 0) + 0x62) or 0,
            s32(ENC_COUNTER),
            s16(CUE_RING), s16(CUE_RING + 2), s16(CUE_RING + 4), s16(CUE_RING + 6)))
        fh:flush()
        log_actors(el, p)
    end,
})
