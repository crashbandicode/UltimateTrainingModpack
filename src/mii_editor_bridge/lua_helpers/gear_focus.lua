-- set_sort_id invokes sort_id_list before the captured item is placed.
local page, sort, item = ...
page:set_sort_id(sort)
page.scroller:select_item(item, false)
page.scroller:set_focus(true)
page.scroller:redraw_all_items()
