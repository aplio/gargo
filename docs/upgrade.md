# Upgrade CLI

## Commands

```
gargo --check
gargo --update
```

- `--check` prints whether a newer release is available.
- `--update` downloads and replaces the current `gargo` binary.

Both commands exit before terminal raw-mode startup.

## Release Source

- GitHub repository: `aplio/gargo`
- Releases are read from GitHub Releases.

## Supported Platforms

- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Other OS/arch combinations return an unsupported-platform error.

## Asset Naming Contract

Release assets must use this exact format:

```
gargo-{target}.tar.gz
```

Examples:

- `gargo-x86_64-apple-darwin.tar.gz`
- `gargo-aarch64-apple-darwin.tar.gz`
- `gargo-x86_64-unknown-linux-gnu.tar.gz`
- `gargo-aarch64-unknown-linux-gnu.tar.gz`

The archive must include a `gargo` executable.

## Test Mode

E2E tests use a deterministic mock source:

- `GARGO_TEST_UPDATE_SOURCE=mock`
- `GARGO_TEST_UPDATE_STATE=up_to_date|has_update|error`

This avoids network dependency in CI while keeping the CLI flow testable end-to-end.
