---
bloom: minor
---

# Add custom hook identities and memoized callbacks

Expose the `Identify` trait and derive macro for structs and enums, allowing
custom identities for `use_memo` and `use_effect` arguments. Existing `Hash`
types continue to work through a blanket implementation.

Add `use_callback` to memoize an owned callback in an `Arc`, refreshing it when
the captures' identity or callback function changes.
