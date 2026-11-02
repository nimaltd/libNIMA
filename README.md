# libNIMA

[![CI](https://github.com/nimaltd/libNIMA/actions/workflows/ci.yml/badge.svg)](https://github.com/nimaltd/libNIMA/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE.md)

Install embedded C libraries into STM32, Zephyr, ESP-IDF, Arduino and PlatformIO
projects, from a window or from the `libnima` command, the same on Windows, macOS
and Linux.

libNIMA is being written to replace [lib-installer](https://github.com/nimaltd/lib-installer).
Where it stands is in [docs/PLAN.md](docs/PLAN.md), and how to build it is in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Using it

In the window: open your project's folder, and libNIMA says which platform it is and
which file told. Add a library by its repository, a zip or a folder, and it joins the
list to install, marked with what it found in your project. **Preview** shows what an
install would change, tried on a copy of the project. **Install all** does it.

From the root of a project, the command does the same:

```bash
libnima nimaltd/ee24
```

`libnima --help` lists the rest. The installers put `libnima` on the PATH. The command
also comes alone, as a zip for a build server or a container: unpack it where it can
stay, and run `libnima --add-to-path`.

A library says how it goes in with a [`libnima.yml`](docs/FORMAT.md). The window's second
tab writes one with you: **New** fills it in from what is in the library's folder, and a
form has a place for the rest, with the file's text beside it. One with only an
`installer.yml` from lib-installer is read through a built-in converter.

## License

Apache License 2.0. See [LICENSE.md](LICENSE.md).
