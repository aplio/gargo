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

## Installation

Quick install:

```bash
curl -fsSL https://github.com/aplio/gargo/raw/refs/heads/master/install.sh | sh
```

Install a specific version:

```bash
GARGO_VERSION=v0.1.13 curl -fsSL https://github.com/aplio/gargo/raw/refs/heads/master/install.sh | sh
```

Install to a custom directory:

```bash
GARGO_BIN_DIR=$HOME/.bin curl -fsSL https://github.com/aplio/gargo/raw/refs/heads/master/install.sh | sh
```

Checksum verification is enabled when a release includes `checksums.txt`. Set `GARGO_SKIP_VERIFY=1` to skip verification.

- Legacy/manual install still works by downloading a release tarball from [GitHub Releases](https://github.com/aplio/gargo/releases) and placing `gargo` on your `PATH`.

Supported assets:
- `gargo-v<version>-x86_64-apple-darwin.tar.gz`
- `gargo-v<version>-aarch64-apple-darwin.tar.gz`
- `gargo-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- `gargo-v<version>-aarch64-unknown-linux-gnu.tar.gz`

## Source install

```bash
cargo install --path .
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
