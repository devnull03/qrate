# Contributing to qrate

Thank you for helping improve qrate. Contributions can be code, documentation, translations, design feedback, bug reports, accessibility improvements, or plugins.

## Before you start

For a bug, please open an issue with the qrate version, operating system, clear reproduction steps, what you expected, and what happened. Do not include collection data, file paths, or screenshots that contain private information.

For a larger feature or a change to how qrate works, open an issue first so the maintainers and contributors can agree on the problem and scope.

## Set up a development copy

qrate is a Rust 2024 workspace. Install the stable Rust toolchain with the `rustfmt` and `clippy` components, then clone the repository and run:

```sh
cargo run
```

The first build downloads Rust dependencies. The [`sample/`](sample) directory contains a sample collection and images for local testing. qrate builds and runs without optional preview binaries; PDF and video preview coverage needs the tools described in [the development setup guide](docs/dev/SETUP.md).

Linux contributors also need the system libraries listed in [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Google Sheets sign-in is optional for local development and needs local credentials; setup details are in [the development setup guide](docs/dev/SETUP.md).

## Make a contribution

1. Create a focused branch from the current default branch and make one coherent change.
2. Write tests when behavior changes, and update user documentation when the interface or workflow changes.
3. Use clear, accessible language and preserve people’s control over their collection data.
4. Open a pull request against `main` or `dev`, with a short summary, relevant issue links, and a **Verification** line listing the checks you ran.

Use conventional commit messages in the form `type(scope): summary`, for example `fix(grid): preserve selection after filtering`.

## Required checks

Run these from the repository root before opening a pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -A dead_code
cargo test --workspace
```

GitHub Actions runs the corresponding format, Clippy, and test checks on Windows, macOS, and Linux for pull requests targeting `main` or `dev`. It also checks that the bundled agent runtime package can be assembled on each platform and fetches PDFium so preview tests exercise real PDF rendering.

CI cancels older runs for the same branch when a newer commit arrives. A passing local run is still valuable: direct pushes to `main` do not trigger the CI workflow automatically.

## Review and maintenance

Keep pull requests small enough to review. Explain user-visible changes, note limitations, and include screenshots or a short recording for visual changes when practical. Do not commit secrets, private collection data, or generated build output.

Plugins are a public compatibility surface. If a contribution changes the plugin API, read the repository guidance in [`CLAUDE.md`](CLAUDE.md) before making the change; the API has matching type definitions in companion repositories.

## License

By contributing, you agree that your contribution is licensed under the [GNU Affero General Public License v3.0](LICENSE.md). Contributors retain copyright in their work. qrate does not require a Contributor License Agreement or copyright assignment.
