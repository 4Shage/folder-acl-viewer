[![Manual Release Build](https://github.com/4Shage/folder-acl-viewer/actions/workflows/release.yml/badge.svg)](https://github.com/4Shage/folder-acl-viewer/actions/workflows/release.yml)
# Folder ACL Viewer

A desktop GUI (Rust, `egui`/`eframe`) for browsing large, pipe-delimited
folder-permission (ACL) exports — search by folder, user, or sub-path, and
drill into a permissions tree instead of scrolling a giant spreadsheet.
Built to comfortably handle multi-million-row exports.

## Expected CSV format

Pipe-delimited (`|`), with a header row:

```
Folder|Identity|Rights|AccessControl|Inherited
```

`Folder` is expected to be a full path (typically a UNC path like
`\\server\share\dept\team\...`). On load it's split into two columns:

- **Folder** — the first N backslashes of the path (a share/department root)
- **Other** — everything past that

See "Split behavior" below for how the cut point is chosen.

## Features

- **Fast loading** — the file is read and parsed on a background thread, so
  the UI never blocks. Repeated field values (folder paths, rights,
  identities) are interned into shared `Arc<str>` allocations instead of
  duplicated per row, which keeps multi-million-row files fast and
  reasonably memory-light.
- **Search by Folder, User, or Other** — three tabs in the sidebar, each
  backed by an indexed (`O(1)`) lookup, so selecting an item stays instant
  regardless of dataset size. Typing in the search box filters the current
  tab's list.
- **Permissions tree** — selecting a folder, user, or sub-path renders a
  tree of everywhere that grantee/path applies, with Identity, Rights,
  AccessControl, and Inherited shown per entry. The tree is virtualized
  (only visible rows are built each frame), so it stays smooth even fully
  expanded with thousands of rows.
- **Copy path** — a 📋 button on every folder node (and the tree's root
  heading, for the Folder tab) copies that folder's full reconstructed path
  to the clipboard.
- **Sortable full table** — before selecting anything, the main view is a
  sortable table of every record; click a column header to sort, click again
  to reverse.

## Load Options (⚙ button)

- **Exclude identities** — a newline-separated list of identities to drop
  entirely while loading (defaults to a few noisy built-ins:
  `BUILTIN\Administrators`, `CREATOR OWNER`, `NT AUTHORITY\SYSTEM`).
- **Split depth** — how many leading backslashes go into `Folder` before the
  rest falls into `Other` (default 4, e.g. `\\server\share\` for a typical
  UNC path — the two backslashes of the UNC prefix each count).
- **Automatic** — when enabled, the split depth above becomes a *floor*
  instead of a fixed cut. Past that floor, the split keeps extending through
  any prefix that's still granted to *every* identity in the dataset (i.e.
  a folder everyone can see isn't interesting on its own), and stops the
  moment a prefix's grants stop covering everyone — that boundary is, by
  definition, a folder that isn't shared between all users. This avoids
  both extremes: a single folder that swallows the entire share, and a
  folder for every individual row.

Both settings apply the next time you open a CSV, not retroactively.

## Building

Requires a recent stable Rust toolchain (edition 2024).

```sh
cargo build --release
```

On Linux, `eframe`/`rfd` need GTK3 and X11/xkbcommon development headers to
build (e.g. on Debian/Ubuntu: `libgtk-3-dev libxcb-render0-dev
libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev`).

The app uses `eframe`'s `glow` (OpenGL) renderer rather than the default
`wgpu` backend, for broader compatibility with limited/embedded GPU drivers.

## Running

```sh
cargo run --release [path/to/export.csv]
```

The CSV path is optional — you can also load a file from within the app via
**📂 Open CSV...**.

## Project layout

- `src/main.rs` — entry point, window setup, tokio runtime
- `src/app.rs` — `eframe::App` implementation: UI, state, selection logic
- `src/loader.rs` — async CSV loading, parsing, interning, and indexing
- `src/model.rs` — `Record` type, folder-splitting, and permissions-tree
  data structures
- `src/style.rs` — visual theme
