# Native operations

These helpers organize the fork's native boundaries. They **do not make an
arbitrary game address safe**: the current native callback, lock and scene
identity must still authorize each access. Rust owns copied values, not fighters,
menus or the game's resource pools.

| Boundary | Owner and contract |
| --- | --- |
| Fields and pointer chains | `native_fields.rs`: nullable reads/writes check null, alignment and address arithmetic. Chains stop at an absent child. No allocation, logging or retained pointers. |
| Required fields | Explicit `read_unchecked` / `write_unchecked` preserve volatile access after the caller qualifies the object. Used inside existing callback and mutation transactions; never substitute them for a failed checked read. |
| Native atomic words | Width-specific acquire loads preserve the game's synchronization. Volatile loads are not a substitute. |
| Fighter installation | The typed descriptor/install boundary preserves the 0x70-byte, eight-byte-aligned descriptor and per-call reload flags. Callers retain the install gate until native completion/destruction. |
| Mii resources | `mii_native_access::PoolLock` unlocks on scope exit. Existing owner, epoch, record and thread checks stay at the domain boundary. No logging, allocation or Rust mutex acquisition inside the pool transaction. |
| Editor and matchmaking | `mii_editor_bridge` rebinds scene objects on each callback. Typed native calls use this bridge's verified text base; pending edits retain values, not native pointers. |

## Operations that remain specialized

- **Hooks, trampolines and assembly:** preserve registers, displaced instructions,
  ABI and original-call order. Fingerprint checks remain with their installations.
- **Unlink/GC guards:** retain the exact observed null cases. A generic checked
  write must not silently suppress some other native cleanup store.
- **Mapped-memory checks:** Horizon `QueryMemory` verifies a range and permissions;
  it does not pin an object or prove that it is still alive.
- **Native transactions:** keep write/restore and enter/leave pairs together.
  Introducing a new early return halfway through would change behavior.
- **Upstream bindings and diagnostic-only probes:** retain their existing scope.
  This refactor is not a claim that every `unsafe` operation has disappeared.

Required-field fast paths avoid repeated checks after qualification. Their
contracts include non-wrapping ranges, correct alignment and representation,
and protection against concurrent invalidation. Default nullable access uses
the checked helpers; a nonzero address alone grants no ownership.

Validation compares field/descriptor behavior, builds and generated code, then
matched emulator observations. Whole-emulator memory and sampled FPS do not
measure the Switch heap or prove console stability.
