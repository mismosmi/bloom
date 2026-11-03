---
bloom: minor
---

# Add use_spawn hook

Expose the renderer's spawner to components through `use_spawn` and a cloneable,
type-erased `Spawner`. Keep renderer entry points generic while using `Spawner`
internally. Stream-rendering entry points now also require spawners to be `Sync`.
