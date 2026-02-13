# jupyter_tui

A terminal-based Jupyter notebook editor written in Rust. Edit and execute `.ipynb` files directly from your terminal with vim-style keybindings.

## Features

- **Full `.ipynb` support** -- Load, edit, and save Jupyter notebooks using the standard nbformat
- **Live kernel execution** -- Start and communicate with Jupyter kernels over ZMQ; execute cells and see output inline
- **Syntax highlighting** -- Code cells are highlighted using syntect (base16-ocean.dark theme), both when viewing and editing
- **Vim-style modal editing** -- Three-level modal interface:
  - **Normal mode** for navigating between cells
  - **Cell Normal mode** for navigating within a cell using vim motions
  - **Cell Insert mode** for typing text
  - **Cell Visual mode** for selecting and operating on text
- **Count prefixes** -- Vim-style numeric prefixes work throughout: `3j` moves 3 cells, `2dd` deletes 2 lines, `5w` moves 5 words, `3G` jumps to cell 3, etc.
- **Search** -- `/` and `?` for forward/backward search with `n`/`N` repeat; works both within cells (tui-textarea search with yellow match highlighting) and across cells (cross-cell navigation from Normal mode with all matches highlighted)
- **Relative line numbers** -- Displayed in the gutter when editing a cell
- **Operator-pending and Visual mode** -- `d`, `y`, `c` with motions, plus `v`/`V` visual selection inside cells
- **Tab completion** -- Kernel-powered tab completion with a bottom panel UI; navigate with Tab/Shift-Tab/Up/Down, apply with Enter, dismiss with Esc
- **Inline image rendering** -- Kitty graphics protocol support for displaying `image/png` and `image/jpeg` outputs (matplotlib plots, PIL images, etc.) directly in the terminal
- **Markdown cell rendering** -- "Execute" a markdown cell to render it as formatted text (headings, bold, italic, lists, code blocks, blockquotes, etc.); enter the cell to switch back to raw source for editing
- **ANSI escape code rendering** -- Cell outputs with ANSI colors (tracebacks, rich output, progress bars) are rendered correctly
- **Confirm-before-quit** -- `:q` warns when there are unsaved changes; use `:q!` to force-quit
- **Kernelspec discovery** -- Automatically finds kernels via `jupyter --paths`, including pyenv installations
- **Cell operations** -- Move, yank, paste, delete, and reorder cells with vim-style keys

## Requirements

- Rust 2024 edition (1.85+)
- A Jupyter kernel installed and discoverable (e.g., `ipykernel` for Python)
- ZeroMQ system library (`libzmq`) -- required by runtimelib
- A Kitty graphics protocol compatible terminal (Kitty, Ghostty) for inline image rendering

### Installing prerequisites

```sh
# Install ipykernel (provides the python3 kernel)
pip install ipykernel

# On Debian/Ubuntu, install libzmq if needed
sudo apt install libzmq3-dev

# On macOS
brew install zmq
```

## Building

```sh
cargo build --release
```

## Usage

```sh
# Open an existing notebook
jupyter_tui path/to/notebook.ipynb

# Create a new notebook
jupyter_tui new_notebook.ipynb

# Start with an empty untitled notebook
jupyter_tui
```

## Keybindings

### Normal Mode (cell navigation)

| Key          | Action                                            |
|--------------|---------------------------------------------------|
| `j` / `k`   | Move down / up between cells (accepts count: `3j`) |
| `g` / `G`   | Jump to first / last cell (`3G` jumps to cell 3)  |
| `i`, `Enter` | Enter selected cell (Cell Normal mode)            |
| `Shift-Enter` | Execute selected cell (stay in Normal mode)      |
| `o` / `O`   | Insert new cell below / above and enter it        |
| `d`          | Delete selected cell (yanks to buffer)            |
| `y`          | Yank (copy) selected cell                         |
| `p` / `P`   | Paste yanked cell below / above                   |
| `x`          | Execute selected cell                             |
| `X`          | Execute cell and move to next                     |
| `J` / `K`   | Move cell down / up (reorder)                     |
| `m`          | Toggle cell type (Code / Markdown)                |
| `/` / `?`   | Search forward / backward across cells            |
| `n` / `N`   | Repeat search forward / backward                  |
| `Esc`        | Clear search highlights                           |
| `:`          | Enter command mode                                |
| `Ctrl-s`    | Save notebook                                     |
| `Ctrl-c`    | Send interrupt to kernel                          |

### Cell Normal Mode (vim motions inside a cell)

| Key                | Action                               |
|--------------------|--------------------------------------|
| `h j k l`          | Cursor movement                      |
| `w` / `e` / `b`   | Word forward / end / back            |
| `0` / `^` / `$`   | Line start / first char / end        |
| `gg` / `G`         | Top / bottom of cell                 |
| `i` / `a`          | Enter insert mode (before / after cursor) |
| `I` / `A`          | Insert at line start / end           |
| `o` / `O`          | New line below / above               |
| `x`                | Delete character                     |
| `D` / `C`          | Delete / change to end of line       |
| `dd` / `yy` / `cc` | Delete / yank / change line         |
| `d{motion}` / `y{motion}` / `c{motion}` | Operator + motion |
| `p`                | Paste                                |
| `u` / `Ctrl-r`     | Undo / redo                         |
| `J`                | Join current line with next          |
| `v` / `V`          | Visual / visual line mode            |
| `Ctrl-d/u`         | Scroll half page down / up           |
| `Ctrl-f/b`         | Scroll full page down / up           |
| `/` / `?`          | Search forward / backward within cell |
| `n` / `N`          | Repeat search within cell            |
| `Shift-Enter`      | Execute cell and exit to Normal mode |
| `:`                | Enter command mode                   |
| `Esc`              | Exit cell, return to Normal mode     |

### Cell Insert Mode

| Key          | Action                                |
|--------------|---------------------------------------|
| `Esc`        | Return to Cell Normal mode            |
| `Tab`        | Request tab completion from kernel    |
| `Shift-Tab`  | Cycle backward through completions   |
| `Up` / `Down` | Navigate completion list             |
| `Enter`      | Apply selected completion (when completions shown) |
| `Shift-Enter` | Execute cell and exit to Normal mode |
| All other keys | Standard text input                 |

### Cell Visual Mode

| Key          | Action                              |
|--------------|-------------------------------------|
| Motions      | Extend selection                    |
| `y`          | Yank selection                      |
| `d`          | Delete selection                    |
| `c`          | Change selection (delete + insert)  |
| `Shift-Enter` | Execute cell and exit              |
| `Esc` / `v`  | Cancel selection                   |

### Command Mode

| Command        | Action                                  |
|----------------|-----------------------------------------|
| `:w`           | Save notebook                           |
| `:q`           | Quit (fails if unsaved changes)         |
| `:q!`          | Quit without saving                     |
| `:wq`          | Save and quit                           |
| `:w <file>`    | Save to a specific file path            |
| `:3c`          | Jump to cell 3                          |
| `:3`           | Jump to line 3 (when inside a cell)     |
| `:run-all` / `:ra` | Execute all cells (code + render markdown) |
| `:restart`     | Restart the kernel                      |
| `:restart!`    | Restart kernel and run all cells        |
| `:interrupt`   | Send interrupt signal to kernel         |

## Markdown Cells

Markdown cells have two display states:

- **Raw source** -- The default view. Shows the markdown source as plain yellow text for editing.
- **Rendered** -- After "executing" a markdown cell (`x`, `Shift-Enter`, or `:run-all`), the source is rendered as formatted text with styled headings, bold/italic, lists, code blocks, blockquotes, and more.

Enter a rendered markdown cell (`i` or `Enter`) to switch back to raw source for editing. Toggling cell type with `m` also resets to raw source.

Rendered markdown supports: headings, bold, italic, strikethrough, ordered/unordered/nested/task lists, blockquotes, fenced code blocks with syntax highlighting, links, horizontal rules, superscript/subscript, and metadata blocks. LaTeX math is shown as raw text. Images in markdown are not yet rendered.

## Inline Images

Cell outputs containing `image/png` or `image/jpeg` data (e.g., matplotlib plots) are rendered inline using the Kitty graphics protocol. Images are decoded from base64, scaled to fit the terminal width without upscaling, and cached for efficient re-rendering. Requires a compatible terminal (Kitty, Ghostty).

For `DisplayData` outputs, images are preferred over `text/plain`. For `ExecuteResult` outputs, both text and image are shown.

## Architecture

```
src/
├── main.rs             Entry point, terminal setup/teardown, main event loop
├── app.rs              App state, Mode enum, kernel message routing, cell operations
├── event.rs            Unified event loop (crossterm keys + kernel IOPub + tick)
├── input/
│   ├── handler.rs      Mode-specific key event handlers, commands, search, completion
│   └── vim.rs          CellVim state machine (motions, operators, counts, visual)
├── kernel/
│   ├── manager.rs      Kernelspec discovery, kernel process lifecycle
│   └── client.rs       Async ZMQ client (shell, iopub, control, stdin channels)
├── notebook/
│   └── model.rs        Cell, Notebook, CellOutput types, .ipynb serialization
└── ui/
    ├── layout.rs       Full-screen layout (cells + completion panel + status bar + command line)
    ├── cell.rs         Cell rendering, syntax highlighting overlay, search highlights,
    │                   inline image rendering (Kitty protocol), markdown rendering
    ├── statusbar.rs    Mode indicator, filename, cursor position, kernel status
    ├── highlight.rs    Syntect-based syntax highlighting engine
    └── output.rs       (placeholder -- output rendering lives in cell.rs)
```

## Status

### Working

- Notebook load/save (.ipynb via nbformat)
- Kernel lifecycle (start, restart, shutdown, interrupt, execute)
- Cell CRUD operations (create, delete, move, yank, paste, reorder)
- Stream, execute_result, error, and display_data output rendering
- ANSI escape code rendering in outputs (ansi-to-tui)
- Vim modal editing with motions, operators, counts, and visual mode
- Relative line numbers in editing gutter
- Syntax highlighting (syntect, base16-ocean.dark, post-render buffer overlay)
- Cross-cell search with match highlighting (`/`, `?`, `n`, `N`)
- In-cell search with tui-textarea integration
- Tab completion via kernel `complete_request` with bottom panel UI
- Inline image rendering via Kitty graphics protocol (image/png, image/jpeg)
- Markdown cell rendering via tui-markdown (headings, bold, italic, lists, code blocks, etc.)
- Correct output routing via Jupyter `parent_header.msg_id` correlation
- Confirm-before-quit on unsaved changes
- Dirty tracking for notebook modifications

### Planned

- More vim text objects (`ciw`, `diw`, `ci"`, `ca(`, etc.)
- Markdown image rendering (local files, data URIs via Kitty protocol)
- Better notebook file management built-in

## License

MIT
