#!/usr/bin/env python3
"""
What libNIMA writes, against what lib-installer 4.0.1 writes.

A development check only, never part of what people get. It runs the same scenarios
through both, each in its own copy of the same project, and says where the two trees
differ, apart from the name: lib-installer's own name in the files it writes, and its
record, of which the fields that mean something are compared.

    python -m pip install lib-installer==4.0.1
    cargo build -p libnima-cli
    python tests/compare/compare.py
"""

import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SIBLINGS = ROOT.parent
LIBNIMA = ROOT / "target" / "debug" / ("libnima.exe" if os.name == "nt" else "libnima")

ENV = dict(os.environ, INSTALLER_NO_UPDATE_CHECK="1", LIBNIMA_NO_UPDATE_CHECK="1")
ENV.pop("GITHUB_TOKEN", None)
ENV.pop("GH_TOKEN", None)

BACKUP = re.compile(r"\.\d{8}-\d{6}(?:-\d+)?\.bak$")

CPROJECT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n'
    "<?fileVersion 4.0.0?><cproject>\n"
    '\t<storageModule moduleId="cdtBuildSystem">\n'
    '\t\t<configuration name="Debug">\n'
    "\t\t\t<toolChain>\n"
    "\t\t\t\t<tool>\n"
    '\t\t\t\t\t<option id="as" superClass="x.assembler.option.definedsymbols" valueType="definedSymbols">\n'
    '\t\t\t\t\t\t<listOptionValue builtIn="false" value="DEBUG"/>\n'
    "\t\t\t\t\t</option>\n"
    "\t\t\t\t</tool>\n"
    "\t\t\t\t<tool>\n"
    '\t\t\t\t\t<option id="da" superClass="x.c.compiler.option.definedsymbols" valueType="definedSymbols">\n'
    '\t\t\t\t\t\t<listOptionValue builtIn="false" value="DEBUG"/>\n'
    '\t\t\t\t\t\t<listOptionValue builtIn="false" value="USE_HAL_DRIVER"/>\n'
    "\t\t\t\t\t</option>\n"
    '\t\t\t\t\t<option id="a" superClass="x.c.compiler.option.includepaths" valueType="includePath">\n'
    '\t\t\t\t\t\t<listOptionValue builtIn="false" value="../Core/Inc"/>\n'
    "\t\t\t\t\t</option>\n"
    '\t\t\t\t\t<option id="opt" superClass="x.c.compiler.option.optimization.level"/>\n'
    "\t\t\t\t</tool>\n"
    "\t\t\t</toolChain>\n"
    "\t\t</configuration>\n"
    '\t\t<configuration name="Release">\n'
    "\t\t\t<toolChain>\n"
    "\t\t\t\t<tool>\n"
    '\t\t\t\t\t<option id="b" superClass="x.c.compiler.option.includepaths" valueType="includePath">\n'
    '\t\t\t\t\t\t<listOptionValue builtIn="false" value="../Core/Inc"/>\n'
    "\t\t\t\t\t</option>\n"
    "\t\t\t\t</tool>\n"
    "\t\t\t</toolChain>\n"
    '\t\t\t<sourceEntries>\n\t\t\t\t<entry flags="VALUE_WORKSPACE_PATH|RESOLVED" kind="sourcePath" name="Core"/>\n\t\t\t</sourceEntries>\n'
    "\t\t</configuration>\n"
    "\t</storageModule>\n"
    "</cproject>\n"
)

UVPROJX = (
    '<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n'
    "<Project>\n  <Targets>\n    <Target>\n      <TargetOption>\n        <TargetArmAds>\n"
    "          <Cads>\n            <VariousControls>\n"
    "              <Define>USE_HAL_DRIVER,STM32G431xx</Define>\n"
    "              <IncludePath>..\\Core\\Inc</IncludePath>\n"
    "            </VariousControls>\n          </Cads>\n"
    "          <Aads>\n            <VariousControls>\n              <Define></Define>\n"
    "            </VariousControls>\n          </Aads>\n"
    "        </TargetArmAds>\n      </TargetOption>\n"
    "      <Groups>\n        <Group>\n          <GroupName>Application/User/Core</GroupName>\n"
    "          <Files>\n            <File>\n              <FileName>main.c</FileName>\n"
    "              <FileType>1</FileType>\n              <FilePath>..\\Core\\Src\\main.c</FilePath>\n"
    "            </File>\n          </Files>\n        </Group>\n      </Groups>\n"
    "    </Target>\n  </Targets>\n</Project>\n"
)

EWP = (
    '<?xml version="1.0" encoding="UTF-8"?>\n<project>\n    <fileVersion>4</fileVersion>\n'
    "    <configuration>\n        <name>Proj</name>\n        <settings>\n"
    "            <name>ICCARM</name>\n            <data>\n"
    "                <option>\n                    <name>CCDefines</name>\n"
    "                    <state>USE_HAL_DRIVER</state>\n                </option>\n"
    "                <option>\n                    <name>CCIncludePath2</name>\n"
    "                    <state>$PROJ_DIR$/../Core/Inc</state>\n                </option>\n"
    "            </data>\n        </settings>\n    </configuration>\n"
    "    <group>\n        <name>Application</name>\n        <group>\n            <name>User</name>\n"
    "            <file>\n                <name>$PROJ_DIR$\\..\\Core\\Src\\main.c</name>\n            </file>\n"
    "        </group>\n    </group>\n</project>\n"
)

EWW = '<?xml version="1.0" encoding="UTF-8"?>\n<workspace>\n  <project>\n    <path>$WS_DIR$\\Proj.ewp</path>\n  </project>\n</workspace>\n'

EWT = (
    '<?xml version="1.0" encoding="UTF-8"?>\n<project>\n    <fileVersion>4</fileVersion>\n'
    "    <group>\n        <name>Drivers</name>\n        <file>\n"
    "            <name>$PROJ_DIR$\\..\\Drivers\\hal.c</name>\n        </file>\n    </group>\n</project>\n"
)

MAKEFILE = (
    "TARGET = Proj\n\n# C sources\nC_SOURCES =  \\\nCore/Src/main.c \\\nCore/Src/it.c  \n\n"
    "# ASM sources\nASM_SOURCES =  \\\nstartup.s\n\n# C defines\nC_DEFS =  \\\n-DUSE_HAL_DRIVER \\\n-DSTM32G431xx\n\n"
    "# C includes\nC_INCLUDES =  \\\n-ICore/Inc \\\n-IDrivers/CMSIS/Include\n\nCFLAGS += $(C_DEFS) $(C_INCLUDES)\n"
)

CMAKE = "cmake_minimum_required(VERSION 3.22)\nproject(Proj C ASM)\nadd_executable(${CMAKE_PROJECT_NAME} Core/Src/main.c)\ntarget_link_libraries(${CMAKE_PROJECT_NAME} stm32cubemx)\n"

HAL_CONF = "#define HAL_MODULE_ENABLED\n#define HAL_SPI_MODULE_ENABLED\n"
IOC = "Mcu.IP0=SPI1\nMcu.IP1=NVIC\n"

ZEPHYR_CMAKE = "cmake_minimum_required(VERSION 3.20.0)\nfind_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})\nproject(app)\ntarget_sources(app PRIVATE src/main.c)\n"
IDF_CMAKE = "cmake_minimum_required(VERSION 3.16)\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nproject(app)\n"
IDF_MAIN = 'idf_component_register(SRCS "main.c"\n    INCLUDE_DIRS "."\n    PRIV_REQUIRES driver)\n'
PIO_INI = "[platformio]\ndefault_envs = uno\n\n[env]\nframework = arduino\n\n[env:uno]\nplatform = atmelavr\nboard = uno\nbuild_flags =\n    -DFROM_YOU=1\n\n[env:esp]\nplatform = espressif32\nboard = esp32dev\n"


def put(root, files, newline="\n"):
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.replace("\n", newline).encode("utf-8"))


def stm32_project(newline="\n"):
    return {
        "Core/Inc/main.h": "/* main */\n",
        "Core/Inc/stm32g4xx_hal_conf.h": HAL_CONF,
        "Core/Src/main.c": "/* main */\n",
        "Proj.ioc": IOC,
        "CMakeLists.txt": CMAKE,
        ".project": "<projectDescription/>\n",
        ".cproject": CPROJECT,
        "MDK/Proj.uvprojx": UVPROJX,
        "EWARM/Proj.ewp": EWP,
        "EWARM/Proj.ewt": EWT,
        "EWARM/Project.eww": EWW,
        "Makefile": MAKEFILE,
    }


KITCHEN = """\
name: kitchen
description: "A library with everything in it, é included"
repository: https://github.com/someone/kitchen
license: Apache-2.0
platform: [stm32, zephyr, esp32, arduino, pio]
kind: driver
category: storage
requires:
  lib_installer: 4.0.0
  peripherals:
    - type: spi
      count: 2
      note: Master mode
install:
  include_dirs: [inc, port]
  defines:
    - KITCHEN_FAST
    - KITCHEN_SIZE=32
headers:
  - inc/kitchen.h
  - inc/kitchen_config.h
sources:
  - src/kitchen.c
  - src/more.c
  - src/asm.s
options:
  fast:
    description: Go fast
    headers: [port/fast.h]
    sources: [port/fast.c]
    zephyr:
      project:
        cmake:
          after-zephyr:
            - message(STATUS "fast")
      library:
        cmake:
          - zephyr_library_compile_definitions(FAST)
    esp32:
      project:
        cmake:
          after-project:
            - message(STATUS "fast")
      library:
        cmake:
          - target_compile_definitions(${COMPONENT_LIB} PRIVATE FAST)
extras:
  - LICENSE.md
  - NOTICE
  - to: doc/guide.md
    from: guide.md
  - root: tools
once:
  - Docs/kitchen_notes.md
  - from: templates/port.c
    to: Core/Src/kitchen_port.c
run:
  - label: make-id
    description: Make an ID
    command: [python, tools/make_id.py]
    args: [kitchen_id.txt, "{library}"]
    unless: kitchen_id.txt
zephyr:
  symbol:
    KITCHEN: the switch of it
  settings:
    - name: KITCHEN_PERIOD
      type: int
      default: 5
      range: [1, 10]
      prompt: How often
      help: |
        Line one.
        Line two.
    - name: KITCHEN_MASK
      type: hex
      default: 0x10
    - name: KITCHEN_ON
      type: bool
      default: true
    - name: KITCHEN_NAME
      type: string
      default: a "name"
  project:
    conf:
      Logging:
        - LOG
        - LOG_LEVEL=2: only errors
      Other:
        - GPIO
    sysbuild:
      - BOOTLOADER_MCUBOOT
      - SIGNATURE_KEY_FILE="${APP_DIR}/k.pem": the key
    cmake:
      before-zephyr:
        Early:
          - set(EARLY "{library}")
      after-zephyr:
        - target_sources(app PRIVATE "{library}/x.c")
  library:
    kconfig:
      Printing:
        - PRINTK: prints
        - BT_MTU=498
    conf:
      Quiet:
        - BOOT_BANNER=n: no banner
    cmake:
      Math:
        - zephyr_library_link_libraries(m)
esp32:
  settings:
    - name: KITCHEN_DEPTH
      type: int
      default: 3
      range: [1, 9]
      prompt: How deep
      help: |
        A help.
  project:
    conf:
      Tick:
        - FREERTOS_HZ=1000: a tick a millisecond
      - skipped
    cmake:
      before-project:
        - set(EARLY 1)
      after-project:
        Late:
          - message(STATUS "{library}")
  library:
    requires: [esp_timer, driver]
    cmake:
      - target_link_libraries(${COMPONENT_LIB} PRIVATE m)
arduino:
  architectures: [avr, esp32]
  category: Data Storage
  author: Someone
  maintainer: Someone Else
  depends:
    - ArduinoJson (>=6.0.0)
pio:
  project:
    build_flags:
      Printing:
        - -Wl,-u,_printf_float: floats
      - -O2
  library:
    frameworks: [arduino, espidf]
    platforms: [atmelavr, espressif32]
    dependencies:
      - bblanchon/ArduinoJson@^7.0.0
      - plain/lib
""".replace("      - skipped\n", "").replace("      - -O2\n", "")

KITCHEN_FILES = {
    "inc/kitchen.h": "/**\n * @file kitchen.h\n * @version 1.2.3\n */\n",
    "inc/kitchen_config.h": "/* USER CODE BEGIN Config */\n#define SIZE 1\n/* USER CODE END Config */\nint config;\n",
    "src/kitchen.c": "int kitchen;\n",
    "src/more.c": "int more;\r\n",
    "src/asm.s": "nop\n",
    "port/fast.h": "int fast(void);\n",
    "port/fast.c": "int fast(void) { return 1; }\n",
    "LICENSE.md": "licence\n",
    "NOTICE": "notice\n",
    "guide.md": "guide\n",
    "tools/make_id.py": "import sys, pathlib\npathlib.Path(sys.argv[1]).write_text(sys.argv[2])\n",
    "tools/sub/other.txt": "other\n",
    "Docs/kitchen_notes.md": "notes\n",
    "templates/port.c": "int port;\n",
}


def kitchen(root):
    folder = root / "lib-kitchen"
    put(folder, KITCHEN_FILES)
    (folder / "installer.yml").write_text(KITCHEN, encoding="utf-8", newline="\n")

    return folder


def old(arguments, cwd):
    return subprocess.run([sys.executable, "-m", "lib_installer"] + arguments, cwd=cwd, env=ENV,
                          stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", errors="replace")


def new(arguments, cwd):
    return subprocess.run([str(LIBNIMA)] + arguments, cwd=cwd, env=ENV,
                          stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", errors="replace")


def normal_text(data):
    """What differs by name alone, taken away."""
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        return data

    # A step of the example libraries makes an ID of its own each time it runs.
    text = re.sub(r"0x[0-9A-F]{8}UL", "0xIDUL", text)

    return text.replace("lib-installer", "libnima").replace("installer.yml", "libnima.yml").encode("utf-8")


def tree(root):
    """
    Every file under root, as {path: bytes}. The time in a backup's name is taken out, and
    the copies of one file are put in the order of what they hold, since which of two copies
    taken in the same second gets which name says nothing.
    """
    found = {}
    copies = {}

    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        relative = path.relative_to(root).as_posix().replace(".lib-installer.json", ".libnima.json")

        if BACKUP.search(relative):
            copies.setdefault(BACKUP.sub(".bak", relative), []).append(path.read_bytes())
        else:
            found[relative] = path.read_bytes()

    for name, held in copies.items():
        for index, data in enumerate(sorted(held, key=normal_text)):
            found[name + "+" * index] = data

    return found


def record_fields(data):
    """What a record says that means something: what it installed, and what it was given."""
    record = json.loads(data.decode("utf-8"))

    for entry in record.get("libraries", {}).values():
        entry.pop("installed_at", None)
        origin = entry.get("origin") or {}

        # The folder a library was read from is a temporary one, and differs by itself.
        origin.pop("path", None)

        # What is inside a block has the installer's name in its comments, so only which blocks there are is compared.
        entry["blocks"] = {name: sorted(found) for name, found in (entry.get("blocks") or {}).items()}

        # A step's fingerprint named its script, and now names its command.
        entry["ran"] = sorted(entry.get("ran") or {})

    return json.dumps(record, indent=1, sort_keys=True)


def compare_trees(name, left, right):
    one, two = tree(left), tree(right)
    problems = []

    for path in sorted(set(one) | set(two)):
        if path not in one:
            problems.append(f"{name}: only libNIMA wrote {path}")
            continue

        if path not in two:
            problems.append(f"{name}: only lib-installer wrote {path}")
            continue

        if path.startswith(".libnima.json") or path.endswith("/.libnima.json"):
            a, b = record_fields(one[path]), record_fields(two[path])

            if ".bak" in path:
                continue
        else:
            a, b = normal_text(one[path]), normal_text(two[path])

        if a != b:
            problems.append(f"{name}: {path} differs")

            if isinstance(a, bytes) and isinstance(b, bytes):
                a = a.decode("utf-8", "replace")
                b = b.decode("utf-8", "replace")

            problems.extend("    " + line.rstrip("\n") for line in
                            difflib.unified_diff(a.splitlines(True), b.splitlines(True), "lib-installer", "libNIMA", n=1))

    return problems


class Scenario:
    def __init__(self, name, library_args, project, steps, newline="\n", cwd=None, extra=None):
        self.name = name
        self.project = project
        self.steps = steps
        self.newline = newline
        self.cwd = cwd
        self.extra = extra or []


def run_scenario(scenario, work):
    problems = []
    left = work / "left"
    right = work / "right"

    for side in (left, right):
        side.mkdir(parents=True)
        put(side, scenario.project, scenario.newline)

    for number, step in enumerate(scenario.steps):
        if callable(step):
            step(left)
            step(right)
            continue

        words = [str(word) for word in step] + list(scenario.extra)
        one = old(words, left)
        two = new(words, right)

        if one.returncode != two.returncode:
            problems.append(f"{scenario.name}, step {number}: lib-installer ended {one.returncode}, libNIMA {two.returncode}")
            problems.append("    lib-installer: " + (one.stdout + one.stderr)[-1500:])
            problems.append("    libNIMA: " + (two.stdout + two.stderr)[-1500:])

        # Two that fail alike would look the same, and prove nothing.
        elif one.returncode != 0:
            problems.append(f"{scenario.name}, step {number}: both ended {one.returncode}")
            problems.append("    lib-installer: " + (one.stdout + one.stderr)[-1500:])
            problems.append("    libNIMA: " + (two.stdout + two.stderr)[-1500:])

        problems.extend(compare_trees(f"{scenario.name}, step {number}", left, right))

        if problems:
            break

    return problems


def scenarios(libs):
    stm32_lib = SIBLINGS / "ee24"
    osal = SIBLINGS / "osal"
    example = SIBLINGS / "lib-installer-example-stm32"
    zephyr_example = SIBLINGS / "lib-installer-example-zephyr"
    esp_example = SIBLINGS / "lib-installer-example-esp32"
    pio_example = SIBLINGS / "lib-installer-example-pio"
    kit = kitchen(libs)

    def edit(name, old_text, new_text):
        def apply(root):
            path = root / name
            path.write_bytes(path.read_bytes().replace(old_text.encode(), new_text.encode()))

        return apply

    found = []

    for newline, tag in (("\n", "LF"), ("\r\n", "CRLF")):
        found.append(Scenario(f"STM32 kitchen, every IDE, {tag}", None, stm32_project(), [
            [kit, "--platform", "stm32", "--dir", "Libs/kitchen", "--with", "fast", "--yes"],
            [kit, "--platform", "stm32", "--dir", "Libs/kitchen", "--with", "fast", "--yes"],
            edit("Libs/kitchen/inc/kitchen_config.h", "#define SIZE 1", "#define SIZE 99"),
            edit("Libs/kitchen/src/kitchen.c", "int kitchen;", "int kitchen; // mine"),
            [kit, "--platform", "stm32", "--dir", "Libs/kitchen", "--without", "fast", "--yes"],
            ["--uninstall", "kitchen"],
        ], newline))

    found.append(Scenario("STM32 example in its own folder", None, stm32_project(), [
        [example, "--yes"],
        [example, "--yes", "--with", "uptime"],
        ["--uninstall", "example"],
    ]))

    found.append(Scenario("STM32 ee24 and osal together, every IDE", None, stm32_project(), [
        [stm32_lib, osal, "--yes"],
        ["--uninstall", "osal", "ee24"],
    ]))

    found.append(Scenario("Zephyr example", None, {
        "CMakeLists.txt": ZEPHYR_CMAKE,
        "prj.conf": "CONFIG_GPIO=y\n",
        "src/main.c": "int main;\n",
    }, [
        [zephyr_example, "--yes"],
        [zephyr_example, "--yes", "--with", "uptime"],
        ["--uninstall", "example"],
    ]))

    for newline, tag in (("\n", "LF"), ("\r\n", "CRLF")):
        found.append(Scenario(f"Zephyr kitchen, {tag}", None, {
            "CMakeLists.txt": ZEPHYR_CMAKE,
            "prj.conf": "CONFIG_GPIO=y\nCONFIG_LOG=n\n",
            "boards/x.conf": "CONFIG_X=1\n",
            "sysbuild.conf": "SB_CONFIG_BOOTLOADER_MCUBOOT=y\n",
            "src/main.c": "int main;\n",
        }, [
            [kit, "--platform", "zephyr", "--with", "fast", "--conf", "boards/x.conf", "--yes"],
            [kit, "--platform", "zephyr", "--with", "fast", "--yes"],
            edit("modules/kitchen/zephyr/CMakeLists.txt", "# USER CODE BEGIN KITCHEN_CMAKE\n" if newline == "\n" else "# USER CODE BEGIN KITCHEN_CMAKE\n", "# USER CODE BEGIN KITCHEN_CMAKE\nmine()\n"),
            [kit, "--platform", "zephyr", "--without", "fast", "--yes"],
            ["--uninstall", "kitchen"],
        ], newline))

    found.append(Scenario("ESP-IDF example", None, {
        "CMakeLists.txt": IDF_CMAKE,
        "main/CMakeLists.txt": IDF_MAIN,
        "main/main.c": "int main;\n",
        "sdkconfig": "CONFIG_FREERTOS_HZ=100\n",
    }, [
        [esp_example, "--yes"],
        [esp_example, "--yes", "--with", "uptime"],
        ["--uninstall", "example"],
    ]))

    for newline, tag in (("\n", "LF"), ("\r\n", "CRLF")):
        found.append(Scenario(f"ESP-IDF kitchen outside components, {tag}", None, {
            "CMakeLists.txt": IDF_CMAKE,
            "main/CMakeLists.txt": IDF_MAIN,
            "main/main.c": "int main;\n",
            "sdkconfig.defaults": "CONFIG_A=y\n",
        }, [
            [kit, "--platform", "esp32", "--dir", "extra/kitchen", "--with", "fast", "--yes"],
            [kit, "--platform", "esp32", "--dir", "extra/kitchen", "--yes"],
            ["--uninstall", "kitchen"],
        ], newline))

    found.append(Scenario("PlatformIO example", None, {"platformio.ini": PIO_INI}, [
        [pio_example, "--yes"],
        [pio_example, "--yes", "--with", "uptime"],
        ["--uninstall", "example"],
    ]))

    for newline, tag in (("\n", "LF"), ("\r\n", "CRLF")):
        found.append(Scenario(f"PlatformIO kitchen, {tag}", None, {"platformio.ini": PIO_INI}, [
            [kit, "--platform", "pio", "--with", "fast", "--yes"],
            [kit, "--platform", "pio", "--yes"],
            ["--uninstall", "kitchen"],
        ], newline))

    return found


def arduino_scenarios(libs, work):
    """Arduino's libraries folder is outside the project, so it runs on its own."""
    kit = kitchen(libs)
    example = SIBLINGS / "lib-installer-example-arduino"
    problems = []

    for name, steps in (
        ("Arduino example, libraries folder", [
            [example, "--into", "libraries", "--yes"],
            ["--uninstall", "example", "--into", "libraries", "--platform", "arduino"],
        ]),
        ("Arduino kitchen, a sketch", [
            [kit, "--platform", "arduino", "--into", "sketch", "--yes"],
            ["--uninstall", "kitchen", "--into", "sketch"],
        ]),
    ):
        here = work / re.sub(r"\W+", "_", name)
        sides = {}

        for side in ("left", "right"):
            sketch = here / side / "Blink"
            book = here / side / "book"
            sketch.mkdir(parents=True)
            book.mkdir(parents=True)
            (sketch / "Blink.ino").write_text("void setup() {}\n", encoding="utf-8")
            sides[side] = (sketch, book)

        for number, step in enumerate(steps):
            one = old([str(word) for word in step] + ["--sketchbook", str(sides["left"][1])], sides["left"][0])
            two = new([str(word) for word in step] + ["--sketchbook", str(sides["right"][1])], sides["right"][0])

            if one.returncode != two.returncode:
                problems.append(f"{name}, step {number}: lib-installer ended {one.returncode}, libNIMA {two.returncode}")
                problems.append("    lib-installer: " + (one.stdout + one.stderr)[-1500:])
                problems.append("    libNIMA: " + (two.stdout + two.stderr)[-1500:])

            elif one.returncode != 0:
                problems.append(f"{name}, step {number}: both ended {one.returncode}")
                problems.append("    lib-installer: " + (one.stdout + one.stderr)[-1500:])

            problems.extend(compare_trees(f"{name}, step {number}", here / "left", here / "right"))

            if problems:
                return problems

    return problems


def main():
    if not LIBNIMA.is_file():
        print(f"Build it first: cargo build -p libnima-cli ({LIBNIMA} is not there)")
        return 2

    failed = 0

    with tempfile.TemporaryDirectory() as temporary:
        base = Path(temporary)
        libs = base / "libs"
        libs.mkdir()

        for index, scenario in enumerate(scenarios(libs)):
            problems = run_scenario(scenario, base / f"s{index}")
            print(("ok    " if not problems else "FAIL  ") + scenario.name)

            for line in problems[:60]:
                print(line)

            failed += bool(problems)

        problems = arduino_scenarios(libs, base / "arduino")
        print(("ok    " if not problems else "FAIL  ") + "Arduino")

        for line in problems[:60]:
            print(line)

        failed += bool(problems)

    print("PASS: both write the same." if not failed else f"FAIL: {failed} scenarios differ.")

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
