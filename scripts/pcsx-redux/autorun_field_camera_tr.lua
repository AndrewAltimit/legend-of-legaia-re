-- autorun_field_camera_tr.lua
--
-- What forms GTE `TR` (CP2C 5/6/7) from the field follow camera's live words?
--
-- The camera chain is record -> parameter block -> compose -> ease
-- (docs/formats/encounter.md#from-the-block-to-the-live-camera-compose-ease-snap):
-- `FUN_801DAB90` writes an eye-space translation trio into the staging
-- descriptor at `0x801F3580` (`+0x0E` / `+0x12` / `+0x16`, halfwords) and the
-- ease `FUN_801DB510` walks the live words `0x800840B8` / `BC` / `C0` toward
-- it. What nothing had measured is the step after that: which routine reads
-- those live words and what arithmetic it applies before `ctc2` puts them in
-- `TR`.
--
-- This probe pins the whole hand-off on ONE frame. On each sampled frame it
-- snapshots, in order:
--
--   (a) the return of the ease `FUN_801DB510` (`0x801DB8E4`, its single exit):
--       staging descriptor, live trio, pitch / yaw / H, player (x, y, z),
--       the camera parameter block bytes, and CP2C 0..31 as the ease left it;
--   (b) every `ctc2` site in `SCUS_942.54` that writes CP2C 5/6/7 - a census
--       group of seventeen, armed only inside the sample window - so the FIRST
--       TR write after the ease is a measurement rather than a guess;
--   (c) the named view-build taps inside `FUN_800172C0`, the field view-matrix
--       builder, read straight after each of its GTE uploads returns.
--
-- Everything in one CSV row per event, keyed by sample id, so a reader can put
-- the composed trio, the live trio and `TR` side by side on a single frame.
--
-- Pad plan: the ease only steps on frames the player's (X, footing, Z) changed,
-- so a state parked at rest never shows a glide. Each walk leg holds one
-- direction for LEGAIA_WALK_ON vsyncs and releases for LEGAIA_WALK_OFF, and the
-- default LEGAIA_WALK_BTN=ANY rotates the four directions leg by leg so a wall
-- on one side cannot read as "the pad does nothing". LEGAIA_WALK_BTN=NONE
-- leaves the pad alone (use it on scripted-shot states).
--
-- Env vars:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 900)
--   LEGAIA_SAMPLE_EVERY  vsyncs between samples (default 60)
--   LEGAIA_SAMPLES     max samples (default 12)
--   LEGAIA_TR_BUDGET   census TR writes recorded per sample (default 10)
--   LEGAIA_WALK_BTN    ANY (cycle all four) / UP / DOWN / LEFT / RIGHT /
--                      NONE (leave the pad alone); default ANY
--   LEGAIA_WALK_ON     vsyncs held (default 45)
--   LEGAIA_WALK_OFF    vsyncs released (default 45)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: field_camera_tr.csv, field_camera_tr.log, field_camera_tr.detail.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE       = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES       = probe.getenv_num("LEGAIA_FRAMES", 900)
local SAMPLE_EVERY = probe.getenv_num("LEGAIA_SAMPLE_EVERY", 60)
local MAX_SAMPLES  = probe.getenv_num("LEGAIA_SAMPLES", 12)
local TR_BUDGET    = probe.getenv_num("LEGAIA_TR_BUDGET", 10)
local WALK_BTN     = probe.getenv("LEGAIA_WALK_BTN", "ANY")
local WALK_ON      = probe.getenv_num("LEGAIA_WALK_ON", 45)
local WALK_OFF     = probe.getenv_num("LEGAIA_WALK_OFF", 45)

local OUT_CSV    = probe.out_path("field_camera_tr.csv")
local OUT_LOG    = probe.out_path("field_camera_tr.log")
local OUT_DETAIL = probe.out_path("field_camera_tr.detail.txt")

-- ---------------------------------------------------------------- addresses
local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local PLAYER_PTR = 0x8007C364

local EYE_X, EYE_Y, EYE_Z = 0x800840B8, 0x800840BC, 0x800840C0
local PITCH, YAW, ROLL    = 0x8007B790, 0x8007B792, 0x8007B794
local GTE_H               = 0x8007B6F4
local CAM_ENABLE          = 0x8007B606
local CAM_BLOCK           = 0x8007B607          -- .. 0x8007B627
local FOCUS_X, FOCUS_Y, FOCUS_Z = 0x80089118, 0x8008911C, 0x80089120
local PREMUL_MAT          = 0x8007BF10          -- pre-multiplied into the view R
local STAGING             = 0x801F3580          -- field overlay
local VIEW_MAT            = 0x1F8003C8          -- scratchpad MATRIX the view uses

-- The ease's single exit (`jr ra` at 0x801DB8E4 in the field overlay). Overlays
-- alias this VA, so the probe fingerprints the bytes before trusting it.
local EASE_ENTRY = 0x801DB510
local EASE_RET   = 0x801DB8E4

-- Named taps inside FUN_800172C0, the field view-matrix builder. Each tap is
-- the first instruction executed AFTER the named upload returns, so CP2C read
-- there is the state that upload left behind. FUN_80026F50 is the same shape
-- for the other game modes and is armed as a control: a field run where it also
-- fires would mean the frame being read is not the field one.
local VIEW_ENTRY = 0x800172C0
local VIEW_TAPS  = {
    { addr = 0x800172F4, name = "after RotMatrix(0x8007B780+0x800) -> 0x1F8003A8" },
    { addr = 0x80017348, name = "after 0x8003D1A4(0x1F8003C8): TR = eye trio verbatim" },
    { addr = 0x80017378, name = "after 0x8003D344 MVMVA: MAC = eye + (R*focus)>>12" },
    { addr = 0x80017384, name = "after 0x8005B6A8(0x1F8003C8): final TR" },
    { addr = 0x80026F50, name = "control: FUN_80026F50 (non-field view builder)" },
}

-- Every `ctc2 rX, CP2C{5,6,7}` triple in SCUS_942.54, addressed at the FIRST
-- ctc2 of the triple with the GPR indices the three writes take their values
-- from. Derived by scanning the image for `(w & 0xFFE007FF) == 0x48C00000`.
local TR_SITES = {
    { addr = 0x8001B3FC, rt = { 12, 13, 14 } },
    { addr = 0x8001BC0C, rt = { 12, 13, 14 } },
    { addr = 0x8001BE10, rt = { 12, 13, 14 } },
    { addr = 0x8001C0B4, rt = { 12, 13, 14 } },
    { addr = 0x8003D190, rt = { 0, 0, 0 } },
    { addr = 0x8003D1D8, rt = { 13, 14, 15 } },
    { addr = 0x8003D1F8, rt = { 8, 9, 10 } },
    { addr = 0x800461B0, rt = { 0, 0, 0 } },
    { addr = 0x80046284, rt = { 5, 6, 7 } },
    { addr = 0x800462A8, rt = { 0, 0, 0 } },
    { addr = 0x80046374, rt = { 5, 6, 7 } },
    { addr = 0x80046398, rt = { 0, 0, 0 } },
    { addr = 0x8004647C, rt = { 5, 6, 7 } },
    { addr = 0x80048BD8, rt = { 13, 14, 15 } },
    { addr = 0x80048E50, rt = { 13, 14, 15 } },
    { addr = 0x800490E0, rt = { 13, 14, 15 } },
    { addr = 0x80049134, rt = { 13, 14, 15 } },
    { addr = 0x80049308, rt = { 13, 14, 15 } },
    { addr = 0x8005B394, rt = { 8, 9, 10 } },
    { addr = 0x8005B6B4, rt = { 8, 9, 10 } },
}

-- ------------------------------------------------------------------ helpers
local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[cam_tr] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end

-- bit.band on a value >= 2^31 yields a NEGATIVE Lua number under LuaJIT, and
-- string.format("%08X", -1) then prints sixteen F's. bit.tohex is the only
-- form that stays eight digits wide for the whole u32 range.
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function s32(v)
    v = n32(v)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local function ru16s(addr) return s16(probe.read_u16(addr) or 0) end
local function ru32s(addr) return s32(probe.read_u32(addr) or 0) end

local function cp2c()
    local r = PCSX.getRegisters()
    local out = {}
    for i = 0, 31 do out[i] = n32(r.CP2C.r[i]) end
    return out
end

local function gpr()
    local r = PCSX.getRegisters()
    local out = {}
    for i = 0, 33 do out[i] = n32(r.GPR.r[i]) end
    return out
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function player_xyz()
    local p = probe.read_u32(PLAYER_PTR)
    if p == nil or p < 0x80000000 or p >= 0x80200000 then return 0, 0, 0, 0 end
    return ru16s(p + 0x14), ru16s(p + 0x16), ru16s(p + 0x18), n32(p)
end

-- ----------------------------------------------------------------- csv shape
local CSV_HEADER = table.concat({
    "sample", "vsync", "event", "pc", "ra", "a0",
    "tr_x", "tr_y", "tr_z",
    "live_ex", "live_ey", "live_ez",
    "stg_ex", "stg_ey", "stg_ez",
    "pitch", "yaw", "roll", "h",
    "stg_pitch", "stg_yaw", "stg_roll", "stg_h",
    "stg_fx", "stg_fy", "stg_fz",
    "focus_x", "focus_y", "focus_z",
    "focus_x16", "focus_y16", "focus_z16",
    "player_x", "player_y", "player_z",
    "cam_enable", "cam_mode", "cam_shift",
    "scene", "mode",
    "cp2c0", "cp2c1", "cp2c2", "cp2c3", "cp2c4",
}, ",")

local csv = nil
local g_elapsed = 0
local sample_id = 0
local want_sample = false
local in_sample = false
local tr_budget = 0
local census_bps = {}
local census_on = false
local ease_hits, view_hits, tr_hits = 0, 0, 0
local ease_free_samples = 0
local open_sample = nil
local site_hits = {}
local first_tr_after_ease = {}
local moved_frames, still_frames = 0, 0
local last_px, last_pz = nil, nil

local function detail(s)
    local fh = io.open(OUT_DETAIL, "a")
    if fh == nil then return end
    fh:write(s)
    if s:sub(-1) ~= "\n" then fh:write("\n") end
    fh:close()
end

local function census_enable(on)
    if census_on == on then return end
    census_on = on
    for _, b in ipairs(census_bps) do
        pcall(function() if on then b:enable() else b:disable() end end)
    end
end

-- One CSV row. `ev` picks the event label; `extra` carries the per-event
-- pc / ra / a0 / TR fields (all optional).
local function row(ev, extra)
    extra = extra or {}
    local px, py, pz = player_xyz()
    local c = extra.cp2c or cp2c()
    local trx = extra.tr_x or s32(c[5])
    local try = extra.tr_y or s32(c[6])
    local trz = extra.tr_z or s32(c[7])
    csv:row(table.concat({
        "%d", "%d", "%s", "%s", "%s", "%s",
        "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "%d", "%d", "%d",
        "%d", "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "%d", "%d",
        "%d", "0x%02X", "0x%02X",
        "%s", "0x%02X",
        "%s", "%s", "%s", "%s", "%s",
    }, ","),
        sample_id, g_elapsed, ev,
        extra.pc and ("0x" .. hex8(extra.pc)) or "",
        extra.ra and ("0x" .. hex8(extra.ra)) or "",
        extra.a0 and ("0x" .. hex8(extra.a0)) or "",
        trx, try, trz,
        ru32s(EYE_X), ru32s(EYE_Y), ru32s(EYE_Z),
        ru16s(STAGING + 0x0E), ru16s(STAGING + 0x12), ru16s(STAGING + 0x16),
        ru16s(PITCH), ru16s(YAW), ru16s(ROLL), ru16s(GTE_H),
        ru16s(STAGING + 0x02), ru16s(STAGING + 0x06), ru16s(STAGING + 0x0A),
        ru16s(STAGING + 0x26),
        ru16s(STAGING + 0x1A), ru16s(STAGING + 0x1E), ru16s(STAGING + 0x22),
        ru32s(FOCUS_X), ru32s(FOCUS_Y), ru32s(FOCUS_Z),
        ru16s(FOCUS_X), ru16s(FOCUS_Y), ru16s(FOCUS_Z),
        px, py, pz,
        probe.read_u8(CAM_ENABLE) or 0,
        probe.read_u8(CAM_BLOCK) or 0,
        probe.read_u8(CAM_BLOCK + 0x04) or 0,
        scene_name(), probe.read_u8(GAME_MODE) or 0,
        hex8(c[0]), hex8(c[1]), hex8(c[2]), hex8(c[3]), hex8(c[4]))
end

local BTNS = {
    UP = probe.BTN.UP, DOWN = probe.BTN.DOWN,
    LEFT = probe.BTN.LEFT, RIGHT = probe.BTN.RIGHT,
}
local walk_dirs = nil
if WALK_BTN == "ANY" then
    walk_dirs = { probe.BTN.UP, probe.BTN.RIGHT, probe.BTN.DOWN, probe.BTN.LEFT }
elseif BTNS[WALK_BTN] then
    walk_dirs = { BTNS[WALK_BTN] }
end
local walk_held = nil

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV, CSV_HEADER)
        probe.write_manifest("autorun_field_camera_tr.lua", {
            sstate = SSTATE, frames = FRAMES, sample_every = SAMPLE_EVERY,
            samples = MAX_SAMPLES, tr_budget = TR_BUDGET,
            walk_btn = WALK_BTN, walk_on = WALK_ON, walk_off = WALK_OFF,
        })
        local descs = {}

        -- (a) ease return. On a scripted-shot scene the ease never runs at all
        -- (a gate ahead of it returns before the call), so the sample can also
        -- be opened at the view builder - `open_sample` is shared.
        open_sample = function(ev, pc)
            want_sample = false
            in_sample = true
            sample_id = sample_id + 1
            tr_budget = TR_BUDGET
            first_tr_after_ease[sample_id] = nil
            local r = PCSX.getRegisters()
            row(ev, { pc = pc, ra = n32(r.GPR.n.ra) })
            local c = cp2c()
            local parts = {}
            for i = 0, 31 do parts[#parts + 1] = hex8(c[i]) end
            local blk = {}
            for i = 0, 0x20 do
                blk[#blk + 1] = string.format("%02X", probe.read_u8(CAM_BLOCK + i) or 0)
            end
            local stg = {}
            for i = 0, 0x27 do
                stg[#stg + 1] = string.format("%02X", probe.read_u8(STAGING + i) or 0)
            end
            local pre = {}
            for i = 0, 7 do
                pre[#pre + 1] = hex8(probe.read_u32(PREMUL_MAT + i * 4) or 0)
            end
            local vm = {}
            for i = 0, 7 do
                vm[#vm + 1] = hex8(probe.read_scratch_u32(VIEW_MAT + i * 4) or 0)
            end
            detail(string.format(
                "sample %d vsync %d scene=%s mode=0x%02X\n" ..
                "  CP2C   = %s\n  block  = %s\n  stage  = %s\n" ..
                "  premul(0x8007BF10) = %s\n  viewmat(0x1F8003C8) = %s",
                sample_id, g_elapsed, scene_name(),
                probe.read_u8(GAME_MODE) or 0,
                table.concat(parts, " "), table.concat(blk, ""),
                table.concat(stg, ""),
                table.concat(pre, " "), table.concat(vm, " ")))
            census_enable(true)
        end

        local d_ease = { addr = EASE_RET, hits_ref = { n = 0 },
                         name = "FUN_801DB510 return" }
        probe.arm_breakpoint(EASE_RET, "Exec", 4, "ease_ret", function()
            ease_hits = ease_hits + 1
            d_ease.hits_ref.n = ease_hits
            if want_sample then open_sample("ease_ret", EASE_RET) end
        end)
        descs[#descs + 1] = d_ease

        -- (b) TR census, armed only inside a sample window.
        for _, site in ipairs(TR_SITES) do
            local s = site
            site_hits[s.addr] = 0
            local d = { addr = s.addr, hits_ref = { n = 0 },
                        name = string.format("ctc2 TR @ 0x%08X", s.addr) }
            local b = probe.arm_breakpoint(s.addr, "Exec", 4,
                string.format("tr_%08X", s.addr), function()
                if tr_budget <= 0 then return end
                tr_budget = tr_budget - 1
                tr_hits = tr_hits + 1
                site_hits[s.addr] = site_hits[s.addr] + 1
                d.hits_ref.n = site_hits[s.addr]
                local g = gpr()
                local r = PCSX.getRegisters()
                if first_tr_after_ease[sample_id] == nil then
                    first_tr_after_ease[sample_id] = s.addr
                end
                row("tr_write", {
                    pc = s.addr, ra = n32(r.GPR.n.ra), a0 = g[4],
                    tr_x = s32(g[s.rt[1]]), tr_y = s32(g[s.rt[2]]),
                    tr_z = s32(g[s.rt[3]]),
                })
                if tr_budget <= 0 then census_enable(false) end
            end)
            pcall(function() b:disable() end)
            census_bps[#census_bps + 1] = b
            descs[#descs + 1] = d
        end
        census_on = false

        -- (c) named view-build taps.
        local d_view = { addr = VIEW_ENTRY, hits_ref = { n = 0 },
                         name = "FUN_80026F50 entry" }
        probe.arm_breakpoint(VIEW_ENTRY, "Exec", 4, "view_entry", function()
            view_hits = view_hits + 1
            d_view.hits_ref.n = view_hits
            if want_sample then
                ease_free_samples = ease_free_samples + 1
                open_sample("view_entry_nosease", VIEW_ENTRY)
                return
            end
            if not in_sample then return end
            local r = PCSX.getRegisters()
            row("view_entry", { pc = VIEW_ENTRY, ra = n32(r.GPR.n.ra) })
        end)
        descs[#descs + 1] = d_view

        for _, tap in ipairs(VIEW_TAPS) do
            local t = tap
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            probe.arm_breakpoint(t.addr, "Exec", 4,
                string.format("view_%08X", t.addr), function()
                d.hits_ref.n = d.hits_ref.n + 1
                if not in_sample then return end
                local r = PCSX.getRegisters()
                row("view_tap", { pc = t.addr, ra = n32(r.GPR.n.ra) })
            end)
            descs[#descs + 1] = d
        end

        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed

        if elapsed == 2 then
            -- Fingerprint the aliasing VAs before any conclusion rests on them.
            local w0 = probe.read_u32(EASE_ENTRY) or 0
            local w1 = probe.read_u32(EASE_RET) or 0
            local w2 = probe.read_u32(VIEW_ENTRY) or 0
            logf("overlay fingerprint: [0x%08X]=0x%s (want 27BDFFE0) " ..
                 "[0x%08X]=0x%s (want 03E00008) [0x%08X]=0x%s (want 27BDFFD8)",
                 EASE_ENTRY, hex8(w0), EASE_RET, hex8(w1), VIEW_ENTRY, hex8(w2))
            logf("scene=%s mode=0x%02X cam_enable=%d",
                 scene_name(), probe.read_u8(GAME_MODE) or 0,
                 probe.read_u8(CAM_ENABLE) or 0)
        end

        -- Close the sample window one vsync after it opened.
        if in_sample then
            in_sample = false
            tr_budget = 0
            census_enable(false)
        end

        local px, _, pz = player_xyz()
        if last_px ~= nil and (px ~= last_px or pz ~= last_pz) then
            moved_frames = moved_frames + 1
        elseif last_px ~= nil then
            still_frames = still_frames + 1
        end
        last_px, last_pz = px, pz

        -- Walk legs. A single held direction can be blocked by a wall for the
        -- whole run and read as "the pad does nothing", so the default cycles
        -- all four: with LEGAIA_WALK_BTN=ANY each on-leg picks the next
        -- direction, and the off-leg lets the ease settle.
        if walk_dirs ~= nil then
            local period = WALK_ON + WALK_OFF
            local phase = period > 0 and (elapsed % period) or 0
            local leg = period > 0 and math.floor(elapsed / period) or 0
            local want = (phase < WALK_ON)
                and walk_dirs[(leg % #walk_dirs) + 1] or nil
            if want ~= walk_held then
                if walk_held then probe.pad_release(walk_held) end
                if want then probe.pad_force(want) end
                walk_held = want
            end
        end

        if sample_id < MAX_SAMPLES and SAMPLE_EVERY > 0
            and elapsed > 10 and (elapsed % SAMPLE_EVERY) == 0 then
            want_sample = true
        end
    end,

    on_summary = function(ctx, descs)
        if walk_held then probe.pad_release(walk_held); walk_held = nil end
        logf("samples=%d (%d opened at the view builder because the ease never ran) " ..
             "ease_hits=%d view_hits=%d tr_census_hits=%d",
             sample_id, ease_free_samples, ease_hits, view_hits, tr_hits)
        logf("player moved on %d of %d sampled vsyncs (still %d)",
             moved_frames, moved_frames + still_frames, still_frames)
        for _, s in ipairs(TR_SITES) do
            if site_hits[s.addr] > 0 then
                logf("  TR site 0x%08X : %d recorded writes", s.addr, site_hits[s.addr])
            end
        end
        local firsts = {}
        for i = 1, sample_id do
            local a = first_tr_after_ease[i]
            firsts[#firsts + 1] = a and string.format("0x%08X", a) or "none"
        end
        logf("first TR write after each ease return: %s", table.concat(firsts, " "))
        local fh = io.open(OUT_LOG, "w")
        if fh then
            fh:write(table.concat(lines, "\n"))
            fh:write("\n")
            fh:close()
        end
        if csv then csv:close() end
    end,
})
