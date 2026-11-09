-- Original Apache-2.0 code. Optional Neovim 0.11.5 definition-only adapter.
-- Never downloads software, writes sources, imports builds, or starts Java.
local M = {}
local api, uv = vim.api, vim.uv
local configured, current
local session_serial = 0

local function warn(message)
  vim.schedule(function() vim.notify('java-analyzer: ' .. message, vim.log.levels.WARN) end)
end

local function finish(request, status, result, err)
  if request.finished then return end
  request.finished = true
  if request.id then request.session.pending[request.id] = nil end
  if request.on_result then
    local ok, failure = pcall(request.on_result, { status = status, result = result, error = err })
    if not ok then warn('result callback failed: ' .. tostring(failure)) end
  elseif status == 'error' then
    warn(err and err.message or 'definition request failed')
  end
end

local function cancel_request(request, explicit)
  if request.finished then return end
  if explicit then request.cancelled = true else request.obsolete = true end
  if request.cancel_scheduled then return end
  request.cancel_scheduled = true
  -- on_lines runs under textlock. The generation changes immediately; protocol
  -- cancellation is scheduled so unrelated autocmds never run under textlock.
  vim.schedule(function()
    local client = request.session.client
    if request.id and client and client.requests[request.id] and not client:is_stopped() then
      client:cancel_request(request.id)
    end
  end)
end

local function invalidate(session)
  session.generation = session.generation + 1
  for _, request in pairs(session.pending) do cancel_request(request, false) end
end

local function canonical(path)
  return uv.fs_realpath(path)
end

local function source_path(session, bufnr)
  if not api.nvim_buf_is_loaded(bufnr) or vim.bo[bufnr].buftype ~= '' then return nil end
  local path = canonical(api.nvim_buf_get_name(bufnr))
  if not path or path:sub(-5) ~= '.java' then return nil end
  for _, root in ipairs(session.roots) do
    if path:sub(1, #root + 1) == root .. '/' then return path end
  end
end

local function untrack(session, bufnr)
  if session.buffers[bufnr] then
    session.buffers[bufnr] = nil
    if current == session and session.active then invalidate(session) end
  end
end

local function attach(session, bufnr)
  if not session.active or current ~= session or not session.client then return end
  local path = source_path(session, bufnr)
  if not path then return end
  local old = session.buffers[bufnr]
  if old and old.path == path and vim.lsp.buf_is_attached(bufnr, session.client.id) then return end
  session.open_serial = session.open_serial + 1
  local entry = { path = path, epoch = session.open_serial }
  session.buffers[bufnr] = entry
  invalidate(session)
  -- Each buffer lifetime has its own callback identity. Old callbacks detach
  -- themselves after restart; they cannot erase a newly attached lifetime.
  local function live()
    return current == session and session.active and session.buffers[bufnr] == entry
  end
  local ok = api.nvim_buf_attach(bufnr, false, {
    on_lines = function()
      if not live() then return true end
      invalidate(session)
    end,
    on_reload = function()
      if not live() then return true end
      invalidate(session)
    end,
    on_detach = function()
      if live() then untrack(session, bufnr) end
    end,
  })
  if not ok or not vim.lsp.buf_attach_client(bufnr, session.client.id) then
    untrack(session, bufnr)
  end
end

local function options(opts)
  local version = vim.version()
  assert(version.major == 0 and version.minor == 11 and version.patch == 5,
    'this preview adapter is validated for Neovim 0.11.5 only')
  opts = vim.deepcopy(opts or configured or {})
  for _, name in ipairs({ 'binary', 'manifest' }) do
    if type(opts[name]) ~= 'string' or opts[name]:sub(1, 1) ~= '/' then
      error(name .. ' must be an explicit absolute path')
    end
    opts[name] = assert(canonical(opts[name]), name .. ' does not exist')
  end
  assert(vim.fn.executable(opts.binary) == 1, 'binary is not executable')
  local stat = assert(uv.fs_stat(opts.manifest))
  assert(stat.type == 'file' and stat.size <= 1048576, 'manifest must be a file of at most 1 MiB')
  local file = assert(io.open(opts.manifest, 'rb'))
  local data = file:read('*a')
  file:close()
  local manifest = vim.json.decode(data)
  assert(manifest.schema_version == 1 and type(manifest.compilations) == 'table', 'invalid manifest')
  opts.root = vim.fs.dirname(opts.manifest)
  opts.roots = {}
  for _, compilation in ipairs(manifest.compilations) do
    for _, root in ipairs(compilation.source_roots or {}) do
      local absolute = root:sub(1, 1) == '/' and root or opts.root .. '/' .. root
      local resolved = assert(canonical(absolute), 'source root does not exist: ' .. root)
      opts.roots[#opts.roots + 1] = resolved
    end
  end
  return opts
end

--- Configure explicit paths. Does not start a server or execute project scripts.
function M.setup(opts)
  assert(not current or not current.active, 'stop the current session before changing setup')
  configured = options(opts)
  return M
end

--- Start one independent session and attach loaded Java buffers under its roots.
function M.start(opts)
  if current and current.active then return current.client and current.client.id end
  if current then M.stop() end
  local config = options(opts)
  configured = config
  session_serial = session_serial + 1
  local session = { active = true, ready = false, generation = 0, open_serial = 0,
    serial = session_serial, request_serial = 0, pending = {}, buffers = {}, roots = config.roots }
  current = session
  session.group = api.nvim_create_augroup('java_analyzer_preview_' .. session.serial, { clear = true })
  local id = vim.lsp.start({
    name = 'java-analyzer-preview',
    cmd = { config.binary, 'lsp', '--manifest', config.manifest },
    cmd_cwd = config.root,
    root_dir = config.root,
    detached = false,
    flags = { debounce_text_changes = 150, exit_timeout = 2000 },
    capabilities = { general = { positionEncodings = { 'utf-16' } } },
    get_language_id = function() return 'java' end,
    on_init = function(client, response)
      if current ~= session or not session.active then client:stop(); return end
      local cap = response.capabilities
      if cap.positionEncoding ~= 'utf-16' or type(cap.textDocumentSync) ~= 'table'
        or cap.textDocumentSync.openClose ~= true or cap.textDocumentSync.change ~= 1
        or cap.definitionProvider ~= true then
        session.error = 'server does not expose the required UTF-16/full-sync/definition capabilities'
        warn(session.error)
        M.stop()
        return
      end
      session.ready = true
    end,
    on_exit = function(code, signal)
      if current == session and session.active then
        session.active, session.ready = false, false
        invalidate(session)
      end
      if config.on_exit then config.on_exit(code, signal) end
    end,
  }, { attach = false })
  if not id then
    session.active = false
    api.nvim_del_augroup_by_id(session.group)
    error('could not start java-analyzer')
  end
  session.client = assert(vim.lsp.get_client_by_id(id))
  api.nvim_create_autocmd({ 'BufReadPost', 'BufEnter' }, { group = session.group,
    callback = function(event) attach(session, event.buf) end })
  api.nvim_create_autocmd('LspAttach', { group = session.group,
    callback = function(event)
      if event.data.client_id == id and not session.buffers[event.buf] then attach(session, event.buf) end
    end })
  api.nvim_create_autocmd('LspDetach', { group = session.group,
    callback = function(event)
      if event.data.client_id == id then untrack(session, event.buf) end
    end })
  api.nvim_create_autocmd('BufFilePost', { group = session.group,
    callback = function(event)
      if session.buffers[event.buf] then
        invalidate(session)
        -- Renaming changes the frozen source inventory, not just a buffer label.
        warn('source path changed; restart after updating the project manifest')
        M.stop()
      end
    end })
  for _, bufnr in ipairs(api.nvim_list_bufs()) do attach(session, bufnr) end
  return id
end

--- Invalidate callbacks immediately, then request ordinary LSP shutdown/exit.
function M.stop()
  local session = current
  if not session then return end
  session.active, session.ready = false, false
  invalidate(session)
  if session.group then pcall(api.nvim_del_augroup_by_id, session.group); session.group = nil end
  if session.client and not session.client:is_stopped() then session.client:stop() end
end

--- Restart after configuration/inventory changes. Also restart after external
--- disk/dependency changes: local generations do not observe those changes.
function M.restart(opts)
  M.stop()
  return M.start(opts)
end

function M.status()
  local s = current
  return { active = s and s.active or false, ready = s and s.ready and s.client.initialized or false,
    client_id = s and s.client and s.client.id, session = s and s.serial,
    generation = s and s.generation, error = s and s.error }
end

--- Safe definition navigation. Return request id and an explicit cancel function.
--- Optional on_result receives {status,result,error}; never changes server results.
function M.definition(opts)
  opts = opts or {}
  local session = current
  if not session or not session.active or not session.ready or not session.client.initialized then
    warn('server is not ready; call start and wait for initialization')
    return nil
  end
  local win, origin = api.nvim_get_current_win(), api.nvim_get_current_buf()
  attach(session, origin)
  local entry = session.buffers[origin]
  if not entry then warn('current buffer is not an existing configured Java source'); return nil end
  for _, pending in pairs(session.pending) do cancel_request(pending, false) end
  session.request_serial = session.request_serial + 1
  local request = { session = session, generation = session.generation, epoch = entry.epoch,
    serial = session.request_serial, origin = origin, cursor = api.nvim_win_get_cursor(win),
    on_result = opts.on_result, ticks = {} }
  for buf in pairs(session.buffers) do request.ticks[buf] = api.nvim_buf_get_changedtick(buf) end
  local client = session.client
  -- Public API, verified specifically against pinned Neovim 0.11.5:
  -- Client:notify flushes pending changes for ALL attached buffers before it
  -- sends any non-didChange notification. Client:request alone flushes origin.
  -- $/setTrace is standard and harmless here; it is NOT an LSP barrier response.
  if not client:notify('$/setTrace', { value = 'off' }) then
    finish(request, 'error', nil, { message = 'could not synchronize buffers' })
    return nil
  end
  local params = vim.lsp.util.make_position_params(win, 'utf-16')
  local function is_current()
    if current ~= session or not session.active or session.client ~= client
      or request.obsolete or request.generation ~= session.generation
      or request.serial ~= session.request_serial then return false end
    local now = session.buffers[origin]
    if not now or now.epoch ~= request.epoch or not api.nvim_win_is_valid(win)
      or api.nvim_get_current_win() ~= win
      or api.nvim_win_get_buf(win) ~= origin
      or not vim.deep_equal(api.nvim_win_get_cursor(win), request.cursor) then return false end
    for buf, tick in pairs(request.ticks) do
      if not api.nvim_buf_is_loaded(buf) or api.nvim_buf_get_changedtick(buf) ~= tick then return false end
    end
    return true
  end
  local sent, id = client:request('textDocument/definition', params, function(err, result)
    if request.cancelled then finish(request, 'cancelled', result, err); return end
    if not is_current() then finish(request, 'stale', result, err); return end
    if err then
      finish(request, err.code == -32800 and 'cancelled' or 'error', result, err)
      return
    end
    if result == nil or result == vim.NIL or (vim.islist(result) and #result == 0) then
      finish(request, 'empty', result)
      return
    end
    local location = result
    if vim.islist(result) then
      if #result ~= 1 then
        finish(request, 'error', result, { message = 'ambiguous definition results are not selected' })
        return
      end
      location = result[1]
    end
    if type(location) ~= 'table' or type(location.uri) ~= 'string'
      or type(location.range) ~= 'table' then
      finish(request, 'error', result, { message = 'unsupported definition result' })
      return
    end
    -- Remove the pending request before opening a previously unloaded target.
    -- The resulting BufEnter can itself advance the workspace generation.
    if request.id then session.pending[request.id] = nil end
    local ok, jumped = pcall(vim.lsp.util.show_document, location, 'utf-16', { focus = true })
    if ok and jumped then finish(request, 'jumped', result)
    else finish(request, 'error', result, { message = 'could not display definition' }) end
  end, origin)
  if not sent then finish(request, 'error', nil, { message = 'could not send definition' }); return nil end
  request.id = id
  if not request.finished then session.pending[id] = request end
  return id, function() cancel_request(request, true) end
end

return M
