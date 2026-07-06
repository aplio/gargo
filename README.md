# Gargo

Gargo is a modal (Vim-like) terminal text editor written in Rust, with Tree-sitter syntax highlighting, git integration, and an optional browser-based editor served over HTTP.

## Install

```bash
curl -fsSL https://github.com/aplio/gargo/raw/refs/heads/master/install.sh | sh
```

Options (set as environment variables before `sh`):

- `GARGO_VERSION=v0.1.13` — install a specific version
- `GARGO_BIN_DIR=$HOME/.bin` — install to a custom directory
- `GARGO_SKIP_VERIFY=1` — skip checksum verification

Prebuilt binaries are published on [GitHub Releases](https://github.com/aplio/gargo/releases) for macOS and Linux (x86_64 / aarch64); manual install is just placing `gargo` on your `PATH`.

To build from source, see [Development](#development).

## Usage

```bash
gargo                 # empty scratch buffer
gargo path/to/file    # open a file
gargo path/to/dir     # start in a directory
gargo --check         # check for a newer release
gargo --update        # self-update to the latest release
```

Configuration is read from `~/.config/gargo/config.toml` (respects `XDG_CONFIG_HOME`).

### Basic keys

- `i` / `Esc`: enter insert mode / back to normal mode
- `Ctrl+S`: save current buffer
- `Ctrl+Q`: close current buffer, or quit when it is the last one
- `SPC f`: file picker
- `SPC p`: command palette
- `SPC /`: global search
- `SPC e`: file explorer sidebar
- `SPC g` / `SPC G`: changed-files sidebar / Git view
- `SPC w` + `v` / `s` / `q`: split window vertically / horizontally / close
- Mouse: left-drag a split border to resize panes

### Web editor

```bash
gargo --server
```

Starts an HTTP server and opens a browser-based editor whose modal core runs in-tab as WebAssembly. Flags: `--no-open` (don't launch the browser), `--port <PORT>` (default: OS-assigned), `--host <HOST>` (default `127.0.0.1`; use `0.0.0.0` to accept remote connections).

The wasm bundle is embedded into release binaries at build time, so `gargo --server` works out of the box for installed releases.

## Development

Requires the Rust stable toolchain.

```bash
cargo run -- path/to/file    # run the terminal editor
cargo test                   # run tests
```

To use the web editor from a source build, generate the wasm bundle **before** `cargo build` so it gets embedded (it lives in `assets/web_editor/pkg/`, which is gitignored):

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli   # must match the wasm-bindgen version in Cargo.lock
./scripts/build-web-editor.sh
```

If the bundle is missing, `cargo build` still succeeds and the editor's asset routes report "wasm not built".

### More docs

- [docs/README.md](docs/README.md) — architecture
- [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) — development workflow and commit message rules
