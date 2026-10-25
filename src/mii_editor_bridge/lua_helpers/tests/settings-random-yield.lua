-- The page coroutine, not the installer chunk, must enter update_random.
local helper = assert(loadfile(arg[1]))
local calls = 0
local page = {}

function page:update_random()
    calls = calls + 1
    coroutine.yield("inside-update-random")
end

function page:update_selector1()
    return true
end

local installed = helper(page, 0)
if installed ~= 1 then
    io.stderr:write("install returned " .. tostring(installed) .. "\n")
    os.exit(1)
end
if calls ~= 0 or page.mii_resume_random_pending == true then
    io.stderr:write("install invoked update_random\n")
    os.exit(1)
end

local direct = coroutine.create(function()
    return page:update_random()
end)
local ok, marker = coroutine.resume(direct)
if not ok or marker ~= "inside-update-random" or coroutine.status(direct) ~= "suspended" then
    io.stderr:write("direct resume did not suspend\n")
    os.exit(1)
end
if page.mii_resume_random_pending ~= true then
    io.stderr:write("pending flag clear while suspended\n")
    os.exit(1)
end
local ok_done = coroutine.resume(direct)
if not ok_done or page.mii_resume_random_pending == true or calls ~= 1 then
    io.stderr:write("pending flag stuck after return\n")
    os.exit(1)
end

local queued = helper(page, 1)
if queued ~= 0 or calls ~= 1 or page.mii_resume_random_queued ~= true then
    io.stderr:write("queue helper called update_random\n")
    os.exit(1)
end

local resumed = false
local selector = coroutine.create(function()
    local result = page:update_selector1()
    resumed = true
    return result
end)
ok, marker = coroutine.resume(selector)
if not ok or marker ~= "inside-update-random" or resumed then
    io.stderr:write("selector wrapper did not enter update_random\n")
    os.exit(1)
end
if page.mii_resume_random_pending ~= true or page.mii_resume_random_queued == true then
    io.stderr:write("queue was not consumed before the yield\n")
    os.exit(1)
end
local ok_selector, result = coroutine.resume(selector)
if not ok_selector or result ~= false or not resumed or calls ~= 2 then
    io.stderr:write("selector wrapper did not return false\n")
    os.exit(1)
end
if page.mii_resume_random_pending == true then
    io.stderr:write("pending flag still set\n")
    os.exit(1)
end

local again = coroutine.create(function()
    return page:update_selector1()
end)
local ok_again, native = coroutine.resume(again)
if not ok_again or native ~= true or calls ~= 2 then
    io.stderr:write("second selector call reentered update_random\n")
    os.exit(1)
end
io.write("ok calls=" .. tostring(calls) .. "\n")
