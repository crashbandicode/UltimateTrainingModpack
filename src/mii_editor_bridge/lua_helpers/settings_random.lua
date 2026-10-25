-- Once per page, at age 1, before main enables input. main and
-- update_selector1 look up these methods with SELF on this table, so the
-- next page resume calls the replacements. The pending flag stays set only
-- while update_random is inside that coroutine. lua_pcall must not call it:
-- the native yield cannot cross pcall. A queued restore is consumed by the
-- next update_selector1. Local Wireless replaces update_random before this
-- runs; wrap that function, do not put the native one back.
local settings, queue = ...
if settings.mii_resume_random_wrapped == true then
    if queue == 1 then
        settings.mii_resume_random_queued = true
    end
    return 0
end
if queue == 1 or settings.update_random == nil or settings.update_selector1 == nil then
    return 2
end
local update_random = settings.update_random
local update_selector1 = settings.update_selector1
settings.update_random = function(self)
    self.mii_resume_random_pending = true
    update_random(self)
    self.mii_resume_random_pending = false
end
settings.update_selector1 = function(self)
    if self.mii_resume_random_queued == true then
        self.mii_resume_random_queued = false
        self:update_random()
        return false
    end
    return update_selector1(self)
end
settings.mii_resume_random_wrapped = true
return 1
