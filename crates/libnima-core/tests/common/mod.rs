//! What the tests share: a throwaway library, a throwaway project, and a run that is watched.
//!
//! The project files are nested and indented the way the real tools write them, tabs in a .cproject
//! and spaces in the other two, because what these are used to check is that an added line comes out
//! looking like the lines around it. A flat fixture cannot tell a right answer from a wrong one.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use libnima_core::download::Client;
use libnima_core::flow::{self, Request};
use libnima_core::platform::Platform;
use libnima_core::report::{Line, Question, Ui};
use serde_json::Value;

/// A run a test watches: everything said is kept as plain text, and each question takes the next
/// answer, or goes unanswered when there is none left, as with nobody at the keyboard.
#[derive(Default)]
pub struct Watched {
    pub said: Vec<String>,
    pub errors: Vec<String>,
    pub asked: Vec<String>,
    pub answers: Vec<String>,
    /// How many lines had been said when each part started.
    pub part_starts: Vec<usize>,
}

impl Watched {
    pub fn answering(answers: &[&str]) -> Watched {
        Watched {
            answers: answers.iter().map(ToString::to_string).collect(),
            ..Watched::default()
        }
    }

    pub fn out(&self) -> String {
        self.said.join("\n")
    }

    pub fn err(&self) -> String {
        self.errors.join("\n")
    }

    /// What was said in each part, the part before the first of them first.
    pub fn parts(&self) -> Vec<String> {
        let mut edges = vec![0];
        edges.extend(&self.part_starts);
        edges.push(self.said.len());

        edges
            .windows(2)
            .map(|edge| self.said[edge[0]..edge[1]].join("\n"))
            .collect()
    }
}

impl Ui for Watched {
    fn say(&mut self, line: Line) {
        self.said
            .push(line.iter().map(|span| span.text.as_str()).collect());
    }

    fn new_part(&mut self) {
        self.part_starts.push(self.said.len());
    }

    fn error(&mut self, message: &str) {
        self.errors.push(message.to_string());
    }

    fn ask(&mut self, question: &Question) -> Option<String> {
        self.asked.push(
            question
                .prompt
                .iter()
                .map(|span| span.text.as_str())
                .collect(),
        );

        if self.answers.is_empty() {
            None
        } else {
            Some(self.answers.remove(0))
        }
    }

    fn shows_output(&self) -> bool {
        false
    }
}

/// One request the stand-in for GitHub was sent.
#[derive(Clone, Debug)]
pub struct Seen {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The parts of the address, with what follows a ? left out.
    fn parts(&self) -> Vec<&str> {
        self.url
            .split('?')
            .next()
            .unwrap()
            .trim_start_matches('/')
            .split('/')
            .collect()
    }

    pub fn is_raw(&self) -> bool {
        self.url.starts_with("/raw/")
    }

    pub fn is_api(&self) -> bool {
        self.url.starts_with("/repos/")
    }

    pub fn repo(&self) -> String {
        let parts = self.parts();

        format!("{}/{}", parts[1], parts[2])
    }

    /// The ref a file or a listing was asked at.
    pub fn reference(&self) -> String {
        if let Some((_, reference)) = self.url.split_once("?ref=") {
            return unquote(reference);
        }

        if let Some((_, rest)) = self.url.split_once("/git/trees/") {
            return unquote(rest.split('?').next().unwrap());
        }

        self.parts()[3].to_string()
    }

    /// The name of the file asked for.
    pub fn file(&self) -> String {
        let path = self.url.split('?').next().unwrap();

        match path.split_once("/contents/") {
            Some((_, file)) => unquote(file),
            None => self.parts()[4..].join("/"),
        }
    }

    pub fn is_listing(&self) -> bool {
        self.url.contains("/git/trees/")
    }
}

fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'%' {
            out.push(u8::from_str_radix(&text[at + 1..at + 3], 16).unwrap());
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }

    String::from_utf8(out).unwrap()
}

/// What one request meets on a link that is not well.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    /// This request is answered well, for one that comes after it to meet the fault.
    Fine,
    /// The connection closes with no answer.
    Dropped,
    /// The answer stops part way through its body.
    CutShort,
    Status(u16),
}

/// What the stand-in for GitHub holds, and what it has been asked.
#[derive(Default)]
pub struct Holding {
    /// Each repository, as owner/name, and the folder that is its files at every ref.
    pub repos: Vec<(String, PathBuf)>,
    /// Every request for a file or a listing.
    pub seen: Vec<Seen>,
    /// Every request for which commit a ref is, kept apart from the files.
    pub lookups: Vec<Seen>,
    /// The commit a ref is. A ref not here is a 404 to the question, as with a limit on requests.
    pub commits: HashMap<String, String>,
    /// When set, only these refs exist.
    pub branches: Option<Vec<String>>,
    /// The answer to everything but the question of which commit a ref is.
    pub status: Option<u16>,
    /// GitHub's X-Accepted-GitHub-Permissions, on that answer.
    pub permissions: Option<String>,
    /// What each request meets in turn, before they are answered well.
    pub faults: Vec<Fault>,
    /// The listing says it left some out.
    pub truncated: bool,
    /// Every request is sent on to this server, as GitHub does for a renamed repository.
    pub redirect: Option<String>,
}

/// GitHub, as far as a client can tell, from a thread on this machine. Each repository is a folder on disk.
pub struct Github {
    origin: String,
    holding: Arc<Mutex<Holding>>,
}

impl Github {
    pub fn serving(repos: &[(&str, &Path)]) -> Github {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let holding = Arc::new(Mutex::new(Holding {
            repos: repos
                .iter()
                .map(|(name, folder)| {
                    let name = if name.contains('/') {
                        name.to_string()
                    } else {
                        format!("nimaltd/{name}")
                    };

                    (name, folder.to_path_buf())
                })
                .collect(),
            ..Holding::default()
        }));
        let serving = holding.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(stream, &serving);
            }
        });

        Github { origin, holding }
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// A client of this server, with no token.
    pub fn client(&self) -> Client {
        Client::at(&self.origin, None)
    }

    /// A client of this server, with a token found in a place: --key, or a variable.
    pub fn client_with(&self, place: &str, token: &str) -> Client {
        Client::at(&self.origin, Some((place, token)))
    }

    pub fn holding(&self) -> MutexGuard<'_, Holding> {
        self.holding.lock().unwrap()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.holding().seen.clone()
    }

    /// The repositories asked for, each once, in the order they were first asked for.
    pub fn repos_asked(&self) -> Vec<String> {
        let mut repos: Vec<String> = Vec::new();

        for seen in self.seen() {
            if !repos.contains(&seen.repo()) {
                repos.push(seen.repo());
            }
        }

        repos
    }
}

fn answer(mut stream: TcpStream, holding: &Mutex<Holding>) {
    let mut asked = Vec::new();
    let mut chunk = [0_u8; 4096];

    while !asked.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => asked.extend_from_slice(&chunk[..count]),
        }
    }

    let text = String::from_utf8_lossy(&asked).into_owned();
    let mut lines = text.split("\r\n");
    let url = lines.next().unwrap().split(' ').nth(1).unwrap().to_string();
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let seen = Seen { url, headers };

    let (code, extra, body, cut) = {
        let mut holding = holding.lock().unwrap();

        match reply(&mut holding, seen) {
            Reply::Nothing => return,
            Reply::Answer {
                code,
                extra,
                body,
                cut,
            } => (code, extra, body, cut),
        }
    };

    let length = body.len();
    let body = if cut { &body[..length / 2] } else { &body[..] };
    let head = format!(
        "HTTP/1.1 {code} Answer\r\nContent-Length: {length}\r\nConnection: close\r\n{extra}\r\n"
    );

    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

enum Reply {
    Nothing,
    Answer {
        code: u16,
        extra: String,
        body: Vec<u8>,
        cut: bool,
    },
}

fn status(code: u16) -> Reply {
    Reply::Answer {
        code,
        extra: String::new(),
        body: Vec::new(),
        cut: false,
    }
}

fn reply(holding: &mut Holding, seen: Seen) -> Reply {
    let path = seen.url.split('?').next().unwrap().to_string();

    if let Some((_, reference)) = path.split_once("/commits/") {
        holding.lookups.push(seen.clone());

        return match holding
            .commits
            .get(&unquote(reference))
            .filter(|_| holding.status.is_none())
        {
            Some(commit) => Reply::Answer {
                code: 200,
                extra: String::new(),
                body: commit.clone().into_bytes(),
                cut: false,
            },
            None => status(holding.status.unwrap_or(404)),
        };
    }

    holding.seen.push(seen.clone());

    if let Some(redirect) = &holding.redirect {
        let extra = format!("Location: {redirect}{}\r\n", seen.url);

        return Reply::Answer {
            code: 302,
            extra,
            body: Vec::new(),
            cut: false,
        };
    }

    let fault = if holding.faults.is_empty() {
        None
    } else {
        Some(holding.faults.remove(0))
    };

    match fault {
        Some(Fault::Dropped) => return Reply::Nothing,
        Some(Fault::Status(code)) => return status(code),
        Some(Fault::CutShort | Fault::Fine) | None => {}
    }

    if let Some(code) = holding.status {
        let extra = holding
            .permissions
            .as_ref()
            .map(|permissions| format!("X-Accepted-GitHub-Permissions: {permissions}\r\n"))
            .unwrap_or_default();

        return Reply::Answer {
            code,
            extra,
            body: Vec::new(),
            cut: false,
        };
    }

    let Some(folder) = holding
        .repos
        .iter()
        .find(|(name, _)| *name == seen.repo())
        .map(|(_, folder)| folder.clone())
    else {
        return status(404);
    };

    if let Some(branches) = &holding.branches
        && !branches.contains(&seen.reference())
    {
        return status(404);
    }

    let (found, body) = if seen.is_listing() {
        let tree: Vec<Value> = libnima_core::paths::files_below(&folder)
            .into_iter()
            .map(|path| serde_json::json!({"path": path, "type": "blob"}))
            .collect();
        let listing = serde_json::json!({"tree": tree, "truncated": holding.truncated});

        (true, listing.to_string().into_bytes())
    } else {
        match fs::read(folder.join(seen.file())) {
            Ok(data) => (true, data),
            Err(_) => (false, Vec::new()),
        }
    };

    if !found {
        return status(404);
    }

    Reply::Answer {
        code: 200,
        extra: String::new(),
        body,
        cut: fault == Some(Fault::CutShort),
    }
}

/// Run with what reaches GitHub given.
pub fn run_with(client: Client, asked: &Request) -> (i32, Watched) {
    let mut ui = Watched::default();
    let code = flow::run_with(client, asked, &mut ui);

    (code, ui)
}

/// A folder of files, each a path and its text.
pub fn put(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// The code of a library called demo, and its libnima.yml's lines for it.
pub const DEMO_CODE: &str = "headers:\n  - inc/demo.h\nsources:\n  - src/demo.c\n";

/// A library on disk: its libnima.yml, with what every one needs around `body`, and its files, those of
/// DEMO_CODE and a NOTICE always among them.
pub fn library(root: &Path, name: &str, body: &str, files: &[(&str, &str)]) -> PathBuf {
    library_for("stm32", root, name, body, files)
}

/// The same, for another platform.
pub fn library_for(
    platform: &str,
    root: &Path,
    name: &str,
    body: &str,
    files: &[(&str, &str)],
) -> PathBuf {
    let folder = root.join(name);

    put(
        &folder,
        &[
            ("inc/demo.h", "/* header */\n"),
            ("src/demo.c", "/* source */\n"),
            ("NOTICE", "notice\n"),
        ],
    );
    put(&folder, files);
    fs::write(
        folder.join("libnima.yml"),
        format!("name: {name}\nplatform: {platform}\n{body}"),
    )
    .unwrap();

    folder
}

/// A second version of a library in another folder.
pub fn version(root: &Path, tag: &str, name: &str, body: &str, files: &[(&str, &str)]) -> PathBuf {
    let holder = root.join(tag);
    library(&holder, name, body, files)
}

/// A run with what it is given, on a project, with nobody at the keyboard.
pub fn request(project: &Path, libraries: &[&Path]) -> Request {
    Request {
        libraries: libraries
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        project: Some(project.to_path_buf()),
        ..Request::default()
    }
}

/// Install one library into a folder of a project. Returns the exit code and what the run said.
pub fn install(project: &Path, library: &Path, folder: &str) -> (i32, Watched) {
    let mut ui = Watched::default();
    let mut asked = request(project, &[library]);
    asked.folder = Some(folder.to_string());

    (flow::run(&asked, &mut ui), ui)
}

/// Install, and say so loudly when it does not go in.
pub fn installed(project: &Path, library: &Path, folder: &str) -> Watched {
    let (code, ui) = install(project, library, folder);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());

    ui
}

/// Take libraries out of a project. Returns the exit code and what the run said.
pub fn uninstall(project: &Path, names: &[&str]) -> (i32, Watched) {
    let mut ui = Watched::default();
    let asked = Request {
        uninstall: names.iter().map(ToString::to_string).collect(),
        project: Some(project.to_path_buf()),
        platform: Some(Platform::Stm32),
        ..Request::default()
    };

    (flow::run(&asked, &mut ui), ui)
}

/// The record's entry for each library.
pub fn record(project: &Path) -> serde_json::Map<String, Value> {
    libnima_core::record::installed_libraries(project)
}

pub fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

pub fn write(path: impl AsRef<Path>, text: &str) {
    fs::create_dir_all(path.as_ref().parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A list in the record, as plain text.
pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect()
}

/// The copies a run put aside in a folder of a file's name.
pub fn backups(folder: &Path, name: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let shown = path.file_name().unwrap().to_string_lossy().into_owned();
            shown.starts_with(&format!("{name}.")) && shown.ends_with(".bak")
        })
        .collect();

    found.sort();

    found
}

pub const STM32_CMAKELISTS: &str = "cmake_minimum_required(VERSION 3.22)\nproject(Proj C ASM)\nadd_executable(${CMAKE_PROJECT_NAME} Core/Src/main.c)\n";
pub const ZEPHYR_CMAKELISTS: &str = "cmake_minimum_required(VERSION 3.20.0)\nfind_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})\nproject(app)\n\ntarget_sources(app PRIVATE src/main.c)\n";
pub const ESP32_CMAKELISTS: &str = "cmake_minimum_required(VERSION 3.16)\n\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nproject(app)\n";
pub const ESP32_MAIN: &str =
    "idf_component_register(SRCS \"main.c\"\n                    INCLUDE_DIRS \".\")\n";
pub const PLATFORMIO_INI: &str = "[env:native]\nplatform = native\n";

/// A project of the platform named, on disk, with any more files given, and with CRLF line endings when asked.
pub fn project_of(root: &Path, platform: &str, files: &[(&str, &str)], crlf: bool) -> PathBuf {
    let project = root.join(format!("{platform}-project"));
    let own: &[(&str, &str)] = match platform {
        "stm32" => &[
            ("CMakeLists.txt", STM32_CMAKELISTS),
            ("Core/Src/main.c", "/* main */\n"),
            ("Core/Inc/main.h", "/* main */\n"),
        ],
        "zephyr" => &[
            ("CMakeLists.txt", ZEPHYR_CMAKELISTS),
            ("prj.conf", "CONFIG_GPIO=y\n"),
            ("src/main.c", "int main(void) { return 0; }\n"),
        ],
        "esp32" => &[
            ("CMakeLists.txt", ESP32_CMAKELISTS),
            ("main/CMakeLists.txt", ESP32_MAIN),
            ("main/main.c", "void app_main(void) {}\n"),
        ],
        "pio" => &[
            ("platformio.ini", PLATFORMIO_INI),
            ("src/main.c", "int main(void) { return 0; }\n"),
        ],
        "arduino" => &[("arduino-project.ino", "void setup() {}\nvoid loop() {}\n")],
        _ => panic!("no project for {platform}"),
    };

    for (name, text) in own.iter().chain(files) {
        let text = if crlf {
            text.replace('\n', "\r\n")
        } else {
            (*text).to_string()
        };

        write(project.join(name), &text);
    }

    project
}

/// A library on disk for a platform, or several written as a list, with its code named after it, and more of its
/// libnima.yml in `body`.
pub fn library_of(root: &Path, name: &str, platform: &str, body: &str) -> PathBuf {
    let folder = root.join("repos").join(name);

    put(
        &folder,
        &[
            (
                &format!("src/{name}.h"),
                &format!("/**\n * @version 1.0.0\n */\nint {name}_answer(void);\n"),
            ),
            (
                &format!("src/{name}.c"),
                "int answer(void) { return 42; }\n",
            ),
        ],
    );
    fs::write(
        folder.join("libnima.yml"),
        format!(
            "name: {name}\ndescription: A library for the tests\nrepository: https://github.com/someone/{name}\nplatform: {platform}\nheaders:\n  - src/{name}.h\nsources:\n  - src/{name}.c\n{body}"
        ),
    )
    .unwrap();

    folder
}

/// An STM32 project folder with nothing in it but a CubeMX header, so it reads as what it is.
pub fn stm32_project(root: &Path) -> PathBuf {
    let project = root.join("Proj");
    put(&project, &[("Core/Src/main.c", "/* main */\n")]);

    project
}

pub const CPROJECT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n",
    "<?fileVersion 4.0.0?><cproject>\n",
    "\t<storageModule moduleId=\"cdtBuildSystem\">\n",
    "\t\t<configuration name=\"Debug\">\n",
    "\t\t\t<toolChain>\n",
    "\t\t\t\t<tool>\n",
    "\t\t\t\t\t<option id=\"as\" superClass=\"x.assembler.option.definedsymbols\" valueType=\"definedSymbols\">\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"DEBUG\"/>\n",
    "\t\t\t\t\t</option>\n",
    "\t\t\t\t</tool>\n",
    "\t\t\t\t<tool>\n",
    "\t\t\t\t\t<option id=\"da\" superClass=\"x.c.compiler.option.definedsymbols\" valueType=\"definedSymbols\">\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"DEBUG\"/>\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"USE_HAL_DRIVER\"/>\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"STM32G431xx\"/>\n",
    "\t\t\t\t\t</option>\n",
    "\t\t\t\t\t<option id=\"a\" superClass=\"x.c.compiler.option.includepaths\" valueType=\"includePath\">\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"../Core/Inc\"/>\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"../Drivers/CMSIS/Include\"/>\n",
    "\t\t\t\t\t</option>\n",
    "\t\t\t\t\t<option id=\"opt\" superClass=\"x.c.compiler.option.optimization.level\"/>\n",
    "\t\t\t\t</tool>\n",
    "\t\t\t</toolChain>\n",
    "\t\t</configuration>\n",
    "\t\t<configuration name=\"Release\">\n",
    "\t\t\t<toolChain>\n",
    "\t\t\t\t<tool>\n",
    "\t\t\t\t\t<option id=\"db\" superClass=\"x.c.compiler.option.definedsymbols\" valueType=\"definedSymbols\">\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"USE_HAL_DRIVER\"/>\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"STM32G431xx\"/>\n",
    "\t\t\t\t\t</option>\n",
    "\t\t\t\t\t<option id=\"b\" superClass=\"x.c.compiler.option.includepaths\" valueType=\"includePath\">\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"../Core/Inc\"/>\n",
    "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"../Drivers/CMSIS/Include\"/>\n",
    "\t\t\t\t\t</option>\n",
    "\t\t\t\t</tool>\n",
    "\t\t\t</toolChain>\n",
    "\t\t</configuration>\n",
    "\t</storageModule>\n",
    "</cproject>\n",
);

/// Backslash paths, which is what uVision writes once a path has been added through its own dialogs.
pub const UVPROJX: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n",
    "<Project>\n",
    "  <Targets>\n",
    "    <Target>\n",
    "      <TargetOption>\n",
    "        <TargetArmAds>\n",
    "          <Cads>\n",
    "            <VariousControls>\n",
    "              <Define>USE_HAL_DRIVER,STM32G431xx</Define>\n",
    "              <IncludePath>..\\Core\\Inc</IncludePath>\n",
    "            </VariousControls>\n",
    "          </Cads>\n",
    "          <Aads>\n",
    "            <VariousControls>\n",
    "              <Define></Define>\n",
    "            </VariousControls>\n",
    "          </Aads>\n",
    "        </TargetArmAds>\n",
    "      </TargetOption>\n",
    "      <Groups>\n",
    "        <Group>\n",
    "          <GroupName>Application/User/Core</GroupName>\n",
    "          <Files>\n",
    "            <File>\n",
    "              <FileName>main.c</FileName>\n",
    "              <FileType>1</FileType>\n",
    "              <FilePath>..\\Core\\Src\\main.c</FilePath>\n",
    "            </File>\n",
    "          </Files>\n",
    "        </Group>\n",
    "      </Groups>\n",
    "    </Target>\n",
    "  </Targets>\n",
    "</Project>\n",
);

/// Four space indentation, groups nested inside groups, and the two slashes mixed: forward in the include
/// paths, back in the file list. All three come straight from a CubeMX generated EWARM project.
pub const EWP: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<project>\n",
    "    <fileVersion>4</fileVersion>\n",
    "    <configuration>\n",
    "        <name>Proj</name>\n",
    "        <settings>\n",
    "            <name>ICCARM</name>\n",
    "            <data>\n",
    "                <option>\n",
    "                    <name>CCDefines</name>\n",
    "                    <state>USE_HAL_DRIVER</state>\n",
    "                    <state>STM32G431xx</state>\n",
    "                </option>\n",
    "                <option>\n",
    "                    <name>CCIncludePath2</name>\n",
    "                    <state>$PROJ_DIR$/../Core/Inc</state>\n",
    "                    <state>$PROJ_DIR$/../Drivers/CMSIS/Include</state>\n",
    "                </option>\n",
    "            </data>\n",
    "        </settings>\n",
    "    </configuration>\n",
    "    <group>\n",
    "        <name>Application</name>\n",
    "        <group>\n",
    "            <name>User</name>\n",
    "            <group>\n",
    "                <name>Core</name>\n",
    "                <file>\n",
    "                    <name>$PROJ_DIR$\\..\\Core\\Src\\main.c</name>\n",
    "                </file>\n",
    "            </group>\n",
    "        </group>\n",
    "    </group>\n",
    "    <group>\n",
    "        <name>Drivers</name>\n",
    "        <file>\n",
    "            <name>$PROJ_DIR$\\..\\Drivers\\stm32g4xx_hal.c</name>\n",
    "        </file>\n",
    "        <file>\n",
    "            <name>$PROJ_DIR$\\..\\Drivers\\stm32g4xx_hal_rcc.c</name>\n",
    "        </file>\n",
    "        <file>\n",
    "            <name>$PROJ_DIR$\\..\\Drivers\\stm32g4xx_hal_gpio.c</name>\n",
    "        </file>\n",
    "        <file>\n",
    "            <name>$PROJ_DIR$\\..\\Drivers\\stm32g4xx_hal_tim.c</name>\n",
    "        </file>\n",
    "    </group>\n",
    "</project>\n",
);

/// The workspace, which is what says which .ewp is the real project.
pub const EWW: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<workspace>\n",
    "  <project>\n",
    "    <path>$WS_DIR$\\Proj.ewp</path>\n",
    "  </project>\n",
    "  <batchBuild />\n",
    "</workspace>\n",
);

/// The companion IAR keeps beside every .ewp, carrying the same file tree for the analysis tools and no
/// include paths at all.
pub const EWT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<project>\n",
    "    <fileVersion>4</fileVersion>\n",
    "    <configuration>\n",
    "        <name>Proj</name>\n",
    "    </configuration>\n",
    "    <group>\n",
    "        <name>Drivers</name>\n",
    "        <file>\n",
    "            <name>$PROJ_DIR$\\..\\Drivers\\stm32g4xx_hal.c</name>\n",
    "        </file>\n",
    "    </group>\n",
    "</project>\n",
);

/// The Makefile CubeMX writes when the toolchain is set to Makefile, cut down to the parts that matter
/// here. The last entry of each list has no backslash, and CubeMX leaves two trailing spaces after the last C
/// source.
pub const MAKEFILE: &str = concat!(
    "######################################\n",
    "# target\n",
    "######################################\n",
    "TARGET = Proj\n",
    "\n",
    "######################################\n",
    "# source\n",
    "######################################\n",
    "# C sources\n",
    "C_SOURCES =  \\\n",
    "Core/Src/main.c \\\n",
    "Core/Src/stm32g4xx_it.c \\\n",
    "Drivers/STM32G4xx_HAL_Driver/Src/stm32g4xx_hal.c \\\n",
    "Core/Src/system_stm32g4xx.c  \n",
    "\n",
    "# ASM sources\n",
    "ASM_SOURCES =  \\\n",
    "startup_stm32g431xx.s\n",
    "\n",
    "# C defines\n",
    "C_DEFS =  \\\n",
    "-DUSE_HAL_DRIVER \\\n",
    "-DSTM32G431xx\n",
    "\n",
    "# C includes\n",
    "C_INCLUDES =  \\\n",
    "-ICore/Inc \\\n",
    "-IDrivers/STM32G4xx_HAL_Driver/Inc \\\n",
    "-IDrivers/CMSIS/Include\n",
    "\n",
    "CFLAGS += $(MCU) $(C_DEFS) $(C_INCLUDES) $(OPT) -Wall\n",
);

pub const CMAKE: &str = "cmake_minimum_required(VERSION 3.22)\nproject(Proj C ASM)\nadd_executable(${CMAKE_PROJECT_NAME} Core/Src/main.c)\n";

/// Which IDE files an STM32 project has, and the line ending it was written with.
#[derive(Default, Clone, Copy)]
pub struct Ides {
    pub cmake: bool,
    pub cubeide: bool,
    pub keil: bool,
    pub iar: bool,
    pub makefile: bool,
    pub crlf: bool,
    pub iar_backup: bool,
    pub keil_backup: bool,
}

/// An STM32 project with these IDE files.
pub fn project_with(root: &Path, ides: Ides) -> PathBuf {
    let project = stm32_project(root);
    let put_text = |name: &str, text: &str| {
        let text = if ides.crlf {
            text.replace('\n', "\r\n")
        } else {
            text.to_string()
        };
        write(project.join(name), &text);
    };

    if ides.cmake {
        put_text("CMakeLists.txt", CMAKE);
    }

    if ides.cubeide {
        put_text(".project", "<projectDescription/>\n");
        put_text(".cproject", CPROJECT);
    }

    if ides.keil {
        put_text("MDK/Proj.uvprojx", UVPROJX);

        if ides.keil_backup {
            put_text("MDK/Backup of Proj.uvprojx", UVPROJX);
        }
    }

    if ides.iar {
        put_text("EWARM/Proj.ewp", EWP);
        put_text("EWARM/Proj.ewt", EWT);
        put_text("EWARM/Project.eww", EWW);

        if ides.iar_backup {
            put_text("EWARM/Backup of Proj.ewp", EWP);
        }
    }

    if ides.makefile {
        put_text("Makefile", MAKEFILE);
    }

    project
}
