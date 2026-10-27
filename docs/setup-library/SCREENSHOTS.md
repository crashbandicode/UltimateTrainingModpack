# Image provenance

The images come from emulator run150 on 2026-10-27. That run used the installed NRO with
SHA-256 `9d537cd543c04d380cc616abeed047632c366ddac68aeb57e026e974693cc8ba` in Eden
`40b6b78a53`, with Mii Gunner as P1 against a Duck Hunt CPU on the Training stage. The
local archive copy is `runtime150/collected150/live/`, and its source hashes match.
The run's receipt `LIBRARY150.json` records the slot change and the bind. The menu's
Save State Slot was changed from 2 to 3 while the menu stayed open. The list then offered
`Y binds to Slot 3`, and the log shows `library_bound slot=2 restore=false`, where slot 2
is the zero-based Slot 3. Y did not move either fighter. A later move and
**ZL + D-pad Up** restored the setup.

Each source is one Eden recording: 1280 × 720 game frames, about 30 fps, 10 s long. The
images were trimmed in time and, where noted, scaled down. Nothing was upscaled
or annotated. `library.gif` joins the binding and restore recordings with one
straight cut; each segment plays at its original speed.

| Image | Source (`collected150/live/`) | Window | Processing |
| --- | --- | --- | --- |
| [list.png](list.png) | `bind-pending-slot3/transition.gif` | frame at 0.48 s, before Y (pressed at 1.03 s) | 1280 × 720, native |
| [bind.gif](bind.gif) | `load-bound-slot3/transition.gif` | 0.5–2.15 s: B closes the menu at 1.03 s, notice by 1.2 s | 1280 × 720, 10 fps, 1.70 s. Ends before the load chord's effect at 2.2 s |
| [restore.png](restore.png) | `slot3-move-and-restore/transition.gif` | 2.59 s (after the move) and 5.59 s (after ZL + D-pad Up at 2.85 s) | Two frames side by side, each scaled to 640 × 360 |
| [restore.gif](restore.gif) | `slot3-move-and-restore/transition.gif` | 0–8 s | 1280 × 720, 10 fps, 256-colour palette |
| [library.gif](library.gif) | `bind-pending-slot3/transition.gif`, then `slot3-move-and-restore/transition.gif` | 0–3 s, then 0–8 s | 1280 × 720, 10 fps, 11 s; one straight cut |

The walkthrough and README use the animated examples. The older stills remain
available as reference images.

[Back to Setup Library](../setup-library.md)
