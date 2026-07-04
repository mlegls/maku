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

--- Stop the running pattern (card stays loaded).
function M.clear()
  M.send("(clear)")
  notify("clear")
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

-- ---------------------------------------------------------------------------
-- run: send forms as an anonymous pattern (the eval operator)

--- Strip a ; comment (string-aware) from one line.
local function strip_comment(line)
  local out, in_str = {}, false
  for i = 1, #line do
    local c = line:sub(i, i)
    if c == '"' and line:sub(i - 1, i - 1) ~= "\\" then
      in_str = not in_str
    end
    if c == ";" and not in_str then
      break
    end
    out[#out + 1] = c
  end
  return table.concat(out)
end

--- Send text (possibly multi-line) as (run ...): one wire line.
function M.run_text(text)
  local parts = {}
  for _, l in ipairs(vim.split(text, "\n", { plain = true })) do
    parts[#parts + 1] = strip_comment(l)
  end
  local one = vim.trim(table.concat(parts, " "))
  if one == "" then
    return notify("nothing to run", vim.log.levels.WARN)
  end
  write_if_modified()
  M.send("(run " .. one .. ")")
  notify("run " .. one:sub(1, 40))
end

local function text_in_range(srow, scol, erow, ecol) -- 1-indexed, inclusive
  local lines = vim.api.nvim_buf_get_lines(0, srow - 1, erow, false)
  if #lines == 0 then
    return ""
  end
  if #lines == 1 then
    lines[1] = lines[1]:sub(scol, ecol)
  else
    lines[1] = lines[1]:sub(scol)
    lines[#lines] = lines[#lines]:sub(1, ecol)
  end
  return table.concat(lines, "\n")
end

function _G.__danmaku_opfunc(motion)
  local s = vim.api.nvim_buf_get_mark(0, "[")
  local e = vim.api.nvim_buf_get_mark(0, "]")
  if motion == "char" then
    M.run_text(text_in_range(s[1], s[2] + 1, e[1], e[2] + 1))
  else
    M.run_text(text_in_range(s[1], 1, e[1], 2147483647))
  end
end

--- The operator: <localleader>e{motion} / visual <localleader>e.
function M.operator()
  vim.o.operatorfunc = "v:lua.__danmaku_opfunc"
  return "g@"
end

function M.run_visual()
  local s = vim.fn.getpos("'<")
  local e = vim.fn.getpos("'>")
  M.run_text(text_in_range(s[2], s[3], e[2], e[3]))
end

--- Innermost form enclosing the cursor (conjure's ee).
function M.run_inner_form()
  local save = vim.fn.getcurpos()
  local open = vim.fn.searchpairpos("(", "", ")", "bcnW")
  if open[1] == 0 then
    return notify("no enclosing form", vim.log.levels.WARN)
  end
  vim.fn.cursor(open[1], open[2])
  local close = vim.fn.searchpairpos("(", "", ")", "nW")
  vim.fn.setpos(".", save)
  if close[1] == 0 then
    return notify("unbalanced form", vim.log.levels.WARN)
  end
  M.run_text(text_in_range(open[1], open[2], close[1], close[2]))
end

--- Root (top-level) form around the cursor (conjure's er).
function M.run_root_form()
  local save = vim.fn.getcurpos()
  local start = vim.fn.search([[^(]], "bcnW")
  if start == 0 then
    return notify("no top-level form", vim.log.levels.WARN)
  end
  vim.fn.cursor(start, 1)
  local close = vim.fn.searchpairpos("(", "", ")", "nW")
  vim.fn.setpos(".", save)
  if close[1] == 0 then
    return notify("unbalanced form", vim.log.levels.WARN)
  end
  M.run_text(text_in_range(start, 1, close[1], close[2]))
end

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})

  -- .dmk cards: own filetype (no clojure plugins attach), clojure/edn
  -- highlighting via regex syntax + treesitter alias when available
  vim.filetype.add({ extension = { dmk = "danmaku" } })
  pcall(vim.treesitter.language.register, "clojure", "danmaku")
  vim.api.nvim_create_autocmd("FileType", {
    pattern = "danmaku",
    group = vim.api.nvim_create_augroup("danmaku-ft", { clear = true }),
    callback = function()
      vim.bo.syntax = "clojure"
      vim.bo.commentstring = ";; %s"
      vim.bo.lisp = true
    end,
  })

  vim.api.nvim_create_user_command("DanmakuPlay", M.play, { desc = "danmaku: play pattern under cursor" })
  vim.api.nvim_create_user_command("DanmakuLoad", M.load, { desc = "danmaku: load current card" })
  vim.api.nvim_create_user_command("DanmakuRestart", M.restart, { desc = "danmaku: restart pattern" })
  vim.api.nvim_create_user_command("DanmakuClear", M.clear, { desc = "danmaku: stop running pattern" })
  vim.api.nvim_create_user_command("DanmakuPause", M.toggle_pause, { desc = "danmaku: toggle pause" })
  vim.api.nvim_create_user_command("DanmakuSend", function(cmd)
    M.raw(cmd.args)
  end, { nargs = 1, desc = "danmaku: send raw EDN command" })

  -- buffer-local mappings on card files
  vim.api.nvim_create_autocmd({ "BufEnter", "BufNewFile" }, {
    pattern = "*.dmk",
    group = vim.api.nvim_create_augroup("danmaku-nvim", { clear = true }),
    callback = function(ev)
      local map = function(mode, lhs, fn, desc, opts)
        local o = vim.tbl_extend("force", { buffer = ev.buf, desc = "danmaku: " .. desc }, opts or {})
        vim.keymap.set(mode, lhs, fn, o)
      end
      -- eval operator, conjure-style
      map("n", "<localleader>e", M.operator, "run {motion}", { expr = true })
      map("x", "<localleader>e", M.run_visual, "run selection")
      map("n", "<localleader>ee", M.run_inner_form, "run innermost form")
      map("n", "<localleader>er", M.run_root_form, "run root form")
      -- fixed commands (not selection-sensitive)
      map("n", "<leader>dl", M.load, "load card (no play)")
      map("n", "<leader>dr", M.restart, "restart")
      map("n", "<leader>dc", M.clear, "clear (stop pattern)")
      map("n", "<leader>d<space>", M.toggle_pause, "toggle pause")
    end,
  })
end

return M
