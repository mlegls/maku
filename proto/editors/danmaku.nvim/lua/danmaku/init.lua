-- danmaku.nvim — thin client for the danmaku-player server.
--
-- The player (proto/player) listens on 127.0.0.1:7777 for newline-delimited
-- EDN commands; the wire format is the card format, so this plugin is just
-- "send a form to a socket" (the sclang/scsynth editor split).
--
-- Commands sent:
--   (load "path")            reload card from disk
--   (load "path" "pattern")  ... selecting a pattern
--   (restart) (pause) (resume)

local M = {}

M.config = {
  host = "127.0.0.1",
  port = 7777,
}

local function notify(msg, level)
  vim.schedule(function()
    vim.notify("danmaku: " .. msg, level or vim.log.levels.INFO)
  end)
end

--- Send one line to the player server (fire and forget).
function M.send(line)
  local uv = vim.uv or vim.loop
  local client = uv.new_tcp()
  client:connect(M.config.host, M.config.port, function(err)
    if err then
      notify("cannot reach player on :" .. M.config.port .. " (" .. err .. ") — is it running?",
        vim.log.levels.ERROR)
      client:close()
      return
    end
    client:write(line .. "\n")
    client:shutdown(function()
      client:close()
    end)
  end)
end

local function write_if_modified()
  if vim.bo.modified then
    vim.cmd.write()
  end
end

local function card_path()
  return vim.fn.expand("%:p")
end

--- Name of the defpattern enclosing (or nearest above, else next below) the cursor.
local function pattern_near_cursor()
  local ln = vim.fn.search([[^(defpattern\s\+\S\+]], "bcnW")
  if ln == 0 then
    ln = vim.fn.search([[^(defpattern\s\+\S\+]], "cnW")
  end
  if ln == 0 then
    return nil
  end
  return vim.fn.getline(ln):match("^%(defpattern%s+([^%s%)]+)")
end

--- Load the current file into the player (first pattern / server default).
function M.load()
  write_if_modified()
  M.send(string.format('(load "%s")', card_path()))
  notify("load " .. vim.fn.expand("%:t"))
end

--- Load the current file and play the pattern under the cursor.
function M.play()
  write_if_modified()
  local pat = pattern_near_cursor()
  if pat then
    M.send(string.format('(load "%s" "%s")', card_path(), pat))
    notify("play " .. pat)
  else
    M.load()
  end
end

function M.restart()
  M.send("(restart)")
end

local paused = false
function M.toggle_pause()
  paused = not paused
  M.send(paused and "(pause)" or "(resume)")
  notify(paused and "paused" or "resumed")
end

--- Send an arbitrary command string.
function M.raw(text)
  M.send(text)
end

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})

  vim.api.nvim_create_user_command("DanmakuPlay", M.play, { desc = "danmaku: play pattern under cursor" })
  vim.api.nvim_create_user_command("DanmakuLoad", M.load, { desc = "danmaku: load current card" })
  vim.api.nvim_create_user_command("DanmakuRestart", M.restart, { desc = "danmaku: restart pattern" })
  vim.api.nvim_create_user_command("DanmakuPause", M.toggle_pause, { desc = "danmaku: toggle pause" })
  vim.api.nvim_create_user_command("DanmakuSend", function(cmd)
    M.raw(cmd.args)
  end, { nargs = 1, desc = "danmaku: send raw EDN command" })

  -- buffer-local mappings on card files
  vim.api.nvim_create_autocmd({ "BufEnter", "BufNewFile" }, {
    pattern = "*.edn",
    group = vim.api.nvim_create_augroup("danmaku-nvim", { clear = true }),
    callback = function(ev)
      local map = function(lhs, fn, desc)
        vim.keymap.set("n", lhs, fn, { buffer = ev.buf, desc = "danmaku: " .. desc })
      end
      map("<leader>dp", M.play, "play pattern under cursor")
      map("<leader>dl", M.load, "load card")
      map("<leader>dr", M.restart, "restart")
      map("<leader>d<space>", M.toggle_pause, "toggle pause")
    end,
  })
end

return M
