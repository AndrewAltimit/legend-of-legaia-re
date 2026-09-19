-- autorun_view_build_attribution.lua
--
-- Which of a field frame's view builds does the drawn geometry use?
--
-- `FUN_800172C0` turns the camera globals into the GTE's `R` and `TR`, and a
-- field frame runs it several times - from `0x801D0F90` and `0x801D1854` in the
-- field overlay and `0x80016670` in SCUS (return addresses `0x801D0F98` /
-- `0x801D185C` / `0x80016678`), plus, when the field-render module is resident,
-- two more sites inside the slot-B window. On an ordinary frame that is
-- harmless because the globals do not move between them; on a scene-entry frame
-- they can, and then "the frame's view matrix" is not one object.
--
-- The builds all write the SAME GTE control registers, so a prim is projected
-- against whichever build last preceded its emission. The probe therefore taps
-- four things: the builder's entry (caller `ra` plus the camera words it is
-- about to read), `0x80017348` - one instruction after `FUN_8003D1A4` has
-- uploaded `TR` - for the `TR` that build leaves, the TMD renderer
-- `FUN_8002735C`, and the universal OT link helper `FUN_8003D2C4`, so every
-- linked prim is counted against the build whose matrix was live. Ranking those
-- counts is what separates "ran last" from "the frame drew under it".
--
-- Caveats the counts carry: prims linked before a vsync's first build inherit
-- the previous vsync's matrix and are bucketed separately.
--
-- The link helper itself says nothing about what it is linking, but its `$a1`
-- is the prim, and a linked PsyQ prim is `[OT tag][GPU word 0]...` - so the
-- byte at `a1 + 7` is the GPU command code. That splits the count the way the
-- question needs: `0x20..0x3F` are polygons (the TMD renderer's output, i.e.
-- the 3D scene), `0x40..0x5F` lines, `0x60..0x7F` rects and sprites (the 2D
-- UI layer), `0xE0..0xE6` attribute packets. A build that only ever precedes
-- sprite and rect links never framed any geometry, whatever its raw share of
-- the vsync's links.
--
-- Env vars:
--   LEGAIA_SSTATE       save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES       capture vsyncs (default 1800)
--   LEGAIA_HOLD_BTN     direction held to drive a scene entry (default UP)
--   LEGAIA_HOLD_START   first vsync of the first hold (default 60)
--   LEGAIA_HOLD_LEN     vsyncs held per attempt (default 90)
--   LEGAIA_HOLD_PERIOD  vsyncs between attempts (default 300)
--   LEGAIA_DRAW_BUDGET  renderer rows recorded per vsync (default 2)
--   LEGAIA_OUT_DIR      output directory
--
-- Outputs: view_build_attribution.csv, view_build_attribution.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local HOLD_BTN    = probe.getenv("LEGAIA_HOLD_BTN", "UP")
local HOLD_START  = probe.getenv_num("LEGAIA_HOLD_START", 60)
local HOLD_LEN    = probe.getenv_num("LEGAIA_HOLD_LEN", 90)
local HOLD_PERIOD = probe.getenv_num("LEGAIA_HOLD_PERIOD", 300)
local DRAW_BUDGET = probe.getenv_num("LEGAIA_DRAW_BUDGET", 2)

local OUT_CSV = probe.out_path("view_build_attribution.csv")
local OUT_LOG = probe.out_path("view_build_attribution.log")

local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local EYE_X, EYE_Y, EYE_Z = 0x800840B8, 0x800840BC, 0x800840C0
local PITCH, YAW, ROLL    = 0x8007B790, 0x8007B792, 0x8007B794

local VIEW_IN  = 0x800172C0   -- builder entry
local VIEW_TR  = 0x80017348   -- one instruction after TR is uploaded
local TMD_DRAW = 0x8002735C   -- TMD renderer entry (60 GTE ops)
local LINK_PRIM = 0x8003D2C4  -- the universal OT link helper; one call per prim

local TAPS = {
    { addr = VIEW_IN,   want = 0x27BDFFD8, kind = "view_in",   name = "FUN_800172C0 entry" },
    { addr = VIEW_TR,   want = 0x27A40018, kind = "view_tr",   name = "TR uploaded" },
    { addr = TMD_DRAW,  want = 0x27BDFEA8, kind = "tmd_draw",  name = "FUN_8002735C entry" },
    { addr = LINK_PRIM, want = 0x3C0800FF, kind = "link_prim", name = "FUN_8003D2C4 linkPrim" },
}

-- The three known call sites, by return address. Keys are the SIGNED 32-bit
-- form LuaJIT's bit.band produces, so a lookup on a live `ra` matches.
local SITES = {
    [0x801D0F98 - 0x100000000] = "A_801D0F90",
    [0x801D185C - 0x100000000] = "B_801D1854",
    [0x80016678 - 0x100000000] = "C_80016670",
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[viewattr] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end
local function s32(v)
    v = n32(v)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
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

local function tr()
    local r = PCSX.getRegisters()
    return s32(r.CP2C.r[5]), s32(r.CP2C.r[6]), s32(r.CP2C.r[7])
end

local csv
local g_elapsed = 0
local seq = 0
local draws_this_frame = 0
local frame_builds = 0
local last_scene = nil
local entry_count = 0
local hold_on = false
-- Per-vsync bookkeeping: the site of the last build, and whether a draw has
-- already happened this vsync before this build (an out-of-order frame).
local last_build_site = ""
local n_build, n_draw, n_prim = 0, 0, 0
local prims_after = {}
-- Same buckets, split by the linked prim's GPU command class.
local prims_after_kind = {}
local prim_class_totals = {}
local site_counts = {}
local draw_after = {}       -- site -> draws attributed to it
local draws_before_any_build = 0
local mismatch_rows = 0

local function row(kind, ra, note)
    seq = seq + 1
    local trx, try, trz = tr()
    csv:row("%d,%d,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,0x%02X,%s",
        seq, g_elapsed, kind, "0x" .. hex8(ra),
        SITES[n32(ra)] or "",
        s32(probe.read_u32(EYE_X) or 0),
        s32(probe.read_u32(EYE_Y) or 0),
        s32(probe.read_u32(EYE_Z) or 0),
        s16(probe.read_u16(PITCH) or 0),
        s16(probe.read_u16(YAW) or 0),
        s16(probe.read_u16(ROLL) or 0),
        trx, try, trz,
        frame_builds,
        scene_name(), probe.read_u8(GAME_MODE) or 0,
        note or "")
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "seq,vsync,event,ra,site,eye_x,eye_y,eye_z,pitch,yaw,roll," ..
            "tr_x,tr_y,tr_z,builds_this_vsync,scene,mode,note")
        probe.env.write_manifest("autorun_view_build_attribution.lua", {
            sstate = SSTATE, frames = FRAMES, hold_btn = HOLD_BTN,
            hold_start = HOLD_START, hold_len = HOLD_LEN,
            hold_period = HOLD_PERIOD, draw_budget = DRAW_BUDGET,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local r = PCSX.getRegisters()
                local ra = n32(r.GPR.n.ra)
                if t.kind == "view_in" then
                    frame_builds = frame_builds + 1
                    n_build = n_build + 1
                    local s = SITES[ra] or ("unknown_0x" .. hex8(ra))
                    site_counts[s] = (site_counts[s] or 0) + 1
                    last_build_site = s
                    row("view_in", ra)
                elseif t.kind == "view_tr" then
                    row("view_tr", ra, last_build_site)
                elseif t.kind == "link_prim" then
                    -- Which build's matrix was live when this prim was linked.
                    local slot = last_build_site
                    if slot == "" then slot = "before_any_build" end
                    prims_after[slot] = (prims_after[slot] or 0) + 1
                    n_prim = n_prim + 1
                    -- ...and WHAT was linked: `$a1` is the prim, so `a1 + 7` is
                    -- the GPU command code of its first packet word.
                    local prim = n32(r.GPR.n.a1)
                    local code = probe.read_u8(prim + 7) or 0
                    local cls
                    if code >= 0x20 and code <= 0x3F then cls = "poly3d"
                    elseif code >= 0x40 and code <= 0x5F then cls = "line"
                    elseif code >= 0x60 and code <= 0x7F then cls = "rect2d"
                    elseif code >= 0xE0 and code <= 0xE6 then cls = "attr"
                    else cls = string.format("other_%02X", code) end
                    local key = slot .. "/" .. cls
                    prims_after_kind[key] = (prims_after_kind[key] or 0) + 1
                    prim_class_totals[cls] = (prim_class_totals[cls] or 0) + 1
                else
                    n_draw = n_draw + 1
                    if last_build_site == "" then
                        draws_before_any_build = draws_before_any_build + 1
                    else
                        draw_after[last_build_site] =
                            (draw_after[last_build_site] or 0) + 1
                    end
                    draws_this_frame = draws_this_frame + 1
                    if draws_this_frame <= DRAW_BUDGET then
                        row("tmd_draw", ra, last_build_site)
                    end
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            for _, t in ipairs(TAPS) do
                local got = probe.read_u32(t.addr) or 0
                logf("fingerprint [0x%08X] = 0x%s (want 0x%s, %s) %s",
                     t.addr, hex8(got), hex8(t.want), t.name,
                     got == t.want and "OK" or "MISMATCH")
            end
            logf("start scene=%s mode=0x%02X", scene_name(),
                 probe.read_u8(GAME_MODE) or 0)
        end

        local sc = scene_name()
        if sc ~= last_scene then
            entry_count = entry_count + 1
            logf("scene change #%d at vsync %d: %s -> %s (mode 0x%02X)",
                 entry_count, elapsed, tostring(last_scene), sc,
                 probe.read_u8(GAME_MODE) or 0)
            last_scene = sc
        end

        -- Per-vsync reset AFTER the frame's rows have been written.
        draws_this_frame = 0
        frame_builds = 0
        last_build_site = ""

        -- Hold a direction in bursts so a run gets several chances at a door.
        local btn = probe.BTN[HOLD_BTN]
        if btn then
            local phase = elapsed - HOLD_START
            if phase >= 0 and (phase % HOLD_PERIOD) < HOLD_LEN then
                probe.pad_force(btn)
                hold_on = true
            elseif hold_on then
                probe.pad_release(btn)
                hold_on = false
            end
        end
    end,

    on_summary = function()
        if probe.BTN[HOLD_BTN] then probe.pad_release(probe.BTN[HOLD_BTN]) end
        local sc = {}
        for s, n in pairs(site_counts) do
            sc[#sc + 1] = string.format("%s=%d", s, n)
        end
        table.sort(sc)
        logf("builds=%d by site: %s", n_build, table.concat(sc, " "))
        local da = {}
        for s, n in pairs(draw_after) do
            da[#da + 1] = string.format("%s=%d", s, n)
        end
        table.sort(da)
        logf("TMD draws=%d attributed to the preceding build: %s "
             .. "(before any build this vsync: %d)",
             n_draw, table.concat(da, " "), draws_before_any_build)
        local pa = {}
        for s2, n in pairs(prims_after) do
            pa[#pa + 1] = string.format("%s=%d", s2, n)
        end
        table.sort(pa)
        logf("prims linked=%d, by the build whose matrix was live: %s",
             n_prim, table.concat(pa, " "))
        local pk = {}
        for k, n in pairs(prims_after_kind) do
            pk[#pk + 1] = string.format("%s=%d", k, n)
        end
        table.sort(pk)
        logf("prims by build x GPU class: %s", table.concat(pk, " "))
        local pc = {}
        for k, n in pairs(prim_class_totals) do
            pc[#pc + 1] = string.format("%s=%d", k, n)
        end
        table.sort(pc)
        logf("prim class totals: %s", table.concat(pc, " "))
        logf("scene changes seen: %d", entry_count)
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
