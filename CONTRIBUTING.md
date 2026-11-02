# Contributing

Thanks for wanting to help. Bug reports, fixes and new features are all welcome.
There is no contributor agreement to sign: under section 5 of the Apache Licence
2.0, anything you submit for inclusion is covered by the project's own licence.

What libNIMA will do, and in which order, is in [docs/PLAN.md](docs/PLAN.md).

## Reporting a bug

Open an issue and include your OS and its version, the libNIMA version (`libnima
--version`), the platform and IDE of your project, the library you installed, what
you did, what you expected and what happened instead.

## Building it

You need:

- [Rust](https://rustup.rs), stable
- [Node.js](https://nodejs.org), the LTS version, for the window
- What Tauri needs on your OS, for the window, listed at
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/):
  - Windows: Visual Studio Build Tools with "Desktop development with C++".
    WebView2 is already part of Windows 10 and 11
  - macOS: Xcode's command line tools (`xcode-select --install`)
  - Linux (Debian, Ubuntu):

    ```bash
    sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev build-essential file
    ```

The command needs none of what is only for the window:

```bash
cargo run -p libnima-cli -- --version
```

The window, in the project folder:

```bash
npm install
npm run tauri dev
```

The first build takes a few minutes. After that, a change to the UI shows up at
once, and a change to the Rust code rebuilds and restarts the app.

To make the installers for your OS, with the command in them:

```bash
npm run package
```

They end up in `target/release/bundle/`. Running them changes your PATH. The Release
workflow tries each on a machine that is thrown away afterwards, which is the place for
that.

## Layout

```
crates/libnima-core/   everything without UI: the yml, the checks, the install
crates/libnima-cli/    the libnima command
src-tauri/             the window: connects libnima-core to the UI
ui/                    the UI: Svelte and TypeScript
scripts/               what packaging runs: builds the command for the installers
docs/                  the plan, and the format of libnima.yml
```

Code that does not need the window goes into `libnima-core`, with tests, so it can
be tested without starting the app. `libnima-core` never prints and never asks.

## Making a change

1. Add or update a test in `libnima-core` that fails before your change and passes
   after it. If a change cannot be covered by a test, say why in the pull request.
2. Run the same checks CI runs:

   ```bash
   npm run check
   npm test
   npm run build
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

3. Add a line about your change under "Unreleased" in [CHANGELOG.md](CHANGELOG.md).

## What CI checks

Every pull request runs on Windows, macOS and Linux, and all three must be green:
the UI type check, tests and build, `cargo fmt`, `cargo clippy` with warnings as
errors, and the tests. On Windows it also puts the command on the PATH and takes it off
again, in the real registry, since that machine is thrown away afterwards. One more job
builds and tests the command on a machine with no web view, since that is where a build
server runs it.

## Questions

Open an issue, or reach me at nima.askari@gmail.com.

## Against lib-installer

What the engine writes into a project was held to lib-installer 4.0.1 byte for byte, apart from the name and the record file.
`tests/compare/compare.py` installs the same libraries into the same projects with both, on all five platforms and every STM32
IDE, in LF and CRLF, and says what differs. It needs Python and lib-installer on your machine, and nothing that ships does:

```bash
pip install lib-installer==4.0.1
cargo build -p libnima-cli
python tests/compare/compare.py
```

`crates/libnima-core/tests/data/pyyaml.json` is the same for the file's reader: what PyYAML read for each document, or that it
refused it, kept so the tests need no Python.
