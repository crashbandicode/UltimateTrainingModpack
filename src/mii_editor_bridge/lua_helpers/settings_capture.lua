local settings = ...
if settings.is_end or settings.exit_value ~= EXIT_VALUE_DECIDE then
    return 1, 0, 0, 0
end
-- True only while the wrapped update_random is suspended in the page
-- coroutine. A Random highlight never enters that method.
if settings.mii_resume_random_pending == true then
    return 0, settings.selector0:get_selected_button_id(), 1, 1
end
local area = -1
if settings.selector0:has_focus() then
    area = 0
elseif settings.selector1:has_focus() then
    area = settings.selector1:get_selected_button_id() + 1
elseif settings:is_selected_com_frame_actor() then
    area = 3
end
return 0, settings.selector0:get_selected_button_id(), area, 0
