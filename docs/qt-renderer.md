# Evaluation: a native Qt renderer for bloom

Status: research / proposal. Nothing in this document is implemented yet.

## TL;DR

A Qt renderer is feasible without major changes to `bloom-core`. The `ObjectModel` trait
already has the right shape for an imperative, retained-mode widget toolkit, and `bloom-rsx`
is renderer-agnostic: it only needs a `tag` function and a builder exposing
`attr` / `on` / `dom_ref` / `build` / `children`.

Most of the work sits outside bloom:

1. **Rust ↔ Qt bindings.** No maintained Rust crate wraps QtWidgets. CXX-Qt (KDAB) is the
   active project, but it targets QObject subclasses and QML, not the widget classes. The
   practical route is a small hand-written C++ shim. If it drives widgets generically through
   Qt's meta-object system (`setProperty`, signals by name), the FFI surface stays at roughly
   a dozen functions.
2. **Event-loop integration.** Bloom's `render_loop` is an async future. Qt requires all widget
   access on the thread that runs `QApplication::exec()`. The render loop has to be polled on
   the GUI thread by an executor whose waker posts to the Qt event loop.
3. **Widget semantics.** HTML lets any element hold any children and free text. Qt does not.
   Each container class needs its own child-insertion strategy: box layouts,
   `QMainWindow::setCentralWidget`, `QTabWidget::insertTab`, `QScrollArea::setWidget`,
   actions on menus, and so on. Bare text must become a `QLabel` or a property of its parent.

Estimate: an MVP (about 10 widget types, box layouts, signals, refs, an example app, headless
tests in CI) is roughly **3–5 person-weeks** for someone comfortable with both Rust and Qt/C++.
A production-grade renderer (menus, dialogs, item views/models, styling, accessibility,
packaging on all three desktop OSes) is a matter of **months**.

## 1. What bloom asks of a renderer

The whole renderer contract is `bloom_core::ObjectModel` (`bloom-core/src/render_loop.rs`):

| Method | Called when | Qt meaning |
|---|---|---|
| `create(node, parent, sibling)` | a node is mounted. Parents are created before children. `sibling` is the next sibling (insert-before semantics) or `None` (append). | Create the QObject, apply its properties, connect its signals, insert it into the parent at `index_of(sibling)`. |
| `update(current, next)` | a node at the same position is re-rendered | Diff properties and signal connections. If the widget class changed, replace the widget. Re-key the bookkeeping from `current` to `next`. |
| `remove(node, parent)` | a node is unmounted. **Also called for every descendant**, with the removed node as `parent`. | Detach the widget and schedule deletion. Descendant calls must not touch already-deleted objects (see §5). |
| `start()` / `finalize()` | around each render pass. `finalize` runs **after** effects. | `finalize` is the natural place to flush deferred deletions and to yield one event-loop turn. |
| `subscribe(signal)` | once at startup | Lets the host force a re-render (e.g. on theme or DPI change). |

Other constraints that matter:

- **Node identity is the `Arc` pointer.** `bloom-client` keeps live DOM objects in a
  `PtrWeakKeyHashMap<Weak<HtmlNode>, NodeState>`. A Qt object model would do the same with
  widget pointers.
- **`N: From<String> + Send + 'static`.** Node values travel inside `Send` component
  futures. Node values therefore have to be plain descriptions (class name, properties,
  handlers), never live `QWidget*`s. This is the same split `HtmlNode` versus `web_sys::Node`
  already uses. Event handlers must be `Fn(..) + Send + Sync`.
- **Effects run on the render-loop thread**, right before `finalize`
  (`render_queue.run_effects()`). If the render loop runs on the GUI thread, a `use_effect`
  can safely dereference a Qt ref. That is the main reason to prefer the GUI-thread design
  in §4.
- **Children are diffed by position only** (`update_children`; no keys or move operation).
  This is not Qt-specific, but reordering a list costs many property updates. With widgets
  that is noticeably more expensive than with the DOM. Keyed children in core would help
  every renderer.
- **`rsx!` lowering** (`bloom-rsx/src/lib.rs`):
  - `<tag a=v on_x=f ref=r>` becomes `tag("tag").attr("a", v).on("x", f).dom_ref(r).build()`.
  - A bare attribute becomes `.attr("a", true)`.
  - Text children are `.to_string().into()`.

  Because `attr` is defined by the node crate, a Qt builder can take `V: Into<QtValue>`.
  That naturally supports `bool`, integers, floats and strings. The `dom_ref` name is a
  small HTML leak, but it is harmless.

## 2. Binding options

| Option | QtWidgets | Qt Quick / QML | Status | Verdict |
|---|---|---|---|---|
| **Hand-written C++ shim + [`cxx`](https://cxx.rs)** | ✅ whatever we write | ✅ | we own it | **Recommended** for the FFI layer |
| **[CXX-Qt](https://github.com/KDAB/cxx-qt)** (KDAB) | ❌ no widget bindings | ✅ main focus | active (0.8.x in 2026), API still changing | Use for its **build tooling** (`cxx-qt-build`: Qt discovery, moc, linking) and `CxxQtThread` queuing. Do not rely on it for widgets. |
| **Qt Bridges for Rust** (The Qt Company) | ? | ✅ | public beta, recently announced "CXX-Qt compatibility" | Watch it. Not something to build on yet. |
| **ritual / `qt_widgets`** (rust-qt) | ✅ generated, unsafe | ❌ | unmaintained, Qt 5 era | No |
| **qmetaobject-rs** | ❌ | ✅ | low activity | No |

### Why a *generic* shim keeps the FFI small

Qt's meta-object system already provides reflection. Nearly everything a declarative
renderer needs can be reached through a handful of entry points:

```cpp
// bloom_qt_shim.h (sketch)
QObject* bq_create(rust::Str class_name);                    // factory: "QPushButton" -> new QPushButton
void     bq_set_property(QObject*, rust::Str name, const QVariant&);  // text, enabled, checked, placeholderText, ...
void     bq_reset_property(QObject*, rust::Str name);
uint64_t bq_connect(QObject*, rust::Str signal, uint64_t handler_id);  // generic slot -> Rust trampoline(handler_id, args)
void     bq_disconnect(uint64_t connection);
void     bq_insert_child(QObject* parent, QObject* child, int index);  // per-class strategy table lives here
void     bq_remove_child(QObject* parent, QObject* child);
void     bq_delete_later(QObject*);
void     bq_post_to_gui_thread(uint64_t task_id);                      // for the executor waker
```

- **Properties.** `QObject::setProperty` covers every `Q_PROPERTY` (`text`, `checked`,
  `value`, `enabled`, `toolTip`, `styleSheet`, …). New widgets therefore need a factory entry
  but no new FFI.
- **Signals.** Connect by name using `QMetaObject::indexOfSignal` and a single generic
  receiver that packs arguments into a `QVariantList` and calls back into Rust with a handler
  id. This is the same trick QML uses.
- **Portability.** The same shim works for Qt Quick items. That keeps a QML backend open
  without a second FFI layer.

Typed, per-widget Rust APIs (e.g. a `button()` builder with a `text` method) can be layered
on top later, as `bloom-html/src/tag.rs` does for HTML.

## 3. QtWidgets vs Qt Quick as the target

| | QtWidgets | Qt Quick (+ Controls) |
|---|---|---|
| Look | Native platform style out of the box | Native-ish styles exist in Qt 6; usually custom-styled |
| Model | Imperative tree with explicit layouts — closest to the DOM | Scene graph with anchors/Layouts; children order via `setParentItem`/`stackBefore` |
| Dynamic creation | Plain `new QFoo(parent)` | Needs a `QQmlEngine`; create via `QQmlComponent` per type |
| Text / styling | `QLabel`, stylesheets | `Text` items, QML styling, animations |
| Fit for "true native GUI" pitch | ✅ strongest | ⚠️ better for custom/animated UIs |

**Recommendation: start with QtWidgets.** It matches bloom's stated goal of native
cross-platform GUIs and needs no QML engine. Insertion semantics map almost 1:1 to
`insert_before`. Keep the shim generic so a Quick backend can follow.

## 4. Threading and event-loop integration

Qt widgets may only be touched on the GUI thread, and that thread is blocked in
`QApplication::exec()`. Two designs:

**A. Render loop on the GUI thread (recommended).**

- Wrap `render_loop(...)` in a tiny single-future executor.
- The waker calls `bq_post_to_gui_thread`, i.e. `QMetaObject::invokeMethod(app, …,
  Qt::QueuedConnection)`. The next poll then happens inside Qt's event loop. CXX-Qt's
  `CxxQtThread::queue` does the same thing if we use its build anyway.
- Component futures are `Send`, so the `Spawn` implementation can be a
  `futures_executor::ThreadPool` or a tokio runtime. Data fetching does not block the UI.
- Qt signal handlers run on the GUI thread and call `State::update`. That sends on bloom's
  channel, which wakes the render loop, which posts a poll. This is the same flow as the
  browser.
- Effects run on the GUI thread, so `QtRef::get()` in `use_effect` is sound.
- `finalize()` can simply return `ready(())`. The bounded(1) signal channel already
  coalesces bursts of updates into one render pass. An optional "yield one event-loop turn"
  future gives Qt a chance to paint, which is the equivalent of `requestAnimationFrame` in
  `bloom-client`.

**B. Render loop on a worker thread, command queue to the GUI thread** (React-Native-bridge
style).

- The executor is simpler, but every operation must be serialized and marshaled.
- Refs and effects can no longer touch widgets.
- `remove`/`create` ordering bugs become cross-thread bugs.
- Not recommended.

A practical consequence of design A: the `ObjectModel` holds raw `QObject*`s, so it is
`!Send`, and so is the `render_loop` future. That is fine because it never leaves the GUI
thread. The `start`/`finalize` futures must still be `Send` because of the trait signature,
so they must not capture `self`.

## 5. Mapping problems specific to Qt

These cover most of the per-widget work:

1. **Text nodes.** `rsx!(<button>"OK"</button>)` produces a `Text` child of the button.
   The object model needs a per-parent rule:
   - For text-bearing widgets (`QPushButton`, `QLabel`, `QCheckBox`, `QGroupBox` title, …),
     *fold* text children into the `text` property.
   - Everywhere else, materialize a `QLabel`.

   Folding means `update` on a text node writes to its parent's property, so the object model
   must remember each text node's parent.
2. **Containers need layouts.** A bare `QWidget` does not arrange its children. Options:
   - Explicit layout nodes (`<vbox>`, `<hbox>`, `<grid>`, `<form>`) that the object model
     turns into `QBoxLayout` and friends.
   - A `layout="vbox"` property on containers. This is simpler for users but hides a real
     Qt object.

   Insertion uses `QBoxLayout::insertWidget(index_of(sibling), w)`. Stretch, spacing and
   alignment become properties of the layout node. Grid and form layouts need per-child
   attributes (`row`, `column`, `label`), which `HtmlElement` has no notion of. They can live
   in the child's property map under a reserved prefix.
3. **Non-widget children.**
   - `QAction` in `QMenu`/`QToolBar` is added through `insertAction`.
   - `QTabWidget` pages take a title, via `insertTab(index, w, title)`.
   - `QMainWindow` takes `setCentralWidget`, `setMenuBar` and `addDockWidget`.
   - `QScrollArea` and `QSplitter` have their own APIs.

   This is a strategy table keyed by the parent's class, in the shim.
4. **Removal of subtrees.** `render_loop` calls `remove` for a node and then for each of its
   descendants. If the object model `deleteLater`s the top widget, Qt deletes the children,
   and the descendant `remove` calls must not touch them. Approach:
   - On `remove`, detach the top node (`setParent(nullptr)` or remove it from the layout) and
     queue it for deletion.
   - For descendants whose parent is already queued, only drop the bookkeeping.
   - Flush `deleteLater` in `finalize`.

   Guard pointers with `QPointer` in debug builds.
5. **Updates that change the class.** `update` may see `QPushButton` become `QLabel` at the
   same position. In that case, replace the widget at the same layout index, as
   `bloom-client`'s `Dom::update` does with `replace_child`.
6. **Controlled inputs.** Setting `QLineEdit::text` on every render moves the cursor and
   re-emits `textChanged`. The object model should skip `setProperty` when the value is
   unchanged, and block signals while it applies props (`QSignalBlocker`). Otherwise a
   controlled `<QLineEdit text=… on_textChanged=…>` feeds back into itself.
7. **Signal arguments.** HTML handlers receive a `web_sys::Event`. Qt signals have typed
   argument lists. The MVP can pass `QtEvent(Vec<QtValue>)` with accessor helpers. Typed
   handlers per signal can come later through tag-specific builders.
8. **Item views.** `QListView`, `QTableView` and `QTreeView` are driven by
   `QAbstractItemModel`, not child widgets. Rendering `n` children into a `QListWidget` works
   for small lists. Real lists need a model adapter, which is a separate project.
9. **Windows and root.** Mirror `bloom_client::render(root, element)` with
   `bloom_qt::render(root: &QWidget, element)`. A `<window>` node type for multi-window apps
   can come later.

## 6. Proposed crate layout

```
bloom-qt-nodes/   QtNode description types, `tag` + `prelude`, QtRef — pure Rust, no Qt needed
bloom-qt/         ObjectModel impl, executor, `render()`, C++ shim, build.rs (cxx-qt-build or cc + qmake/pkg-config)
bloom-qt-example/ counter + form example
```

This mirrors `bloom-html` / `bloom-client`. Keeping the node types Qt-free means components
can be compiled and unit-tested, and even rendered by a test object model, without a Qt
installation.

```rust
pub enum QtNode {
    Object(Arc<QtObject>),
    Text(String),
}

pub struct QtObject {
    class: &'static str,                       // "QPushButton", "vbox", ...
    props: HashMap<String, QtValue>,           // Bool | Int | Float | String | ...
    signals: HashMap<String, SignalHandler>,   // Box<dyn Fn(QtEvent) + Send + Sync>
    qt_ref: Option<Arc<QtRef>>,                // same thread-local id trick as bloom_html::DomRef
}
// PartialEq like HtmlElement: never equal if any handler is set.
```

## 7. Build, CI and licensing

- **Toolchain.** Users need Qt 6 dev packages and a C++17 compiler. `cxx-qt-build` handles
  `qmake` discovery, moc and linker flags, and is the least painful option. Plain
  `cc` + `pkg-config` also works on Linux but is fragile on macOS and Windows.
- **Workspace CI.** `.github/workflows/ci.yml` runs `cargo clippy --workspace` and
  `cargo test --workspace`. Adding `bloom-qt` as a workspace member breaks those on runners
  without Qt. Either:
  - exclude it from `default-members` and give it its own job (`apt install qt6-base-dev` or
    `jurplel/install-qt-action`), or
  - gate the build behind a feature.

  Headless tests run with `QT_QPA_PLATFORM=offscreen`.
- **Licensing.** Qt is LGPLv3 or commercial. An MIT bloom crate that links Qt dynamically is
  fine. Apps that statically link Qt take on LGPL obligations. Document this in the crate
  README.

## 8. Suggested plan

| Milestone | Scope | Estimate |
|---|---|---|
| M0 spike | build.rs + shim with `create`/`set_property`/`insert`/`connect`. Render a static `rsx!` tree into a `QWidget` with a `QVBoxLayout`. | 3–5 days |
| M1 interactive | GUI-thread executor and waker. Signals to `State::update`. Counter example works. | 3–5 days |
| M2 correctness | `update`/`remove` semantics from §5 (text folding, subtree removal, class change, signal blocking). Object-model unit tests mirroring `bloom-client/src/dom.rs` tests, run offscreen. | 4–6 days |
| M3 widget set | Layout nodes (box/grid/form), ~10 widgets (label, button, line edit, checkbox, combo box, spin box, slider, group box, scroll area, tab widget), `QtRef`. Example app. | 5–8 days |
| M4 CI and docs | Separate CI job, README, licensing note | 1–2 days |

Optional `bloom-core` improvements that help this renderer but are not blockers:

- Keyed children or a `move` operation on `ObjectModel`.
- A doc comment on `ObjectModel` stating that `remove` is invoked for every descendant and
  that effects run before `finalize`.

## Open decisions

- **Widgets first or Quick first?** This document recommends Widgets.
- **Layout API.** Explicit layout nodes (`<vbox>`) or a `layout` property on containers.
- **Signal handler arguments.** Untyped `QtEvent` for the MVP, or typed handlers from day one.
- **Binding ownership.** Own the C++ shim, or wait for Qt Bridges for Rust to stabilize.
