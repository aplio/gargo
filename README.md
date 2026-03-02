# Gargo

Gargo is a terminal text editor written in Rust.

## Requirements

- Rust stable toolchain
- A terminal with true color support

## Quick start

Run with an empty scratch buffer:

```bash
cargo run
```

Open a file or directory:

```bash
cargo run -- path/to/file_or_directory
```

Run optimized build:

```bash
cargo run --release -- path/to/file
```

## Basic keys

- `i`: enter insert mode
- `Esc`: return to normal mode
- `Ctrl+S`: save current buffer
- `Ctrl+Q`: close current buffer, or quit when it is the last one
- `SPC f`: open file picker
- `SPC p`: open command palette
- `SPC g`: open flat changed-files sidebar with status badges
- `SPC G`: open Git view
- Mouse: left-drag an editor split border to resize pane widths/heights

## More docs

- `docs/README.md` for architecture
- `docs/CONTRIBUTING.md` for development workflow
