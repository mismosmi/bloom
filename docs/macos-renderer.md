# A native macOS renderer for bloom: evaluation

**Verdict: feasible, and bloom-core needs no changes to get started.** A minimal AppKit renderer
plugs into the existing `ObjectModel` trait and `render_loop`. The real work sits outside the core:
a node vocabulary that isn't HTML, layout, text measurement, and native controls that hold their
own state.

There is a working prototype in [`prototypes/bloom-macos`](../prototypes/bloom-macos) (~650 lines
excluding tests). From Linux:

- a headless test runs `render_loop` with a non-HTML node type, using `rsx!`, `use_state` and a
  click handler that triggers a re-render: **passes**;
- the AppKit backend type-checks and passes clippy with `--target aarch64-apple-darwin`.

The prototype has **not been run on a Mac**. That is the first thing to do with it
(`cargo run --example counter`).

## 1. How bloom plugs into a backend today

| Piece | Where | Web implementation | Reusable on macOS? |
|---|---|---|---|
| Diffing, hooks, effects, context | `bloom-core` | – | Yes, unchanged |
| Backend interface: `create(node, parent, sibling)`, `update(old, new)`, `remove`, `start`/`finalize`, `subscribe` | `bloom_core::ObjectModel` | `bloom-client::Dom` | Yes, it's a retained-mode interface that maps directly onto a view tree |
| Node type (`Component::Node`) | `bloom-html::HtmlNode` | – | **No.** Event handlers are typed `Fn(web_sys::Event)` and `DomRef` stores `web_sys::Node` |
| `rsx!` | `bloom-rsx` | expands to `tag("x").attr(..).on(..).dom_ref(..).build().children(..)` | Yes. The macro is duck-typed, so any crate exporting a matching `tag()` builder works |
| Task spawning | `futures::task::Spawn` arg | `spawn_local` | Yes. Any thread pool works for component futures |

`render_loop` puts no `Send` bound on the `ObjectModel`, so a model that owns `!Send` AppKit objects
(`Retained<NSView>`) compiles as is. Effects run inside the render loop, so on the main thread they
can touch views directly.

## 2. Architecture

```
Component (Node = NativeNode)
        │ rsx!
        ▼
bloom-core render_loop ──spawn──▶ thread pool (Send component futures)
        │ create / update / remove / finalize
        ▼
AppKitModel (main thread only)
  ├─ PtrWeakKeyHashMap<Weak<NativeNode>, NodeState{ NSView, taffy NodeId, ActionTarget }>
  ├─ taffy LayoutTree → frames assigned in finalize()
  └─ NSWindow.contentView (flipped root view)
        ▲
main-thread executor: Waker → dispatch_async(main queue) → poll render_loop
```

### 2.1 Bindings: `objc2` + `objc2-app-kit`

`objc2` and its generated framework crates are the maintained, standard way to call AppKit from
Rust (winit, wgpu and Tauri's stack use them). They provide `MainThreadMarker`/`MainThreadOnly` to
enforce AppKit's threading rules at compile time, and `define_class!` for the two Objective-C
subclasses a renderer needs: a flipped `NSView`, and a target object for target/action events.

Alternatives considered:

- **cacao**: a higher-level wrapper, but development has stalled and it would sit between bloom and
  AppKit anyway.
- **SwiftUI**: it is itself a diffing framework, and Rust can only drive it through a Swift bridge.
  This means two reconcilers and a hard FFI boundary.
- **WKWebView + bloom-client** (Tauri-style): already possible today with no new renderer, but it
  isn't native UI.
- **Custom drawing** (Blitz/vello, Xilem): not native controls, so accessibility and the platform
  look would have to be rebuilt.

### 2.2 Threading and the event loop

AppKit must run on the main thread, and `NSApplication.run()` owns that thread. The prototype's
[`executor.rs`](../prototypes/bloom-macos/src/executor.rs) stores the `render_loop` future in a
main-thread `thread_local`. Its `Waker` calls `DispatchQueue::main().exec_async(poll)`, and wakes
are coalesced with an `AtomicBool`. AppKit drains the main queue between events, so render passes
interleave with input handling. Component futures are `Send` and run on a `ThreadPool`. When one
completes it signals the render loop, which wakes the main thread.

This replaces `wasm_bindgen_futures::spawn_local` and is roughly 60 lines.

### 2.3 Node vocabulary: a new crate, not `bloom-html`

`HtmlNode` can't be reused: `HtmlElementBuilder::on` takes `Fn(web_sys::Event)`, and `bloom-html`
depends on `web-sys`. A native node crate needs:

- `NativeNode { Element(Arc<NativeElement>), Text(String) }` with `From<String>`, because bare
  strings in `rsx!` become text nodes;
- a `tag()` builder with `attr` / `on` / `build` / `children` (and a `ref` replacement for
  `DomRef`), so that `rsx!` works unchanged;
- a platform-neutral event enum (`Click`, `Change(String)`, …) so node descriptions stay
  `Send + Sync` and testable without a Mac.

The prototype maps tags as follows: `stack`/`view` → flipped `NSView`, `button` → `NSButton`, text
→ `NSTextField` label. A real crate would add `input`, `image`, `scroll`, `toggle`, `select`,
`list`, etc.

**Cross-platform decision.** Because `Component` has an associated `Node` type, a component written
for `HtmlNode` can't render on macOS. If the goal is "true native cross-platform" (README), the node
crate should be a platform-neutral `bloom-native` vocabulary (React Native's model), with
`bloom-macos` as its first backend. GTK/WinUI backends, or a DOM backend in the style of
react-native-web, can come later. This decision shapes the public API more than anything else, so
make it before writing much code.

### 2.4 Layout: taffy

AppKit has no flexbox. The two options are:

1. **Auto Layout/`NSStackView`**: translate styles into constraints. Constraint updates on
   every diff are slow and hard to debug, and AppKit's model doesn't match CSS-like props.
2. **taffy** (Rust flexbox/grid, the Yoga equivalent): keep a taffy node per view. Leaf controls
   are measured with `fittingSize`. Layout runs once per pass in `finalize()` and the result is
   assigned to `NSView.frame`. Containers are flipped so that taffy's top-left origin matches.

The prototype uses taffy ([`layout.rs`](../prototypes/bloom-macos/src/layout.rs)), which is the
recommended option. Remaining work:

- **Text measurement**: wrapping labels need width-constrained measuring
  (`NSTextFieldCell.cellSizeForBounds:`) rather than `fittingSize`.
- **Window resize**: re-run layout on `windowDidResize`/`setFrameSize:` without a full render pass.
  The layout tree then needs to be shared (`Rc<RefCell<…>>`) between the model and a resize hook.
- **Style props**: colours, borders and corner radius go through `wantsLayer` + `CALayer`. Fonts go
  through `NSFont`. Pick a typed style API rather than stringly CSS.

### 2.5 Events

`NSControl` uses target/action, and the target is held **weakly**. The prototype's `ActionTarget`
(`define_class!` over `NSObject`, with a `Box<dyn Fn()>` ivar) is therefore stored in `NodeState`.
It captures the `Arc<NativeNode>` and is rebuilt on `update`, the same approach as bloom-client's
`Closure`s. Text input needs an `NSTextFieldDelegate` (`controlTextDidChange:`). Lists need an
`NSTableView` data source. Both are further `define_class!` objects of the same kind.

## 3. Gaps in bloom-core this exposed

None of these blocks a first version, but the first two cause visible bugs in native UIs.

1. **Changing a node's tag keeps its children in the old parent.** For `(Node, Node)`, the core
   always calls `ObjectModel::update` and then diffs the children against the *new* parent
   (`render_loop.rs`, the `Update` arm). When a backend has to swap the native object (for example
   `stack` → `button`, or `div` → `span` on the web), the children stay attached to the replaced
   object. `bloom-client`'s `replace_child` path has the same problem. Fix: let the core treat an
   incompatible change as `remove` + `create`, either with an `ObjectModel::can_update(old, new)`
   hook or a node-kind comparison.
2. **No keyed children.** `update_children` diffs by position. Reordering a list re-targets every
   row. On native, rows carry real state (focus, cursor position, scroll offset, selection), so this
   is far more noticeable than in the DOM. Add `key`s.
3. **`update` is called for every node on every pass.** `HtmlElement`'s `PartialEq` returns false
   whenever callbacks exist. Backends should diff attributes themselves (the prototype rebuilds
   targets each time, which is cheap), or the core could skip unchanged nodes.
4. **`start`/`finalize` futures must be `Send`.** This is unnecessary because `render_loop` itself
   isn't `Send`, and it would block a `finalize` that awaits a display-link tick through `!Send`
   objects. Removing the bound is easy and the change is backwards compatible.
5. **Controlled inputs.** Writing `stringValue` back on every render moves the cursor. Backends
   must only write when the value differs. A core helper or documented convention would help.

## 4. Effort estimate

Rough figures for one developer who knows Rust but not AppKit:

| Phase | Scope | Estimate |
|---|---|---|
| 0. Core prep | Tag-change replace (§3.1), drop `Send` on `start`/`finalize`, macOS CI job | 2–4 days |
| 1. MVP | `bloom-native` node crate, `bloom-macos` with window, view/stack, text, button, text input, image; taffy layout; target/action and delegate events; `NativeRef`; example; `macos-latest` CI | 2–3 weeks (the prototype covers roughly a third of this) |
| 2. Usable | Typed styles (colour/border/radius/font), wrapping text, scroll views, keyed children (§3.2), resize relayout, focus/keyboard, menus, multiple windows | 4–6 weeks |
| 3. Polish | Accessibility audit, animations, virtualised lists (`NSTableView`), `.app` bundling/codesigning (`cargo-bundle`), UI tests | open-ended |

## 5. Risks

- **Layout fidelity**: taffy is solid, but text measurement and baseline alignment against AppKit
  controls take iteration.
- **Native-state controls** (text fields, scroll views, tables) are where React Native spent most of
  its effort. Without keys (§3.2) and careful controlled-input handling, they will feel wrong.
- **Testing**: AppKit code can be type-checked on Linux (verified), but anything that runs needs a
  macOS runner. Keep as much as possible platform-neutral (node crate, layout, a recording
  `ObjectModel`) so most tests stay on ubuntu.
- **API churn**: choosing `bloom-native` versus a macOS-specific vocabulary is a one-way door for
  users' components.

## 6. Recommended next steps

1. Run the prototype on a Mac (`cd prototypes/bloom-macos && cargo run --example counter`) to
   confirm the executor and frame assignment behave as type-checked.
2. Decide on the node vocabulary question (§2.3).
3. Fix §3.1 in bloom-core, with a headless test using a recording `ObjectModel` like the one in
   `prototypes/bloom-macos/tests/headless.rs`.
4. Promote the prototype into workspace crates (`bloom-native`, `bloom-macos`) and add a
   `macos-latest` CI job. On ubuntu, `cargo clippy --target aarch64-apple-darwin -p bloom-macos`
   also works as a cheap type-check.
