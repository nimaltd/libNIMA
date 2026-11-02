# Changelog

## 1.0.0

- libNIMA replaces lib-installer: the same installer rewritten in Rust, with a window beside the command line.
- The window: open a project, see which platform it is and which file told, add libraries by repository, zip or folder, see each marked OK, CHECK or NO with the way to put it right, preview what an install would change on a copy of the project, install, update and uninstall. What a run does is shown library by library, each in a part of its own.
- An editor for a library's `libnima.yml`, as a form or as text, which are one file. New fills one in from what is in the folder. The text offers what can go at the cursor and says what a key is, problems come with a fix a click makes, and a summary says what the file comes to. It also converts an `installer.yml`.
- `docs/libnima.schema.json`, which gives other editors the same help on the file.
- `libnima --check`, `--lint`, `--convert`, `--schema` and `--gui`, and `--add-to-path` for the command alone.
- A project moves over from lib-installer, or from one of the five installers before it, on the first run. Its blocks and record keep their contents, and each file is saved first.
