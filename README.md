# MyPad

A minimal code editor built with Rust and [Iced](https://iced.rs), featuring syntax highlighting and live Markdown preview.


## Features

- **Syntax highlighting** — powered by Syntect with auto-detection by file extension
- **Live Markdown preview** — side-by-side rendering with tables, footnotes, strikethrough, and task lists
- **Workspace tree view** — open folders and browse files in an expandable sidebar
- **File watching** — detects external changes and refreshes the tree automatically
- **Light and dark themes** — switch between Light and Tokyo Night
- **Custom window chrome** — frameless window with a built-in titlebar
- **Windows context menu** — register "Open with MyPad" in Explorer for files and directories
- **Unsupported file handling** — modal with an "Open Anyway" option for unrecognized extensions

## Supported File Types

| Category       | Extensions                              |
|----------------|-----------------------------------------|
| Languages      | `.rs` `.js` `.ts` `.php`                |
| Data & Config  | `.json` `.toml` `.yaml` `.yml`          |
| Web            | `.html` `.css`                          |
| Markup & Text  | `.md` `.txt`                            |

## Keyboard Shortcuts

| Shortcut   | Action              |
|------------|----------------------|
| `Ctrl+S`   | Save active file     |
| `Escape`   | Dismiss modal / menu |

Double-click a file in the sidebar to open it.

## Building

Requires Rust 1.85+ (edition 2024).

```
cargo build --release
```

The build script converts `icon.png` into a multi-resolution `.ico` and embeds it as a Windows resource.

## Running

```
cargo run
```

Open a specific file or folder:

```
cargo run -- path/to/file.rs
cargo run -- path/to/folder
```

## Dependencies

| Crate              | Purpose                      |
|--------------------|------------------------------|
| `iced`             | GUI framework                |
| `syntect`          | Syntax highlighting          |
| `pulldown-cmark`   | Markdown parsing             |
| `notify`           | File system watching         |
| `rfd`              | Native file dialogs          |
| `open`             | Open links in default browser|
| `windows-registry` | Explorer context menu        |

## License

This project is not yet licensed.
