# A native macOS renderer for bloom: evaluation

**Verdict: feasible, and bloom-core needs no changes to get started.** An AppKit renderer plugs
into the existing `ObjectModel` trait and `render_loop`.

**Direction.** The renderer stays as close to AppKit's own paradigm as possible:

- nodes are typed mirrors of AppKit classes (`Window`, `StackView`, `Button`, `TextField`), not
  string tags;
- props are named after AppKit properties and take AppKit's own types;
- layout is AppKit's (`NSStackView` with Auto Layout);
- events are target/action and delegates, and handlers receive the sender.

Cross-platform abstractions can be built on top of this later.

There is a working prototype in [`prototypes/bloom-appkit`](../prototypes/bloom-appkit) (~780
lines of library code). It has been type-checked and passes clippy with `--target aarch64-apple-darwin` from
Linux. It has **not yet been compiled natively, run, or tested on a Mac**. The first step is
`cargo test && cargo run --example counter` there.

```rust
rsx!(
    <Window title="bloom" content_size={NSSize::new(320.0, 160.0)}>
        <StackView orientation={NSUserInterfaceLayoutOrientation::Vertical} spacing=8>
            {format!("Count: {}", *count)}
            <TextField
                placeholder_string="Your name"
                string_value={name.to_string()}
                on_change={move |field: &NSTextField| {
                    let value = field.stringValue().to_string();
                    name.update(move |_| value)
                }}
            />
            <Button title="Increment" on_action={move |_| count.update(|c| *c + 1)} />
        </StackView>
    </Window>
)
```

## 1. How bloom plugs into a backend today

| Piece | Where | Reusable for AppKit? |
|---|---|---|
| Diffing, hooks, effects, context | `bloom-core` | Yes, unchanged |
| Backend interface: `create(node, parent, sibling)`, `update(old, new)`, `remove`, `start`/`finalize`, `subscribe` | `bloom_core::ObjectModel` | Yes. It's retained-mode and maps directly onto AppKit's object graph |
| Node type (`Component::Node`) | `bloom-html::HtmlNode` | No. It needs its own node type (§2.3) |
| `rsx!` | `bloom-rsx` | Yes, unchanged. Capitalised names expand to `Name::builder().prop(v).children(..).build().into()`, so typed node structs with builders work without string tags |
| Task spawning | `futures::task::Spawn` argument | Yes. Component futures run on any thread pool |

`render_loop` puts no `Send` bound on the `ObjectModel`, so a model that owns `!Send` AppKit
objects (`Retained<NSView>`) compiles as is. Effects run inside the render loop, on the main
thread, so they can call AppKit directly.

## 2. Architecture

```
Component (Node = AppKitNode)
        │ rsx!  <Window> <StackView> <Button> …
        ▼
bloom-core render_loop ──spawn──▶ thread pool (Send component futures)
        │ create / update / remove
        ▼
AppKitModel (main thread only)
  PtrWeakKeyHashMap<Weak<AppKitNode>, { Native object, Slot it is attached to }>
        │
        ├─ Application → NSWindow children
        ├─ Window      → its single child becomes contentView
        └─ StackView   → children become arranged subviews (Auto Layout)
        ▲
main-thread executor: Waker → dispatch_async(main queue) → poll render_loop
```

### 2.1 Bindings: `objc2` + `objc2-app-kit`

`objc2` and its generated framework crates are the maintained, standard way to call AppKit from
Rust (winit, wgpu and Tauri's stack use them). The relevant pieces:

- **`MainThreadMarker`/`MainThreadOnly`** enforce AppKit's threading rules at compile time.
- **`define_class!`** declares the one Objective-C class the prototype needs: `BloomTarget`, an
  action target that is also an `NSTextFieldDelegate`.
- **`dispatch2::MainThreadBound`** makes `Outlet<T>` `Send + Sync` so it can live in `use_ref`.

Alternatives considered:

- **cacao**: development has stalled, and it is another layer between bloom and AppKit.
- **SwiftUI**: it is itself a diffing framework, so this would mean two reconcilers and a Swift
  bridge.
- **WKWebView + bloom-client**: possible today, but not native.
- **Custom drawing** (Blitz, Xilem): not native controls.

### 2.2 Threading and the event loop

AppKit must run on the main thread, and `NSApplication.run()` owns that thread. The prototype's
[`executor.rs`](../prototypes/bloom-appkit/src/executor.rs) stores the `render_loop` future in a
main-thread `thread_local`. Its `Waker` calls `DispatchQueue::main().exec_async(poll)`, and wakes
are coalesced with an `AtomicBool`. AppKit drains the main queue between events, so render passes
interleave with input handling. Component futures are `Send` and run on a `ThreadPool`.

The executor is about 60 lines, and replaces `wasm_bindgen_futures::spawn_local`.

### 2.3 Node types mirror AppKit classes

[`node.rs`](../prototypes/bloom-appkit/src/node.rs) defines one struct per AppKit class.

| Node | AppKit | Props (AppKit property names) | Children |
|---|---|---|---|
| (root) | `NSApplication` | – | `Window`s |
| `Window` | `NSWindow` | `title`, `content_size: NSSize`, `outlet` | one view → `contentView` |
| `StackView` | `NSStackView` | `orientation: NSUserInterfaceLayoutOrientation`, `alignment: NSLayoutAttribute`, `spacing`, `edge_insets: NSEdgeInsets`, `outlet` | arranged subviews |
| `Button` | `NSButton` | `title`, `enabled`, `bezel_style: NSBezelStyle`, `key_equivalent`, `on_action`, `outlet` | – |
| `TextField` | `NSTextField` | `string_value`, `placeholder_string`, `editable`, `selectable`, `bezeled`, `draws_background`, `on_action`, `on_change`, `outlet` | – |

Design points:

- **Props.** Every prop is an `Option`. An unset prop leaves AppKit's default alone. On `update`,
  a prop is written only when it differs from the previous node's value. Enum props take
  objc2's own types (`NSUserInterfaceLayoutOrientation::Vertical`), so there is no translation
  layer to keep in sync with AppKit.
- **Bare strings in `rsx!`** become labels, configured the way `+[NSTextField labelWithString:]`
  does it. This is needed because bloom-core requires `Node: From<String>`.
- **`Outlet<T>`** is the IBOutlet equivalent: `use_ref::<Outlet<NSButton>>()`, passed as
  `outlet={…}`. It gives effects the real `Retained<NSButton>` for anything the props don't
  cover (`makeFirstResponder`, animations, …). `ref` is a keyword in prop position, which is
  another reason for the AppKit name.
- **Names.** The NS prefix is dropped so that `bloom_appkit::Button` (description) doesn't
  collide with `objc2_app_kit::NSButton` (object). Handlers and outlets use the latter. This is
  cheap to revisit.
- **Builders.** They are hand-written, with two small macros for props and actions. A derive in
  `bloom-macro` would make adding classes mechanical. That matters because the class set will
  keep growing: `NSImageView`, `NSScrollView`, `NSPopUpButton`, `NSSwitch`, `NSSlider`,
  `NSTableView`/`NSOutlineView`, `NSSplitView`, `NSTabView`, and menus.

### 2.4 Layout is AppKit's

There is no layout engine in the renderer. `NSWindow` sizes its `contentView`, and `NSStackView`
arranges its subviews with Auto Layout using each control's intrinsic content size. `finalize()`
has nothing to do. This removes the taffy dependency an earlier version of the prototype used,
along with its measurement and resize problems: window resizing, text wrapping and baselines are
AppKit's job.

Open question: arbitrary Auto Layout. `NSStackView` covers most forms and toolbars. For anything
else, AppKit's answer is constraints between views. Options, in increasing order of effort:

1. a plain `View` (`NSView`) node plus `Outlet`s, with constraints set up imperatively in an
   effect;
2. a declarative `constraints` prop that refers to sibling outlets;
3. `NSGridView` as a further typed node.

Option 1 is enough to ship. Decide on option 2 once real layouts need it.

### 2.5 Events: target/action and delegates

- **Target/action.** Every control gets a `BloomTarget` as its `target` with
  `action = performAction:`. AppKit holds targets and delegates **weakly**, so the model keeps
  each `BloomTarget` alive next to its control.
- **Handler types.** Handlers have the AppKit signature with the sender:
  `on_action: Fn(&NSButton)`, `on_change: Fn(&NSTextField)` (from the delegate's
  `controlTextDidChange:`). They are stored as `Arc<dyn Fn(&T) + Send + Sync>` so that nodes can
  be built on the thread pool. They are only ever called on the main thread.
- **Rebinding.** Closures can't be compared, so the handler inside the existing target is
  swapped on every update. The control and target stay the same objects.
- **Constraint found while type-checking.** `State::update` closures must be `Send`, so a
  handler can't capture the sender inside them. It has to read plain data first:
  `let v = field.stringValue().to_string(); name.update(move |_| v)`. This is the same rule as
  for AppKit code that hops queues, and it is worth documenting for users.
- **Controlled text fields.** `string_value` is compared against the field's *current* value,
  not the previous prop, before writing. This avoids moving the cursor while the user types.

Still to add: an `NSWindowDelegate` (`windowWillClose:` → `on_close`; today, closing a window
that the tree still renders leaves a hidden window), an app delegate (terminate after the last
window closes), and menus (`NSMenu` as nodes under the application).

## 3. Gaps in bloom-core this exposed

None of these blocks a first version, but the first two cause visible bugs.

1. **Changing a node's type keeps its children in the replaced object.** For `(Node, Node)`, the
   core always calls `ObjectModel::update` and then diffs the children against the new parent.
   When a node changes AppKit class (`StackView` → `Button`, or `div` → `span` on the web), the
   backend has to swap objects. The prototype swaps leaves correctly, but a replaced
   `StackView`'s children stay in the old view. `bloom-client`'s `replace_child` path has the same
   problem. Fix: let the core treat an incompatible change as `remove` + `create`, through an
   `ObjectModel::can_update(old, new)` hook or a node-kind comparison.
2. **No keyed children.** `update_children` diffs by position. Native controls carry real state
   (focus, cursor position, scroll offset, selection), so position-based reuse shows up as focus
   jumping between rows. Add `key`s.
3. **`update` is called for every node on every pass.** The prototype keeps this cheap by
   diffing props itself, but the core could skip nodes whose elements are identical.
4. **`start`/`finalize` futures must be `Send`.** This is unnecessary because `render_loop`
   itself isn't `Send`. Removing the bound is backwards compatible.

## 4. Effort estimate

Rough figures for one developer who knows Rust but not AppKit:

| Phase | Scope | Estimate |
|---|---|---|
| 0. Core prep | Kind-change replace (§3.1), drop `Send` on `start`/`finalize`, macOS CI job | 2–4 days |
| 1. MVP | Promote the prototype to a `bloom-appkit` crate; run it on macOS; window/app delegates; `View`, `ImageView`, `ScrollView`, `Switch`, `Slider`, `PopUpButton`; builder derive; examples; `macos-latest` CI | 2–3 weeks (the prototype covers roughly a third) |
| 2. Usable | Keyed children (§3.2), `NSTableView`/`NSOutlineView` with data sources, menus, constraint API (§2.4), focus/first responder, multiple windows with delegates | 4–6 weeks |
| 3. Polish | Accessibility review, animations, `.app` bundling/codesigning (`cargo-bundle`), UI tests | open-ended |

Using AppKit's layout instead of taffy takes about a week out of the earlier estimate.
Supporting the class surface (one typed node per class, with its props) becomes the main ongoing
cost, which is why a builder derive is in phase 1.

## 5. Risks

- **Data-source views.** `NSTableView` and `NSOutlineView` pull rows through a data source instead
  of owning child views, so they don't fit "children = subviews". They need a dedicated design,
  for example a `rows` prop plus a component per row rendered into the cell views.
- **Native-state controls.** Without keys (§3.2), list reordering will feel wrong.
- **Testing.** The whole crate is macOS-only, because handlers take AppKit types. The prototype
  can be type-checked from Linux (verified), but its tests need a macOS runner.
  `tests/headless.rs` keeps most logic testable without creating AppKit objects or needing the
  main thread.
- **API surface growth.** Mirroring AppKit faithfully means many node types and props. Generating
  them, or at least deriving the builders, keeps this manageable.

## 6. Recommended next steps

1. On a Mac: `cd prototypes/bloom-appkit && cargo test && cargo run --example counter` to confirm
   that the executor, target/action and stack layout behave as they type-check.
2. Fix §3.1 in bloom-core, with a headless test using a recording `ObjectModel`.
3. Promote the prototype into a workspace crate `bloom-appkit`, gated on `target_os = "macos"`.
   Add a `macos-latest` CI job. On ubuntu, `cargo clippy --target aarch64-apple-darwin -p
   bloom-appkit` works as a cheap type-check.
4. Add the window/app delegates and the next batch of controls, then tackle `NSTableView`.
