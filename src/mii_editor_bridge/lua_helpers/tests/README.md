# Native editor helper checks

These Lua 5.3 checks execute the production helper against a small native-page
stand-in. They check Voice pitch eligibility and that Random resumes on the
page coroutine, across a real coroutine yield. They do not qualify the game’s
native object layout, owner teardown, or popup lifetime; those require the
targeted emulator match/return checks.

Run from the repository root with Lua 5.3:

```sh
lua5.3 src/mii_editor_bridge/lua_helpers/tests/voice-focus-eligibility.lua src/mii_editor_bridge/lua_helpers/voice_focus.lua
lua5.3 src/mii_editor_bridge/lua_helpers/tests/settings-random-yield.lua src/mii_editor_bridge/lua_helpers/settings_random.lua
```
