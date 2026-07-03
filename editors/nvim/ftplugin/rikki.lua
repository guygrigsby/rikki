-- rikki filetype plugin: comments + rikki check on save into vim.diagnostic.
if vim.b.did_rikki_ftplugin then
  return
end
vim.b.did_rikki_ftplugin = true

vim.bo.commentstring = "// %s"

local ns = vim.api.nvim_create_namespace("rikki-check")

local function check(buf)
  local file = vim.api.nvim_buf_get_name(buf)
  if file == "" then
    return
  end
  local bin = vim.g.rikki_bin or "rikki"
  -- everything vim.diagnostic-shaped runs in vim.schedule: the vim.system
  -- callback is a fast event context where the module may not even load
  vim.system({ bin, "check", file }, { text = true }, function(out)
    vim.schedule(function()
      if not vim.api.nvim_buf_is_valid(buf) then
        return
      end
      local base = vim.fs.basename(file)
      local diags = {}
      local text = (out.stderr or "") .. "\n" .. (out.stdout or "")
      for line in text:gmatch("[^\n]+") do
        local msg = (line:gsub("^error: ", ""))
        local f, l, c, m = msg:match("^(.-):(%d+):(%d+): (.+)$")
        if f == base then
          diags[#diags + 1] = {
            lnum = tonumber(l) - 1,
            col = tonumber(c) - 1,
            message = m,
            severity = vim.diagnostic.severity.ERROR,
          }
        elseif msg ~= "" then
          -- another file (an import) or a span-less loader error: pin to
          -- the top of this buffer rather than drop it
          diags[#diags + 1] = {
            lnum = 0,
            col = 0,
            message = msg,
            severity = vim.diagnostic.severity.ERROR,
          }
        end
      end
      vim.diagnostic.set(ns, buf, diags)
    end)
  end)
end

vim.api.nvim_create_autocmd("BufWritePost", {
  group = vim.api.nvim_create_augroup("RikkiCheck" .. vim.api.nvim_get_current_buf(), {}),
  buffer = vim.api.nvim_get_current_buf(),
  callback = function(a)
    check(a.buf)
  end,
})

check(vim.api.nvim_get_current_buf())
