# Lobby Practice footage provenance

## Added Local Wireless examples — 2026-10-25

Both examples in the [walkthrough](../lobby-practice.md) are unchanged native
game recordings from the completed run116 acceptance session. No new emulator
session was run to make this documentation. The files retain their original
1280 × 720 pixels and frame delays, with no cuts, overlays or speed changes.

| File | Source capture | Frames / duration | Observed behavior |
| --- | --- | --- | --- |
| [ldn-item-restore.gif](ldn-item-restore.gif) | `run116/case173-item-reload.gif` | 66 / 6.610 s | Throw, empty hands and movement, followed by restoration of the saved position and Daisy Stitch-Face Turnip. Native save readback independently confirmed both saved positions and item choice5/variation7 |
| [ldn-ledge-playback.gif](ldn-ledge-playback.gif) | `run116/case173-ledge-getup.gif` | 67 / 6.690 s | Positioning recording reaches the ledge; the separate 60-input A-tap Slot1 recording is then admitted automatically, produces a getup attack and returns idle. P1 remains idle; no manual playback chord was sent |

SHA-256 identities (source and documentation copies are identical):

- Item restore: `aaf311a78ba20c0204470104f549e2bd2895d59babdcf0fcd9395a68e8b14f0e`
- Ledge playback: `052956f96b3b119d6e942ecdd0e7add162a812ca92d6d7c8a85b1b6a60a7559b`

Both captures report zero dropped frames. The primary reviewed first/action/end
frames, including the actual throw and empty-handed interval, ledge hang,
Playback label, getup attack and final idle state. The initial red Recording
label in the ledge clip belongs to the positioning fixture, not the subsequent
Slot1 playback. These are ordinary numbered-recording playback and save/load
examples, not replay-clip-editor previews.

### Build and evidence scope

- Smash Ultimate 13.0.5, muted Eden baseline commit
  `40b6b78a5323488c9431520143af321d70c27060`, Local Wireless waiting practice.
- Training Modpack lobby-parity build60 NRO:
  `d42a363cead5e1b535452faae14983bae51e0a667860811f7184f3b005a6eaf1`.
- Native Partners1/Stop, waiting Player entry0 and CPU entry4; original Ready card
  retained. The held-item example moves only the player before reloading.
- Private evidence: campaign `console-practice91/lobby-parity167`,
  `runtime116/PRIMARY-REVIEW173.md` and
  `runtime116/collected-1792954331704318671/run/`, including the matching JSON
  receipts/native save readbacks and bounded one-hit admission/idle observations.
  `DOCS176.json` binds the promoted files and original frame timing.
- Run116 exited normally and its backed-up settings/plugins were independently
  verified restored. Native game saves were retained.

These clips do not establish physical-Switch acceptance or nonzero-damage restore
in LDN. Later real-match LDN interruption/return checks passed without published
footage. The historical run115 ledge fixture remains partial; it is not the clip
shown here.

## Existing media

`waiting-room.gif`, `waiting-room.png` and `stage-menu.gif` predate these examples
and are preserved unchanged. Their older-build captions and acceptance scope
remain separate from build60.

The older `../feature-walkthroughs/lobby-enable.png` remains available for the
earlier alpha. The current guide uses [enable.png](enable.png), captured in
ordinary Training during run145 on C72. It shows **On** checked and the current
help: save states and recording apply only to the local practice fighters.
The 900 × 506 image removes only the window chrome and black margins, with no
scaling or alteration of the UI. The option was already On; opening and closing
this page changed no setting. Identity and crop are in `../media-provenance.json`.
