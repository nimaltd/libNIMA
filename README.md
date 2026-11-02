# libNIMA

[![CI](https://github.com/nimaltd/libNIMA/actions/workflows/ci.yml/badge.svg)](https://github.com/nimaltd/libNIMA/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE.md)

Installs embedded C libraries into STM32, Zephyr, ESP-IDF, Arduino and PlatformIO projects, from a window or from the `libnima` command, the same on Windows, macOS and Linux.

Copying two files into a project is easy. Doing it so the build actually picks them up, without clobbering the settings you changed last month, is the part this handles.

It works with any library that has a [`libnima.yml`](docs/FORMAT.md) at the root of its repository, or an `installer.yml` from lib-installer, which it reads through a built-in converter. It replaces [lib-installer](https://github.com/nimaltd/lib-installer), and needs no Python.

| Platform | Its name in `libnima.yml` and `--platform` | What libNIMA does there |
|---|---|---|
| STM32, with CubeMX | `stm32` | Registers the library with CMake, STM32CubeIDE, Keil MDK, IAR or CubeMX's Makefile. [More](docs/stm32.md) |
| Zephyr, and nRF Connect SDK | `zephyr` | Makes the library a Zephyr module, adds it to the build and switches it on in `prj.conf`. [More](docs/zephyr.md) |
| ESP-IDF | `esp32` | Makes the library a component, with its settings in menuconfig. [More](docs/esp32.md) |
| Arduino | `arduino` | Puts the library in the libraries folder, or in one sketch, with the `library.properties` Arduino needs. [More](docs/arduino.md) |
| PlatformIO | `pio` | Puts the library in `lib/`, with a `library.json`, and its defines in `platformio.ini`. [More](docs/pio.md) |

## Getting it

Download it from the [releases page](https://github.com/nimaltd/libNIMA/releases/latest):

| For | File |
|---|---|
| Windows 10 and 11 | the `-setup.exe` installer, or the `.msi` |
| macOS, Apple Silicon and Intel | the `.dmg` |
| Linux | the `.AppImage`, `.deb` or `.rpm` |
| A build server or a container | the command alone: `libnima-...-windows-x86_64.zip`, `-linux-x86_64.tar.gz` or `-macos-universal.tar.gz` |

The `-setup.exe`, `.msi`, `.deb` and `.rpm` put `libnima` on the PATH. From the `.dmg` or the `.AppImage`, use **Tools**, **Add libnima to the PATH**. For the command alone, unpack it where it can stay, and run `libnima --add-to-path`.

The apps are not signed yet:

- Windows may say "Windows protected your PC": click **More info**, then **Run anyway**.
- macOS may say the app is damaged or cannot be checked: move it to Applications, then run `xattr -cr /Applications/libNIMA.app` in Terminal.
- Linux: `chmod +x` the AppImage.

When a newer libNIMA is out, a run says so, and **Help**, **Check for updates** looks in the window. `--no-update-check` turns it off, and so do `LIBNIMA_NO_UPDATE_CHECK` and `CI`.

## Using it

In the window: open your project's folder, and libNIMA says which platform it is and which file told. Add a library by its repository, a zip or a folder, and it joins the list to install, marked with what it found in your project. **Preview** shows what an install would change, tried on a copy of the project. **Install all** does it.

From your project's folder, the command does the same:

```bash
libnima owner/example
```

`owner/example` is the library's GitHub owner and name, such as `nimaltd/ee24`. To see it work first, take the example library of your platform. Each one's README walks through an install, an update that keeps a setting you changed, an option, and an uninstall:

| Platform | Run it from | Try it with |
|---|---|---|
| STM32 | the project's root, the folder CubeMX made | `libnima nimaltd/lib-installer-example-stm32` |
| Zephyr | the application's folder, the one holding `prj.conf` | `libnima nimaltd/lib-installer-example-zephyr` |
| ESP-IDF | the project's folder, the one holding `main/` | `libnima nimaltd/lib-installer-example-esp32` |
| Arduino | anywhere, or a sketch's folder | `libnima nimaltd/lib-installer-example-arduino` |
| PlatformIO | the project's folder, the one holding `platformio.ini` | `libnima nimaltd/lib-installer-example-pio` |

Then build as you always do. An update is the same command again: what you wrote between `USER CODE BEGIN` and `USER CODE END` is kept, and anything else of yours that would be lost is saved as a `.bak` first. `libnima --uninstall example` takes a library out, and gives your project's files back as they were.

### Which platform

libNIMA reads your project and says which file told it. When that is wrong, say which it is: `--platform` on the command line, **Change** in the window. A library for another platform is refused before anything is written, with the reason, and a library that names several goes in as the one your project is.

### Every command

| To | Run |
|---|---|
| See which version you have | `libnima --version` |
| See every option | `libnima --help` |
| Open the window on this folder | `libnima --gui` |
| Install or update a library from GitHub | `libnima owner/example` |
| One release of it, a branch or a commit | `libnima owner/example@v2.0.0`, or `--ref v2.0.0` |
| By its GitHub address | `libnima https://github.com/owner/example` |
| From a downloaded zip, or a folder | `libnima D:/Downloads/example-main.zip` |
| Several at once | `libnima owner/example owner/other` |
| Check a library against this project, and write nothing | `libnima owner/example --check` |
| Uninstall, one or several in any order | `libnima --uninstall osal example` |
| Say the platform | `libnima owner/example --platform zephyr` |
| Into a folder you choose, without the question | `libnima owner/example --dir Libs/example` |
| Into a project you are not running from | `libnima owner/example --project D:/Work/MyBoard` |
| Take an option of the library, or leave it out | `--with littlefs`, `--without littlefs` |
| Run a step of the library, or leave it unrun | `--run make-key`, `--no-run make-key` |
| Answer every question with the answer offered, for a script | `--yes` |
| STM32: register with one IDE only | `--ide cubeide`, of `cmake`, `cubeide`, `keil`, `iar` and `makefile` |
| Zephyr: the Kconfig file the libraries are switched on in | `--conf boards/x.conf` |
| Arduino: into the libraries folder, or the sketch's `src` | `--into libraries`, `--into sketch` |
| Arduino: give the sketchbook folder | `--sketchbook D:/Arduino` |
| Check a folder's `libnima.yml` with no project | `libnima --lint` |
| Write a folder's `installer.yml` out as `libnima.yml` | `libnima --convert` |
| Write the JSON Schema of `libnima.yml`, for an editor | `libnima --schema` |
| Put the command alone on the PATH, or take it off | `libnima --add-to-path`, `--remove-from-path` |

### On a bad connection

A file that does not arrive is tried again, and every file of every library is fetched before anything is installed, so a run that fails part way leaves your project as it was. Behind a proxy, set `HTTPS_PROXY`, such as `http://127.0.0.1:10809`.

### Without internet

On a machine that has internet, download the command alone from the releases page, and the library from its green **Code** button, **Download ZIP**. Copy both over, then from your project:

```bash
libnima D:/Downloads/example-main.zip
```

If the library needs another one, such as `osal`, give both zips on one line. Forget it, and libNIMA stops before writing anything, naming the zip it is missing.

### A private repository

Give a GitHub token that can read it, made at [github.com/settings/personal-access-tokens](https://github.com/settings/personal-access-tokens), with **Contents** set to **Read-only**. On the command line use `--key github_pat_...`, or set `GITHUB_TOKEN` or `GH_TOKEN`, which keeps it out of your shell's history. The window asks for it. It is never saved, and is sent to GitHub and nowhere else.

## For someone writing a library

Put a `libnima.yml` at the root of the repository. [The format page](docs/FORMAT.md) has every key, and each platform's guide has what is worth knowing beyond the keys. The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and a form has a place for the rest, with the file's text beside it. `libnima --lint` checks one with no project, for a library's own CI.

There is nothing else to copy into the repository: put the install line in your README, with your library's name in it.

## Moving over from lib-installer

**A project lib-installer installed into moves over by itself** on the first run. `.lib-installer.json` becomes `.libnima.json`, and the lines that mark its blocks in your files name libnima from then on. Each file is saved first, as a `.bak`.

**A library with only an `installer.yml`** asking for lib-installer 3.x or 4.x installs as it is. `libnima --convert` writes the same out as `libnima.yml`. A step written as a Python script needs Python on that machine, since libNIMA has none of its own.

**The commands are the same**, with `libnima` in front.

## Building it

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Support

I write these tools and libraries in my own time and give them away, because good tools should be easy to get. If this one saved you an afternoon, there are two things that genuinely help:

**Star the repo.** It costs you one click, it helps other engineers find the tool, and it is the main reason I keep going.

**[Buy me a coffee on Ko-fi](https://ko-fi.com/nimaltd).** Any amount is a real motivation to keep writing, documenting and maintaining this work.

[![GitHub](https://img.shields.io/badge/GitHub-Follow-black?style=for-the-badge&logo=github)](https://github.com/NimaLTD)
[![YouTube](https://img.shields.io/badge/YouTube-Subscribe-red?style=for-the-badge&logo=youtube)](https://youtube.com/@nimaltd)
[![Instagram](https://img.shields.io/badge/Instagram-Follow-purple?style=for-the-badge&logo=instagram)](https://instagram.com/github.nimaltd)
[![LinkedIn](https://img.shields.io/badge/LinkedIn-Connect-blue?style=for-the-badge&logo=linkedin)](https://linkedin.com/in/nimaltd)
[![Email](https://img.shields.io/badge/Email-Contact-red?style=for-the-badge&logo=gmail)](mailto:nima.askari@gmail.com)
[![Ko-fi](https://img.shields.io/badge/Ko--fi-Support-orange?style=for-the-badge&logo=ko-fi)](https://ko-fi.com/nimaltd)

## License

Apache License 2.0. See [LICENSE.md](LICENSE.md).
