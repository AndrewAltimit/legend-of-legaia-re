-- autorun_charm_softlock_recon.lua
--
-- Slot 1 = a softlock on the Tetsu tutorial fight (shiny+charm 100% ROM). The
-- charm hook flipped the tutorial's only enemy to the party's side. Dump the
-- battle state to (a) confirm the charm flag is on the tutorial enemy and (b)
-- find a reliable "random encounter vs scripted/tutorial fight" discriminator
-- readable at battle setup, so charm can skip scripted fights.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local OUT = probe.out_path("charm_softlock_recon.txt")
local f = assert(io.open(OUT, "w"))
local function w(s) f:write(s .. "\n"); f:flush() end
local function u8(a) return probe.read_u8(a) or 0xFF end
local function u16(a) return probe.read_u16(a) or 0xFFFF end
local function u32(a) return probe.read_u32(a) or 0 end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames = 20,
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el ~= 5 then if el >= 7 then ctx.request_quit = true end return end
    -- Formation (first-monster id table) DAT_8007BD0C[0..3]
    w(string.format("formation DAT_8007BD0C = %02X %02X %02X %02X",
      u8(0x8007BD0C), u8(0x8007BD0D), u8(0x8007BD0E), u8(0x8007BD0F)))
    -- Battle context
    local ctxp = u32(0x8007BD24)
    w(string.format("battle ctx ptr 0x8007BD24 = 0x%08X", ctxp))
    if ctxp >= 0x80000000 and ctxp < 0x80200000 then
      w(string.format("  ctx+0x13 active slot=%02X  +0x287 flag=%02X  +0x28A modeCtr=%04X",
        u8(ctxp+0x13), u8(ctxp+0x287), u16(ctxp+0x28A)))
    end
    w(string.format("per-battle flags 0x8007BD60 = %02X", u8(0x8007BD60)))
    -- random-encounter marker: *(_DAT_8007c364+0x10) bit 0x80000
    local c364 = u32(0x8007C364)
    w(string.format("_DAT_8007c364 = 0x%08X", c364))
    if c364 >= 0x80000000 and c364 < 0x80200000 then
      local flagw = u32(c364 + 0x10)
      w(string.format("  *(0x8007c364+0x10) = 0x%08X  (bit0x80000 set? %s = random-encounter marker)",
        flagw, tostring(bit.band(flagw, 0x80000) ~= 0)))
    end
    -- 8 battle actors: 0x800EC9E8 + i*0x2D4; +0x16E charm flag (0x380), +0x3e, +0x21c
    for i = 0, 7 do
      local a = 0x800EC9E8 + i * 0x2D4
      local p = u32(0x801C9370 + i * 4)
      w(string.format("actor[%d]@0x%08X tblptr=0x%08X +0x16E=%04X (charm0x380=%s) +0x3e=%02X +0x21c=%02X",
        i, a, p, u16(a+0x16E), tostring(bit.band(u16(a+0x16E), 0x380) == 0x380), u8(a+0x3e), u8(a+0x21c)))
    end
    -- game-mode / SM state
    w(string.format("game-mode next 0x8007B83C = %04X", u16(0x8007B83C)))
    f:write("done\n"); f:close()
    ctx.request_quit = true
  end,
})
