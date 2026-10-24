local sf, pending, main, sub = ...
local state = sf:get()
local command, cursor = -1, -1
if state == 8 then
    command = sub.command_id
    cursor = sub.selector:get_selected_button_id()
elseif state == 5 then
    command = main.decided_id
elseif state == 6 or state == 7 then
    command = sub.command_id
end
return state, pending[1], pending[2], pending[3], pending[4],
    main.selector:get_selected_button_id(), command, cursor
