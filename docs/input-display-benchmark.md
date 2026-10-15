# Input display host benchmark

These numbers are a host CPU and pane-operation count. They are not emulator FPS.

Baseline label: baseline-git-17c4e3f9b257000b17d099b4ed42b79448b5dcfd
Tree nodes: 2897
Tree shape: root/TrModInputLog/15 rows/(Frame,Status,Input0-4 x 37 icons)

Baseline repeated on the same binary and the pane dumps matched.

Candidate label: candidate-worktree

| scenario | step | visits base | visits cand | calls base | calls cand | vis base | vis cand | text base | text cand | allocs base | allocs cand | visible |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| steady_repeat | draw1 | 24468 | 2 | 212 | 1 | 203 | 1 | 2 | 0 | 25 | 0 | match |
| steady_repeat | draw2 | 24468 | 2 | 212 | 1 | 203 | 1 | 2 | 0 | 25 | 0 | match |
| steady_tick | draw1 | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| steady_tick | draw2 | 24423 | 6 | 210 | 3 | 202 | 1 | 2 | 1 | 24 | 1 | match |
| changing_buttons | before | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| changing_buttons | after | 24849 | 440 | 218 | 14 | 206 | 7 | 2 | 1 | 29 | 3 | match |
| ring_wrap | base14 | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| ring_wrap | wrapped | 24424 | 20314 | 210 | 20 | 202 | 3 | 2 | 1 | 24 | 2 | match |
| rapid_input | input0 | 24571 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input1 | 24572 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input2 | 24573 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input3 | 24574 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input4 | 24575 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input5 | 24576 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input6 | 24577 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input7 | 24578 | 20469 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input8 | 24571 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input9 | 24572 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input10 | 24573 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input11 | 24574 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input12 | 24575 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input13 | 24576 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input14 | 24577 | 20461 | 214 | 24 | 204 | 5 | 2 | 1 | 26 | 2 | match |
| rapid_input | input15 | 24578 | 20388 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input16 | 24571 | 20382 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input17 | 24572 | 20384 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input18 | 24573 | 20386 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input19 | 24574 | 20388 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input20 | 24575 | 20390 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input21 | 24576 | 20392 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input22 | 24577 | 20394 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input23 | 24578 | 20388 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input24 | 24571 | 20382 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input25 | 24572 | 20384 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input26 | 24573 | 20386 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input27 | 24574 | 20388 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input28 | 24575 | 20390 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input29 | 24576 | 20392 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input30 | 24577 | 20394 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| rapid_input | input31 | 24578 | 20388 | 214 | 19 | 204 | 3 | 2 | 0 | 26 | 1 | match |
| icon_color | left_white | 24431 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| icon_color | right_yellow | 24431 | 28 | 210 | 5 | 202 | 3 | 2 | 0 | 24 | 1 | match |
| icon_color | cleared | 24417 | 18 | 208 | 4 | 201 | 2 | 2 | 0 | 22 | 0 | match |
| frame_cap | first99 | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| frame_cap | held99 | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| mode_cycle | smash | 24552 | 2 | 214 | 1 | 204 | 1 | 2 | 0 | 26 | 0 | match |
| mode_cycle | raw | 24552 | 100 | 214 | 8 | 204 | 5 | 2 | 0 | 26 | 1 | match |
| mode_cycle | status | 24417 | 139 | 208 | 8 | 201 | 4 | 2 | 0 | 22 | 0 | match |
| status_toggle | on | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 26 | 0 | match |
| status_toggle | off | 24423 | 7 | 210 | 3 | 202 | 1 | 2 | 1 | 24 | 0 | match |
| status_toggle | changed | 24423 | 7 | 210 | 3 | 202 | 1 | 2 | 1 | 26 | 3 | match |
| hide_reopen | shown | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 26 | 0 | match |
| hide_reopen | quick_hidden | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 26 | 0 | match |
| hide_reopen | quick_reopen | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 26 | 0 | match |
| hide_reopen | vanilla_hidden | 24424 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 25 | 0 | match |
| hide_reopen | vanilla_reopen | 24424 | 18 | 210 | 7 | 202 | 3 | 2 | 2 | 25 | 3 | match |
| hide_reopen | none_hidden | 2 | 2 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | match |
| hide_reopen | smash_reopen | 24424 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 25 | 0 | match |
| ttl_fade | draw | 24423 | 2 | 210 | 1 | 202 | 1 | 2 | 0 | 24 | 0 | match |
| reset | dirty | 24571 | 2 | 214 | 1 | 204 | 1 | 2 | 0 | 26 | 0 | match |
| reset | cleared | 24417 | 20453 | 208 | 23 | 201 | 4 | 2 | 1 | 22 | 1 | match |
| reregister | established | 24483 | 2 | 212 | 1 | 203 | 1 | 2 | 0 | 25 | 0 | match |
| reregister | after_invalidate | 24483 | 81969 | 212 | 2912 | 203 | 2807 | 2 | 30 | 25 | 30 | history restored |

| scenario | baseline ns/iter | candidate ns/iter | candidate/baseline |
| --- | ---: | ---: | ---: |
| steady_repeat | 399521 | 421 | 0.001 |
| steady_tick | 387307 | 633 | 0.002 |
| changing_buttons | 489232 | 5669 | 0.012 |
| ring_wrap | 450248 | 184115 | 0.409 |
| rapid_input_120_frames | 27367134 | 19078139 | 0.697 |
| mode_cycle | 592980 | 3192 | 0.005 |
| status_toggle | 780910 | 1256 | 0.002 |
| hide_reopen | 1205637 | 2412 | 0.002 |
| ttl_fade | 205785 | 370 | 0.002 |
| reset | 407587 | 171852 | 0.422 |
| reregister | 602402 | 725069 | 1.204 |

CPU nanoseconds are host `Instant` around the scenario body at opt-level 3, 20 iterations after 2 warmups. Not FPS.

Notes:
- `hide_reopen/vanilla_hidden` screenshot-visible state matches. Full pane dump differs while the log root is hidden or children diverged off-screen.
- Informational `reregister/stale_redraw` is outside the pass rule. visible baseline a0df3d044eeaf6e0 candidate 5bd0c1f0f2a9c67d.

Visible pane state matched baseline on ordinary updates; registration restored all retained history and the missing-invalidation control failed as expected.
