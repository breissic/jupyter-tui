# jupyter_tui

A terminal-based Jupyter notebook editor written in Rust. Edit and execute `.ipynb` files directly from your terminal with vim-style keybindings.

## Features

- **Full `.ipynb` support** -- Load, edit, and save Jupyter notebooks using the standard nbformat
- **Live kernel execution** -- Start and communicate with Jupyter kernels over ZMQ; execute cells and see output inline
- **Vim-style modal editing** -- Three-level modal interface:
  - **Normal mode** for navigating between cells
  - **Cell Normal mode** for navigating within a cell using vim motions
  - **Cell Insert mode** for typing text
- **Relative line numbers** -- Displayed in the gutter when editing a cell
- **Operator-pending and Visual mode** -- `d`, `y`, `c` with motions, plus `v`/`V` visual selection inside cells
- **Kernelspec discovery** -- Automatically finds kernels via `jupyter --paths`, including pyenv installations

## Requirements

- Rust 2024 edition (1.85+)
- A Jupyter kernel installed and discoverable (e.g., `ipykernel` for Python)
- ZeroMQ system library (`libzmq`) -- required by runtimelib

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

| Key       | Action                        |
|-----------|-------------------------------|
| `j` / `k` | Move down / up between cells |
| `g` / `G` | Jump to first / last cell    |
| `i`, `Enter` | Enter selected cell (Cell Normal mode) |
| `o` / `O` | Insert new cell below / above and enter it |
| `d`       | Delete selected cell          |
| `x`       | Execute selected cell         |
| `X`       | Execute cell and move to next |
| `m`       | Toggle cell type (Code / Markdown) |
| `:`       | Enter command mode            |
| `Ctrl-s`  | Save notebook                 |
| `Ctrl-c`  | Send interrupt to kernel      |

### Cell Normal Mode (vim motions inside a cell)

| Key             | Action                          |
|-----------------|---------------------------------|
| `h j k l`       | Cursor movement                |
| `w` / `e` / `b` | Word forward / end / back      |
| `0` / `^` / `$` | Line start / first char / end  |
| `gg` / `G`      | Top / bottom of cell           |
| `i` / `a`       | Enter insert mode (before / after cursor) |
| `I` / `A`       | Insert at line start / end     |
| `o` / `O`       | New line below / above         |
| `x`             | Delete character               |
| `D` / `C`       | Delete / change to end of line |
| `dd` / `yy` / `cc` | Delete / yank / change line |
| `d{motion}` / `y{motion}` / `c{motion}` | Operator + motion |
| `p`             | Paste                          |
| `u` / `Ctrl-r`  | Undo / redo                    |
| `J`             | Join current line with next    |
| `v` / `V`       | Visual / visual line mode      |
| `Ctrl-d/u`      | Scroll half page down / up     |
| `Ctrl-f/b`      | Scroll full page down / up     |
| `:`             | Enter command mode             |
| `Esc`           | Exit cell, return to Normal mode |

### Cell Insert Mode

| Key    | Action                              |
|--------|-------------------------------------|
| `Esc`  | Return to Cell Normal mode          |
| All other keys | Standard text input          |

### Cell Visual Mode

| Key          | Action                          |
|--------------|---------------------------------|
| Motions      | Extend selection                |
| `y`          | Yank selection                  |
| `d`          | Delete selection                |
| `c`          | Change selection (delete + insert) |
| `Esc` / `v`  | Cancel selection                |

### Command Mode

| Command     | Action                                  |
|-------------|-----------------------------------------|
| `:w`        | Save notebook                           |
| `:q`        | Quit (fails if unsaved changes)         |
| `:q!`       | Quit without saving                     |
| `:wq`       | Save and quit                           |
| `:w <file>` | Save to a specific file path            |
| `:3c`       | Jump to cell 3                          |
| `:3`        | Jump to line 3 (when inside a cell)     |

## Architecture

```
src/
├── main.rs             Entry point, terminal setup/teardown, main event loop
├── app.rs              App state, Mode enum, kernel message routing
├── event.rs            Unified event loop (crossterm keys + kernel IOPub + tick)
├── input/
│   ├── handler.rs      Mode-specific key event handlers
│   └── vim.rs          CellVim state machine (motions, operators, visual)
├── kernel/
│   ├── manager.rs      Kernelspec discovery, kernel process lifecycle
│   └── client.rs       Async ZMQ client (shell, iopub, control, stdin channels)
├── notebook/
│   └── model.rs        Cell, Notebook, CellOutput types, .ipynb serialization
└── ui/
    ├── layout.rs       Full-screen layout (cells + status bar + command line)
    ├── cell.rs         Cell rendering with relative line number gutter
    ├── statusbar.rs    Mode indicator, filename, cursor position, kernel status
    └── output.rs       Output rendering (image support planned)
```

## Status

This is an early-stage project. Working:

- Notebook load/save
- Kernel lifecycle (start, execute, interrupt, shutdown)
- Cell CRUD operations
- Stream, execute_result, error, and display_data output rendering
- Vim modal editing with motions, operators, and visual mode
- Relative line numbers
- Correct output routing via Jupyter `parent_header.msg_id` correlation

Planned:

- Syntax highlighting
- Kitty graphics protocol for inline images (matplotlib plots, etc.)
- Tab completion via kernel `complete_request`
- Markdown cell rendering
- Search within cells (`/`, `?`, `n`, `N`)
- Confirm-before-quit on unsaved changes

## License

MIT
