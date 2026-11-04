# bloom

Bloom is react-like UI framework in rust.

This is a proof of concept for a react-like diffing rendering model with an abstract core allowing
for diverse rendering backends from HTML for browsers through native UI frameworks in order
to build true native cross platform GUI applications.

It is split into the following packages:

* [bloom-core](./bloom-core/README.md) is the core package including the diffing logic and core traits.
* [bloom-html](./bloom-html/README.md) provides node definitions for working with HTML rendering targets.
* [bloom-client](./bloom-client/README.md) is a reference implementation for using bloom on web-clients using web assembly.
* [bloom-gtk](./bloom-gtk/README.md) renders bloom components to native GTK4 widgets.
* [bloom-ssr](./bloom-ssr/README.md) is a reference implementation for server-side rendering, both to static strings and as streaming responses.
* [bloom-rsx](./bloom-rsx/README.md) provides the procedural `rsx!` macro used to define views with bloom.
* [bloom-macro](./bloom-macro/README.md) provides some additional macros to simplify defining components.
* [async-context](./async-context/README.md) allows providing context to async rust functions building the foundation of bloom's runtime hooks and allowing bloom-components (in contrast to react components) to be truly async.
* [bloom-client-example](./bloom-client-example/README.md) is an example implementation of an SPA (runnning completely in the browser on web assembly) with bloom.
* [bloom-gtk-example](./bloom-gtk-example/README.md) is a todo app built with native GTK4 widgets.
* [bloom-server-example](./bloom-server-example/README.md) is an example implementation of using bloom to render html on the server.

## Releases

[Knope](https://knope.tech/) manages two release groups: the six `bloom-*`
libraries share the workspace version, and `async-context` has its own version.
The example applications have `publish = false`.

For a change that should ship, run `knope document-change` (Knope 0.23.2), select
`bloom` and/or `async-context`, and commit the generated `.changeset/*.md` file
with the code. You can also write one directly:

```markdown
---
bloom: patch
async-context: patch
---

Describe the user-visible change here.
```

Omit groups that did not change. Use `patch`, `minor`, or `major` as appropriate.
If an `async-context` change requires Bloom to use a new incompatible dependency
version, include both groups. Conventional commits alone do not trigger bumps.
Knope updates package versions, workspace dependency requirements, and lockfile
entries together, and writes `CHANGELOG.md` and `async-context/CHANGELOG.md`.
Tags are `bloom/v<version>` and `async-context/v<version>`.

Repository setup (once):

- In **Settings → Actions → General → Workflow permissions**, enable
  **Allow GitHub Actions to create and approve pull requests**. The workflows
  use the default `GITHUB_TOKEN`; no personal access token is required.
- In each crate's crates.io settings, add a **GitHub trusted publisher** with
  owner `mismosmi`, repository `bloom`, and workflow filename `release.yml`.
  Leave the environment field empty, since the publishing job does not use a
  GitHub environment. Configure all seven libraries: `async-context`,
  `bloom-macro`, `bloom-rsx`, `bloom-core`, `bloom-html`, `bloom-ssr`, and
  `bloom-client`.
- [Trusted publishing](https://crates.io/docs/trusted-publishing) requires the
  crate to exist already. For any new crate, publish its first version locally
  before registering its trusted publisher.

The publishing job exchanges its GitHub OIDC identity for a temporary crates.io
access token immediately before uploading. No `RELEASE_TOKEN` or `CARGO_TOKEN`
repository secrets are needed.

**Prepare releases** runs automatically on pushes to `main` and creates or
updates the `release` PR when `.changeset/*.md` files are pending. It skips
preparation when no changes remain, including after a release PR is merged.
You can also run it manually from the Actions tab on `main`.
Because the PR uses `GITHUB_TOKEN`, a maintainer
with write access must select **Approve workflows to run** on the PR before its
CI runs can start ([GitHub documentation](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)).
Review the versions and changelogs, wait for CI, and merge the PR manually. **Publish releases** tests the libraries and doctests, publishes
unpublished versions in dependency order, waits for crates.io availability
between uploads, then uses Knope to create the GitHub releases. It publishes
the exact merge commit, even when rerun later.

If publishing fails partway through, use **Re-run failed jobs** on that run.
Already published versions are skipped, and GitHub releases are created only
after publishing succeeds. A prepared release cannot overwrite a version
already uploaded to crates.io. Do not merge another release PR until the
previous release finishes successfully.

For a local preview that changes no files, run:

```sh
knope --validate
knope prepare-release --dry-run
```
