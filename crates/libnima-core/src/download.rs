//! Getting a library ready to install: fetched from GitHub, or unpacked from the zip GitHub offers for download.
//!
//! Only the files the manifest actually lists are downloaded. GitHub can only hand out a zip of an entire repository,
//! so the zip is avoided: the manifest is read first, then each listed file is fetched on its own. For a library whose
//! repo carries a vendored test framework, that is the difference between a few hundred kilobytes and a few.
//!
//! A private repository needs a GitHub token that can read it: given with --key, or in GITHUB_TOKEN or GH_TOKEN. With
//! one, every file comes from the API rather than from raw.githubusercontent, which cannot see a private repository at
//! all.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value, json};
use ureq::Agent;
use ureq::config::RedirectAuthHeaders;
use ureq::tls::{RootCerts, TlsConfig};

use crate::manifest::{
    self, INSTALLER_NAME, MANIFEST_NAME, MANIFEST_NAMES, expand_known, is_pattern, new_form,
    unsafe_path,
};
use crate::paths;
use crate::yaml::{Mapping, Node};

const DEFAULT_OWNER: &str = "nimaltd";

const API: &str = "https://api.github.com";
const RAW: &str = "https://raw.githubusercontent.com";

/// What git calls the repository's default branch, whatever its name: main, master, develop, the one GitHub marks
/// "default". raw.githubusercontent.com and the API both take it as a ref.
const DEFAULT_REF: &str = "HEAD";

const TIMEOUT: Duration = Duration::from_secs(30);

/// Asking which commit a ref is now is worth a few seconds, not thirty: without the answer the files still come from
/// the same ref, only not pinned to it.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);

/// Tries in all for one file, when the connection drops, resets, times out or is cut off part way, or GitHub answers
/// 5xx. 1, 2, 4 and 8 seconds between them, so a link that is down for a few seconds, or throttled, gets through.
/// Without a token they take turns between raw.githubusercontent.com and api.github.com, two hosts on different
/// addresses, so one that is filtered or failing does not decide alone.
const ATTEMPTS: u32 = 5;

/// The first wait between two tries of one file, doubled each time after it.
const PAUSE: Duration = Duration::from_secs(1);

/// Where a token is looked for, after --key, in this order. GitHub Actions sets the first and the gh command reads the
/// second, so a machine set up for either works as it is.
const TOKEN_VARIABLES: [&str; 2] = ["GITHUB_TOKEN", "GH_TOKEN"];

/// Where the user makes and edits a fine-grained token, named in every message about one that cannot read.
const TOKENS_PAGE: &str = "https://github.com/settings/personal-access-tokens";

/// The file as it is, not wrapped in JSON and base64 the way the API hands a file out by default.
const RAW_MEDIA: &str = "application/vnd.github.raw";

/// A library could not be fetched, with a reason worth showing the user.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadError {
    pub message: String,
    /// GitHub answered 404: no such file, branch or repository, or a private one.
    pub not_found: bool,
}

impl DownloadError {
    pub fn new(message: impl Into<String>) -> DownloadError {
        DownloadError {
            message: message.into(),
            not_found: false,
        }
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DownloadError {}

/// One failed request: GitHub's answer, or none at all.
enum Failure {
    Status { code: u16, permissions: String },
    Connection(String),
}

/// The system's proxy, when the user set one in Windows' own settings. HTTPS_PROXY comes first.
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;

    if enabled == 0 {
        return None;
    }

    let server: String = key.get_value("ProxyServer").ok()?;
    proxy_from_setting(&server)
}

#[cfg(not(windows))]
fn system_proxy() -> Option<String> {
    None
}

/// The proxy for https in Windows' setting: host:port for every protocol, or http=host:port;https=host:port.
#[cfg_attr(not(windows), allow(dead_code))]
fn proxy_from_setting(server: &str) -> Option<String> {
    let server = server.trim();

    if server.is_empty() {
        return None;
    }

    let found = if server.contains('=') {
        server
            .split(';')
            .find_map(|part| part.trim().strip_prefix("https="))
            .or_else(|| {
                server
                    .split(';')
                    .find_map(|part| part.trim().strip_prefix("http="))
            })?
    } else {
        server
    };

    Some(if found.contains("://") {
        found.to_string()
    } else {
        format!("http://{found}")
    })
}

/// An agent that asks for answers rather than raising errors, trusts what the OS trusts, goes through a proxy when one
/// is set, and takes a token only as far as the host it was for.
pub fn agent(timeout: Duration) -> Agent {
    let proxy = ureq::Proxy::try_from_env()
        .or_else(|| system_proxy().and_then(|url| ureq::Proxy::new(&url).ok()));

    agent_through(proxy, timeout)
}

fn agent_through(proxy: Option<ureq::Proxy>, timeout: Duration) -> Agent {
    Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_recv_body(Some(timeout * 10))
        .redirect_auth_headers(RedirectAuthHeaders::SameHost)
        .user_agent(format!("libnima/{}", crate::VERSION))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .proxy(proxy)
        .build()
        .into()
}

fn quote_path(text: &str) -> String {
    let mut out = String::new();

    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }

    out
}

fn quote_all(text: &str) -> String {
    quote_path(text).replace('/', "%2F")
}

/// The token, as a message names it: the one given with --key, or the one in a variable.
fn named(place: &str) -> String {
    if place == "--key" {
        "the token given with --key".to_string()
    } else {
        format!("the token in {place}")
    }
}

/// What to say when a manifest is not found, in case the repository is private.
fn private_hint(token: Option<&(String, String)>) -> String {
    match token {
        Some((place, _)) => format!(
            " If the repository is private, {} cannot see it: add it under the token's Repository access at {TOKENS_PAGE}.",
            named(place)
        ),
        None => " If the repository is private, give a token that can read it with --key, or in GITHUB_TOKEN.".to_string(),
    }
}

/// What to do when GitHub cannot be reached, said once wherever that happens.
fn offline_hint() -> String {
    "Run it again, as a bad connection often passes. Behind a proxy or a VPN, set HTTPS_PROXY to it, or use the system \
     proxy on Windows. Or download each library with Code, Download ZIP, from wherever GitHub opens, and give the zips \
     on the command line: libnima spif-main.zip osal-main.zip"
        .to_string()
}

/// Whether trying again could get a different answer.
///
/// A dropped connection, a timeout or a 5xx could. A 404 is GitHub's answer and the same every time, which the
/// manifest's own fallback relies on, and so is a 401 or a 403 to a token. Without a token, a 403 or a 429 is the API's
/// limit on requests, and the other host may still answer.
fn passing(failure: &Failure, token: bool) -> bool {
    match failure {
        Failure::Connection(_) => true,
        Failure::Status { code, .. } => *code >= 500 || (!token && (*code == 403 || *code == 429)),
    }
}

/// Owner, repository name and ref from a source: "owner/name@v1.2.0", "name", or a GitHub URL, a /tree/<ref> one
/// included. The ref is None when none is named, which means the repository's default branch.
pub fn split_ref(source: &str) -> Result<(String, String, Option<String>), DownloadError> {
    let mut text = source.trim().to_string();
    let mut reference = None;

    if text.starts_with("http://") || text.starts_with("https://") {
        let trimmed = text.trim_end_matches('/').to_string();
        let parts: Vec<&str> = trimmed.split('/').collect();

        // https://github.com/owner/repo/tree/v1.2.0, as a browser shows a tag.
        if parts.len() > 6 && parts[5] == "tree" {
            reference = Some(parts[6..].join("/"));
            text = parts[..5].join("/");
        }
    } else if let Some((head, tail)) = text.clone().split_once('@') {
        if tail.is_empty() || tail.contains("..") || tail.starts_with('-') {
            return Err(DownloadError::new(format!(
                "{} names no usable tag, branch or commit after the @.",
                crate::yaml::quoted(source)
            )));
        }

        text = head.to_string();
        reference = Some(tail.to_string());
    }

    let (owner, repo) = split(&text)?;

    Ok((owner, repo, reference))
}

/// Owner and repository name from a bare name, an owner/name pair, or a URL.
fn split(source: &str) -> Result<(String, String), DownloadError> {
    let text = source.trim().trim_end_matches('/');
    let text = text.strip_suffix(".git").unwrap_or(text);

    if text.starts_with("http://") || text.starts_with("https://") {
        let parts: Vec<&str> = text
            .split('/')
            .filter(|part| !part.is_empty() && *part != "https:" && *part != "http:")
            .collect();

        if parts.len() < 3 {
            return Err(DownloadError::new(format!(
                "{source} does not look like a GitHub repository URL."
            )));
        }

        return Ok((parts[1].to_string(), parts[2].to_string()));
    }

    if let Some((owner, repo)) = text.split_once('/') {
        if !owner.is_empty() && !repo.is_empty() {
            return Ok((owner.to_string(), repo.to_string()));
        }

        return Err(DownloadError::new(format!(
            "Could not work out the repository from {}.",
            crate::yaml::quoted(source)
        )));
    }

    if text.is_empty() {
        return Err(DownloadError::new("No library name given."));
    }

    Ok((DEFAULT_OWNER.to_string(), text.to_string()))
}

/// The headers and the sources of a manifest or an option.
fn code_of(data: &Mapping) -> Vec<Node> {
    let mut found = Vec::new();

    for key in ["headers", "sources"] {
        if let Some(list) = data.get(key).and_then(Node::as_list) {
            found.extend_from_slice(list);
        }
    }

    found
}

/// Every repository path or pattern a parsed manifest refers to.
///
/// An entry is either a plain path, a folder, a wildcard, or a {from, to} pair, and only the "from" side names something
/// to download.
fn listed_entries(data: &Mapping) -> Vec<String> {
    let mut listed = code_of(data);
    let options = data.given("options").and_then(Node::as_map);
    let bodies: Vec<&Mapping> = options
        .map(|options| {
            options
                .entries
                .iter()
                .filter_map(|entry| entry.value.as_map())
                .collect()
        })
        .unwrap_or_default();

    // Every option's files, chosen or not. Which are taken is asked once the library is on disk, and the answer must
    // not need a second download.
    for body in &bodies {
        listed.extend(code_of(body));
    }

    if let Some(once) = data.get("once").and_then(Node::as_list) {
        listed.extend_from_slice(once);
    }

    match data
        .given("extras")
        .and_then(Node::as_list)
        .filter(|extras| !extras.is_empty())
    {
        Some(extras) => listed.extend_from_slice(extras),
        None => {
            listed.push(Node::string("LICENSE.md", 0));
            listed.push(Node::string("NOTICE", 0));
        }
    }

    // The manifest carried for a library that has none, the library's own options' included.
    let mut needed: Vec<&Node> = data
        .given("requires")
        .and_then(Node::as_map)
        .and_then(|requires| requires.get("libraries"))
        .and_then(Node::as_list)
        .map(|list| list.iter().collect())
        .unwrap_or_default();

    for body in &bodies {
        if let Some(list) = body.get("libraries").and_then(Node::as_list) {
            needed.extend(list);
        }
    }

    let mut entries: Vec<String> = Vec::new();

    for item in &listed {
        entries.push(match item.as_map() {
            // A {from, to} pair names the file by its from, or by its to or its root alone, when the library's path is
            // the same.
            Some(map) => {
                let Some(source) = map
                    .given("from")
                    .or_else(|| map.given("to"))
                    .or_else(|| map.given("root"))
                    .filter(|source| source.truthy())
                else {
                    continue;
                };

                source.text()
            }
            None => item.text(),
        });
    }

    for dependency in needed {
        if let Some(carried) = dependency
            .as_map()
            .and_then(|map| map.given("manifest"))
            .filter(|carried| carried.truthy())
        {
            entries.push(carried.text());
        }
    }

    entries
}

/// The token for a run and where it was found: --key, or else the first of the variables that holds one.
fn token_of(
    key: Option<&str>,
    variable: impl Fn(&str) -> Option<String>,
) -> Option<(String, String)> {
    let given = key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(|key| ("--key".to_string(), key.to_string()));

    given.or_else(|| {
        TOKEN_VARIABLES.iter().find_map(|name| {
            let value = variable(name)?;
            let value = value.trim();

            (!value.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
    })
}

/// A client for one run: what GitHub answered is kept, the commit a ref was and a manifest's bytes, so a library's
/// manifest read first for its platform is not asked for again when the files follow.
pub struct Client {
    api: String,
    raw: String,
    token: Option<(String, String)>,
    agent: Agent,
    quick: Agent,
    pause: Duration,
    resolved: HashMap<(String, String, Option<String>), Option<String>>,
    manifests: HashMap<(String, String, String), (Vec<u8>, String)>,
    origins: HashMap<PathBuf, Map<String, Value>>,
}

impl Client {
    /// `key` is a token given for this run, which comes before the environment.
    pub fn new(key: Option<&str>) -> Client {
        Client {
            api: API.to_string(),
            raw: RAW.to_string(),
            token: token_of(key, |name| std::env::var(name).ok()),
            agent: agent(TIMEOUT),
            quick: agent(RESOLVE_TIMEOUT),
            pause: PAUSE,
            resolved: HashMap::new(),
            manifests: HashMap::new(),
            origins: HashMap::new(),
        }
    }

    /// A client asking a test's own server rather than GitHub, with no proxy, the token given and no waiting between
    /// tries. The raw files are under /raw on it.
    #[doc(hidden)]
    pub fn at(origin: &str, token: Option<(&str, &str)>) -> Client {
        Client {
            api: origin.to_string(),
            raw: format!("{origin}/raw"),
            token: token.map(|(place, value)| (place.to_string(), value.to_string())),
            agent: agent_through(None, TIMEOUT),
            quick: agent_through(None, RESOLVE_TIMEOUT),
            pause: Duration::ZERO,
            resolved: HashMap::new(),
            manifests: HashMap::new(),
            origins: HashMap::new(),
        }
    }

    /// Use this token from now on, as one given with --key.
    pub fn use_token(&mut self, key: &str) {
        self.token = Some(("--key".to_string(), key.trim().to_string()));
    }

    /// Where the library fetched into root came from, or nothing when it was not fetched.
    pub fn origin(&self, root: &Path) -> Map<String, Value> {
        self.origins
            .get(&paths::resolve(root))
            .cloned()
            .unwrap_or_default()
    }

    /// One request for one file, its bytes read to the end.
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, Failure> {
        let mut request = self.agent.get(url);

        if let Some((_, token)) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }

        if let Some(accept) = accept {
            request = request.header("Accept", accept);
        }

        let mut response = request
            .call()
            .map_err(|error| Failure::Connection(error.to_string()))?;
        let code = response.status().as_u16();

        if code >= 400 {
            let permissions = response
                .headers()
                .get("X-Accepted-GitHub-Permissions")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();

            return Err(Failure::Status { code, permissions });
        }

        let mut body = Vec::new();

        response
            .body_mut()
            .as_reader()
            .read_to_end(&mut body)
            .map_err(|error| Failure::Connection(error.to_string()))?;

        Ok(body)
    }

    /// The commit a ref names now, as a full SHA, or None when the API cannot say.
    ///
    /// Every file is then fetched from that commit, so a push that lands half way through a download cannot mix two
    /// versions, and the record says exactly what was installed. Without an answer, from a limit on requests or a
    /// filtered host, the files still come from the ref itself.
    pub fn resolve(&mut self, owner: &str, repo: &str, reference: Option<&str>) -> Option<String> {
        let key = (
            owner.to_string(),
            repo.to_string(),
            reference.map(str::to_string),
        );

        if let Some(found) = self.resolved.get(&key) {
            return found.clone();
        }

        let url = format!(
            "{}/repos/{owner}/{repo}/commits/{}",
            self.api,
            quote_all(reference.unwrap_or(DEFAULT_REF))
        );

        let mut request = self
            .quick
            .get(&url)
            .header("Accept", "application/vnd.github.sha");

        if let Some((_, token)) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }

        let found = request
            .call()
            .ok()
            .filter(|response| response.status().as_u16() == 200)
            .and_then(|mut response| response.body_mut().read_to_string().ok())
            .map(|text| text.trim().to_string())
            .filter(|sha| {
                sha.len() == 40
                    && sha
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            });

        self.resolved.insert(key, found.clone());

        found
    }

    /// Fetch one file's bytes.
    fn fetch_file(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
        path: &str,
    ) -> Result<Vec<u8>, DownloadError> {
        let api = format!(
            "{}/repos/{owner}/{repo}/contents/{}?ref={}",
            self.api,
            quote_path(path),
            quote_all(reference)
        );
        let raw = format!("{}/{owner}/{repo}/{reference}/{path}", self.raw);

        // A private repository is only reachable through the API, with the token.
        let urls: Vec<&str> = if self.token.is_some() {
            vec![&api]
        } else {
            vec![&raw, &api]
        };

        let mut attempt = 1;

        loop {
            let url = urls[(attempt - 1) as usize % urls.len()];

            match self.get(url, (url == api).then_some(RAW_MEDIA)) {
                Ok(data) => return Ok(data),
                Err(failure) => {
                    if !passing(&failure, self.token.is_some()) || attempt == ATTEMPTS {
                        return Err(self.report(failure, owner, repo, reference, path));
                    }

                    thread::sleep(self.pause * (1 << (attempt - 1)));
                    attempt += 1;
                }
            }
        }
    }

    /// The error that says, in words, what went wrong with one file.
    fn report(
        &self,
        failure: Failure,
        owner: &str,
        repo: &str,
        reference: &str,
        path: &str,
    ) -> DownloadError {
        let place = self.token.as_ref().map(|(place, _)| place.as_str());

        match failure {
            Failure::Status { code: 401, .. } if place.is_some() => DownloadError::new(format!(
                "GitHub did not accept {}. It may have expired, been revoked, or been copied with a character missing. \
                 Make a new one at {TOKENS_PAGE}.",
                named(place.unwrap_or_default())
            )),
            // The usual mistake with a fine-grained token: the repository is chosen, so the token can see it, but its
            // files need a permission of their own, and GitHub says which in a header.
            Failure::Status {
                code: 403,
                permissions,
            } if place.is_some() && permissions.contains("contents=read") => {
                let name = named(place.unwrap_or_default());
                let mut capital = name.chars();
                let name = match capital.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), capital.as_str()),
                    None => name,
                };

                DownloadError::new(format!(
                    "{name} can see {owner}/{repo}, but cannot read its files: GitHub wants the Contents permission, and \
                     the token has only Metadata.\n\
                     Edit the token, and its value stays the same, so nothing else changes:\n\
                     \x20 1. Open {TOKENS_PAGE} and click the token's name.\n\
                     \x20 2. Click Edit.\n\
                     \x20 3. In Permissions, set Contents to Read-only. In the older layout that is under\n\
                     \x20    Repository permissions; in the newer one, click + Add permissions and tick Contents.\n\
                     \x20 4. Click Update at the bottom of the page.\n\
                     Then run this again as it was."
                ))
            }
            Failure::Status { code: 403, .. } => DownloadError::new(match place {
                Some(place) => format!(
                    "GitHub refused access to {owner}/{repo} (HTTP 403) with {}. It may not be allowed to read this \
                     repository: check its Repository access and its Contents permission at {TOKENS_PAGE}.",
                    named(place)
                ),
                None => format!(
                    "GitHub refused access to {owner}/{repo} (HTTP 403). That is usually its limit on requests without a \
                     token: wait, or give one with --key."
                ),
            }),
            Failure::Status { code: 404, .. } => {
                // GitHub answers a private repository it will not show you with 404, the same as one that does not
                // exist. Only the manifest is asked for first, so that is where a missing token shows up.
                let hint = if MANIFEST_NAMES.contains(&path) {
                    private_hint(self.token.as_ref())
                } else {
                    String::new()
                };

                DownloadError {
                    message: format!(
                        "{path} does not exist in {owner}/{repo} at {reference}.{hint}"
                    ),
                    not_found: true,
                }
            }
            Failure::Status { code, .. } => DownloadError::new(format!(
                "Could not download {path} from {owner}/{repo}: HTTP {code}."
            )),
            // Not reached, or the connection dropped, reset, timed out or was cut short.
            Failure::Connection(reason) => DownloadError::new(format!(
                "Could not fetch {path} from {owner}/{repo}, after {ATTEMPTS} tries: {reason}.\n{}",
                offline_hint()
            )),
        }
    }

    /// The bytes of the manifest, and its name.
    ///
    /// libnima.yml is asked for first, then the names lib-installer read. Only a 404 moves on. Any other failure, a
    /// refused token or no network, would fail the same way on the next try, and would then be reported as the wrong
    /// problem.
    ///
    /// `asked` is the ref as the user gave it, for the message: None for the default branch, which `reference` is then
    /// the commit of.
    fn manifest_of(
        &mut self,
        owner: &str,
        repo: &str,
        reference: &str,
        asked: Option<&str>,
    ) -> Result<(Vec<u8>, String), DownloadError> {
        let key = (owner.to_string(), repo.to_string(), reference.to_string());

        if let Some(found) = self.manifests.get(&key) {
            return Ok(found.clone());
        }

        for name in MANIFEST_NAMES {
            match self.fetch_file(owner, repo, reference, name) {
                Ok(data) => {
                    let found = (data, name.to_string());
                    self.manifests.insert(key, found.clone());

                    return Ok(found);
                }
                Err(error) if error.not_found => continue,
                Err(error) => return Err(error),
            }
        }

        let place = match asked {
            Some(asked) => format!("at {asked}"),
            None => "on its default branch".to_string(),
        };

        Err(DownloadError {
            message: format!(
                "{MANIFEST_NAME} does not exist in {owner}/{repo} {place}.{}",
                private_hint(self.token.as_ref())
            ),
            not_found: true,
        })
    }

    /// Every file path in the repository, from the GitHub API.
    ///
    /// Needed because a folder or a wildcard cannot be resolved against raw.githubusercontent, which only serves one
    /// named file at a time. None when the listing is unavailable, so the caller can fall back to treating each entry
    /// as a literal path.
    fn tree(&self, owner: &str, repo: &str, reference: &str) -> Option<Vec<String>> {
        let url = format!(
            "{}/repos/{owner}/{repo}/git/trees/{}?recursive=1",
            self.api,
            quote_all(reference)
        );
        let body = self.get(&url, Some("application/vnd.github+json")).ok()?;
        let data: Value = serde_json::from_slice(&body).ok()?;

        // Enormous repository. Literal paths still work, patterns would quietly miss files, so it is better to admit to
        // knowing nothing.
        if data.get("truncated").and_then(Value::as_bool) == Some(true) {
            return None;
        }

        Some(
            data.get("tree")?
                .as_array()?
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("blob"))
                .filter_map(|item| item.get("path").and_then(Value::as_str).map(str::to_string))
                .collect(),
        )
    }

    /// The bytes of a library's manifest on GitHub and its name, and nothing else of it: what a run reads first, for the
    /// platform the library names.
    pub fn peek(
        &mut self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<(Vec<u8>, String), DownloadError> {
        let (owner, repo, named_ref) = split_ref(source)?;
        let asked = one_ref(source, named_ref, reference)?;
        let commit = self.resolve(&owner, &repo, asked.as_deref());
        let found = commit
            .or_else(|| asked.clone())
            .unwrap_or_else(|| DEFAULT_REF.to_string());

        self.manifest_of(&owner, &repo, &found, asked.as_deref())
    }

    /// The version of a library on GitHub, without fetching the library: from the @version tag of its first header, or what
    /// its file says when that has none. None when GitHub cannot say.
    pub fn newest_version(&mut self, source: &str) -> Option<String> {
        let (raw, name) = self.peek(source, None).ok()?;
        let text = String::from_utf8(raw).ok()?;
        let (text, _) = new_form(&name, &text).ok()?;
        let node = manifest::parse(&text).ok()?;
        let data = node.as_map()?;
        let written = data.given("version").map(Node::text);
        let first = data
            .given("headers")
            .and_then(Node::as_list)
            .and_then(|headers| headers.first())
            .map(|header| match header.as_map() {
                Some(map) => map
                    .given("from")
                    .or_else(|| map.given("to"))
                    .map_or_else(String::new, Node::text),
                None => header.text(),
            })
            .filter(|path| !path.is_empty() && !is_pattern(path));

        let in_header = first.and_then(|path| {
            let (owner, repo, named_ref) = split_ref(source).ok()?;
            let asked = one_ref(source, named_ref, None).ok()?;
            let commit = self.resolve(&owner, &repo, asked.as_deref());
            let reference = commit.or(asked).unwrap_or_else(|| DEFAULT_REF.to_string());
            let data = self.fetch_file(&owner, &repo, &reference, &path).ok()?;

            manifest::version_in(&String::from_utf8_lossy(&data))
        });

        in_header.or(written)
    }

    /// Download a library's manifest and the files it lists into a new folder, which the caller owns and removes.
    ///
    /// The result mirrors the repository's own layout, so manifest::load() and install_to() work on it exactly as they
    /// do on a local clone.
    ///
    /// `carried` is the bytes of a manifest for a repository that has none, which the library needing it carries, used
    /// in place of the one the repository would hold.
    pub fn fetch(
        &mut self,
        source: &str,
        reference: Option<&str>,
        carried: Option<&[u8]>,
    ) -> Result<PathBuf, DownloadError> {
        let (owner, repo, named_ref) = split_ref(source)?;
        let asked = one_ref(source, named_ref, reference)?;
        let root = new_staging().map_err(|error| {
            DownloadError::new(format!("Could not make a temporary folder: {error}"))
        })?;

        // Half a library is of no use, and nobody else knows this folder exists, since it was never returned.
        if let Err(error) = self.fill(&owner, &repo, asked.as_deref(), &root, carried) {
            cleanup(&root);
            return Err(error);
        }

        Ok(root)
    }

    /// Download the manifest, then every file it lists, into root.
    fn fill(
        &mut self,
        owner: &str,
        repo: &str,
        asked: Option<&str>,
        root: &Path,
        carried: Option<&[u8]>,
    ) -> Result<(), DownloadError> {
        let commit = self.resolve(owner, repo, asked);

        // Every file then comes from the one commit the ref was when this began, or, when nobody could say which that
        // was, from the ref itself.
        let reference = commit
            .clone()
            .or_else(|| asked.map(str::to_string))
            .unwrap_or_else(|| DEFAULT_REF.to_string());

        let (raw, name) = match carried {
            // A file written for lib-installer is carried under its old name until a new one replaces it.
            Some(carried) => (carried.to_vec(), carried_name(carried)),
            None => self.manifest_of(owner, repo, &reference, asked)?,
        };

        write_under(root, &name, &raw)?;

        let mut origin = Map::new();
        origin.insert("kind".to_string(), json!("github"));
        origin.insert("source".to_string(), json!(format!("{owner}/{repo}")));
        origin.insert("ref".to_string(), json!(asked));
        origin.insert("commit".to_string(), json!(commit));
        origin.insert(
            "manifest".to_string(),
            if carried.is_some() {
                json!("carried")
            } else {
                Value::Null
            },
        );
        self.origins.insert(paths::resolve(root), origin);

        let unreadable = |why: String| {
            DownloadError::new(format!(
                "{owner}/{repo} has a {name} that cannot be read: {why}"
            ))
        };
        let text = String::from_utf8(raw.clone()).map_err(|error| unreadable(error.to_string()))?;
        let (text, _) =
            new_form(&name, &text).map_err(|error| DownloadError::new(error.message))?;
        let node = manifest::parse(&text).map_err(|error| unreadable(error.message))?;
        let data = node.as_map().expect("parse() gives a mapping");

        // Before a single listed file is fetched. A manifest for a newer libNIMA may list its files in a way this one
        // would get wrong.
        manifest::check_version(data).map_err(|error| DownloadError::new(error.message))?;

        let entries = listed_entries(data);

        // The extras the manifest names, or the two every library may carry when it names none. All are optional: a
        // repository may lack any of them.
        let named: Vec<String> = data
            .given("extras")
            .and_then(Node::as_list)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| match item.as_map() {
                Some(map) => map
                    .given("from")
                    .or_else(|| map.given("to"))
                    .or_else(|| map.given("root"))
                    .map(Node::text),
                None => Some(item.text()),
            })
            .collect();
        let default_extras = ["LICENSE.md".to_string(), "NOTICE".to_string()];
        let optional = |entry: &str| {
            if named.is_empty() {
                default_extras.iter().any(|item| item == entry)
            } else {
                named.iter().any(|item| item == entry)
            }
        };

        // Before a single one is fetched: each is written under root by its own path, so one that climbs out with ..
        // would land outside it.
        for entry in &entries {
            if let Some(reason) = unsafe_path(entry) {
                return Err(DownloadError::new(format!(
                    "{owner}/{repo} lists {} in its {name}, a file outside the library, which is never fetched: {reason}. \
                     Nothing was changed.",
                    crate::yaml::quoted(entry)
                )));
            }
        }

        // One listing of the repository, and only when something actually needs it: a wildcard, now, or a plain path
        // that turns out to be a folder, below.
        let mut known = if entries.iter().any(|entry| is_pattern(entry)) {
            self.tree(owner, repo, &reference)
        } else {
            None
        };
        let mut listed = known.is_some();

        let mut wanted: Vec<(String, String)> = Vec::new();

        for entry in &entries {
            match &known {
                None => wanted.push((entry.clone(), entry.clone())),
                Some(known) => {
                    for path in expand_known(entry, known) {
                        wanted.push((path, entry.clone()));
                    }
                }
            }
        }

        for (path, entry) in &wanted {
            let clean = paths::clean_posix(path);

            match self.fetch_file(owner, repo, &reference, &clean) {
                Ok(data) => write_under(root, &clean, &data)?,
                Err(error) => {
                    // LICENSE.md or NOTICE, which most libraries do not have. No folder, and not worth a call to the
                    // API, which allows 60 an hour without a token.
                    if named.is_empty() && optional(entry) {
                        continue;
                    }

                    // A plain path that is not a file may be a folder, which only the repository listing can resolve.
                    // An extra may be one too, such as doc, so it is looked for there before it is passed over. The
                    // listing is asked for once, however many entries need it.
                    if !listed {
                        known = self.tree(owner, repo, &reference);
                        listed = true;
                    }

                    let found = known
                        .as_ref()
                        .map(|known| expand_known(entry, known))
                        .unwrap_or_default();

                    if found.is_empty() {
                        // Optional by nature, so a repository missing one is fine.
                        if optional(entry) {
                            continue;
                        }

                        return Err(error);
                    }

                    for extra in found {
                        let data = self.fetch_file(owner, repo, &reference, &extra)?;
                        write_under(root, &extra, &data)?;
                    }
                }
            }
        }

        Ok(())
    }
}

/// What a test of the manifest wants to know: every path it lists.
#[cfg(test)]
pub(crate) fn tests_listed(data: &Mapping) -> Vec<String> {
    listed_entries(data)
}

/// One ref, whichever way it was given: refused when given twice and different.
fn one_ref(
    source: &str,
    named: Option<String>,
    reference: Option<&str>,
) -> Result<Option<String>, DownloadError> {
    if let (Some(named), Some(reference)) = (&named, reference)
        && named != reference
    {
        return Err(DownloadError::new(format!(
            "{source} names {named}, and --ref gives {reference}. Give the ref once."
        )));
    }

    Ok(named.or_else(|| reference.map(str::to_string)))
}

/// What a carried manifest is written as: as its text says, libnima.yml unless it asks for lib-installer.
fn carried_name(carried: &[u8]) -> String {
    let asks_old = String::from_utf8_lossy(carried).contains("lib_installer");

    (if asks_old {
        INSTALLER_NAME
    } else {
        MANIFEST_NAME
    })
    .to_string()
}

fn write_under(root: &Path, relative: &str, data: &[u8]) -> Result<(), DownloadError> {
    let path = root.join(relative);
    let failed = |error: std::io::Error| {
        DownloadError::new(format!("Could not write {}: {error}", path.display()))
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }

    fs::write(&path, data).map_err(failed)
}

/// A new temporary folder the caller removes with cleanup().
fn new_staging() -> std::io::Result<PathBuf> {
    Ok(tempfile::Builder::new()
        .prefix("libnima-")
        .tempdir()?
        .keep())
}

/// The folder holding a library's file: the one given, or the only one inside it.
///
/// GitHub's zip holds a single top folder, example-main, and Windows' Extract All puts that inside another folder of the
/// same name. Either is what a user will point at, so both have to work. None when neither holds a manifest, or when
/// several subfolders do and picking one would be a guess.
pub fn find_root(folder: &Path) -> Option<PathBuf> {
    if manifest::manifest_path(folder).is_some() {
        return Some(folder.to_path_buf());
    }

    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && manifest::manifest_path(path).is_some())
        .collect();

    found.sort();

    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// The bytes of the manifest in a library zip and its name, read without unpacking it, or None when it holds none: at its
/// top, or in its one top folder, as GitHub's zip has it, the way find_root() finds it once unpacked.
pub fn peek_zip(archive: &Path) -> Option<(Vec<u8>, String)> {
    let file = fs::File::open(archive).ok()?;
    let mut bundle = zip::ZipArchive::new(file).ok()?;
    let names: Vec<String> = bundle.file_names().map(str::to_string).collect();

    let mut tops: Vec<&str> = names
        .iter()
        .filter_map(|name| name.split_once('/').map(|(top, _)| top))
        .collect();
    tops.sort_unstable();
    tops.dedup();

    let holds = |prefix: &str| {
        MANIFEST_NAMES.iter().any(|name| {
            names
                .iter()
                .any(|found| *found == format!("{prefix}{name}"))
        })
    };
    let mut prefixes: Vec<String> = vec![String::new()];
    prefixes.extend(tops.iter().map(|top| format!("{top}/")));

    let found: Vec<&String> = prefixes.iter().filter(|prefix| holds(prefix)).collect();
    let prefix = match found.as_slice() {
        [] => return None,
        [first, ..] if first.is_empty() => String::new(),
        [only] => (*only).clone(),
        _ => return None,
    };

    for name in MANIFEST_NAMES {
        if let Ok(mut entry) = bundle.by_name(&format!("{prefix}{name}")) {
            let mut data = Vec::new();
            entry.read_to_end(&mut data).ok()?;

            return Some((data, name.to_string()));
        }
    }

    None
}

/// Unpack a library zip into a temporary folder.
///
/// Returns the folder to hand to cleanup() when done, and the library inside it. An error when the file is not a zip or
/// holds no library, which leaves nothing behind either way.
pub fn unpack(archive: &Path) -> Result<(PathBuf, PathBuf), DownloadError> {
    let name = archive
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let staging = new_staging().map_err(|error| {
        DownloadError::new(format!("Could not make a temporary folder: {error}"))
    })?;

    let unpacked = (|| -> Result<(), String> {
        let file = fs::File::open(archive).map_err(|error| error.to_string())?;
        let mut bundle = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;

        for index in 0..bundle.len() {
            let mut entry = bundle.by_index(index).map_err(|error| error.to_string())?;

            // A name that would lead out of the folder is passed by, never followed.
            let Some(relative) = entry.enclosed_name() else {
                continue;
            };

            let path = staging.join(relative);

            if entry.is_dir() {
                fs::create_dir_all(&path).map_err(|error| error.to_string())?;
                continue;
            }

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }

            let mut out = fs::File::create(&path).map_err(|error| error.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|error| error.to_string())?;
        }

        Ok(())
    })();

    if let Err(error) = unpacked {
        cleanup(&staging);
        return Err(DownloadError::new(format!(
            "{name} is not a usable zip: {error}"
        )));
    }

    match find_root(&staging) {
        Some(root) => Ok((staging, root)),
        None => {
            cleanup(&staging);
            Err(DownloadError::new(format!(
                "{name} holds no {MANIFEST_NAME}, so there is no library in it to install."
            )))
        }
    }
}

/// Remove a folder created by fetch() or unpack(). Never fails.
pub fn cleanup(path: &Path) {
    let _ = crate::files::remove_tree(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn split_of(text: &str) -> (String, String, Option<String>) {
        split_ref(text).unwrap()
    }

    #[test]
    fn a_source_is_read_every_way_it_is_written() {
        let plain = ("nimaltd".to_string(), "ee24".to_string(), None);
        let owned = ("someone".to_string(), "lib".to_string(), None);
        let tagged = (
            "someone".to_string(),
            "lib".to_string(),
            Some("v1.2.0".to_string()),
        );

        assert_eq!(split_of("ee24"), plain);
        assert_eq!(split_of("someone/lib"), owned);
        assert_eq!(split_of("https://github.com/someone/lib"), owned);
        assert_eq!(split_of("https://github.com/someone/lib/"), owned);
        assert_eq!(split_of("https://github.com/someone/lib.git"), owned);
        assert_eq!(split_of("someone/lib@v1.2.0"), tagged);
        assert_eq!(
            split_of("https://github.com/someone/lib/tree/v1.2.0"),
            tagged
        );
        assert_eq!(
            split_of("someone/lib@feature/x").2.as_deref(),
            Some("feature/x")
        );
    }

    #[test]
    fn a_ref_that_is_no_ref_is_refused() {
        for text in ["a/b@", "a/b@../x", "a/b@-x"] {
            assert!(split_ref(text).is_err(), "{text}");
        }

        assert!(split_ref("").is_err());
        assert!(split_ref("https://github.com/x").is_err());
    }

    #[test]
    fn a_ref_given_twice_and_different_is_refused() {
        assert!(one_ref("a/b@x", Some("x".to_string()), Some("y")).is_err());
        assert_eq!(
            one_ref("a/b@x", Some("x".to_string()), Some("x"))
                .unwrap()
                .as_deref(),
            Some("x")
        );
        assert_eq!(
            one_ref("a/b", None, Some("y")).unwrap().as_deref(),
            Some("y")
        );
    }

    #[test]
    fn a_key_comes_before_the_variables_and_github_token_before_gh_token() {
        let both = |name: &str| match name {
            "GITHUB_TOKEN" => Some("first".to_string()),
            "GH_TOKEN" => Some("second".to_string()),
            _ => None,
        };
        let only_gh = |name: &str| (name == "GH_TOKEN").then(|| "second".to_string());
        let found = |place: &str, value: &str| Some((place.to_string(), value.to_string()));

        assert_eq!(token_of(Some("key"), both), found("--key", "key"));
        assert_eq!(token_of(None, both), found("GITHUB_TOKEN", "first"));
        assert_eq!(token_of(None, only_gh), found("GH_TOKEN", "second"));
        assert_eq!(token_of(Some("   "), only_gh), found("GH_TOKEN", "second"));
        assert_eq!(token_of(Some("   "), |_| None), None);
        assert_eq!(token_of(None, |_| Some("  ".to_string())), None);
    }

    #[test]
    fn a_token_only_stops_retries_where_it_decides() {
        let status = |code| Failure::Status {
            code,
            permissions: String::new(),
        };

        assert!(passing(&Failure::Connection(String::new()), true));
        assert!(passing(&status(503), true));
        assert!(!passing(&status(404), false));
        assert!(passing(&status(403), false));
        assert!(passing(&status(429), false));
        assert!(!passing(&status(403), true));
        assert!(!passing(&status(401), true));
    }

    #[test]
    fn every_path_a_manifest_refers_to_is_listed() {
        let node = crate::yaml::loads(
            "name: x\nheaders: [a.h, 'inc/*.h']\nsources:\n  - from: b.c\n    to: c.c\n  - to: d.c\n\
             once: [e.txt]\nextras:\n  - root: tools\n  - f.md\n\
             requires:\n  libraries:\n    - source: a/b\n      manifest: m/b.yml\n\
             options:\n  o:\n    sources: [g.c]\n    libraries:\n      - source: c/d\n        manifest: m/d.yml\n",
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            listed_entries(node.as_map().unwrap()),
            [
                "a.h", "inc/*.h", "b.c", "d.c", "g.c", "e.txt", "tools", "f.md", "m/b.yml",
                "m/d.yml"
            ]
        );
    }

    #[test]
    fn the_licence_and_the_notice_are_asked_for_when_it_names_no_extras() {
        let node = crate::yaml::loads("name: x\nheaders: [a.h]\n")
            .unwrap()
            .unwrap();

        assert_eq!(
            listed_entries(node.as_map().unwrap()),
            ["a.h", "LICENSE.md", "NOTICE"]
        );
    }

    #[test]
    fn the_proxy_in_windows_settings_is_read() {
        assert_eq!(
            proxy_from_setting("127.0.0.1:10809").as_deref(),
            Some("http://127.0.0.1:10809")
        );
        assert_eq!(
            proxy_from_setting("http=a:1;https=b:2").as_deref(),
            Some("http://b:2")
        );
        assert_eq!(proxy_from_setting(""), None);
    }

    fn zip_with(path: &Path, files: &[(&str, &str)]) {
        let mut bundle = zip::ZipWriter::new(fs::File::create(path).unwrap());

        for (name, text) in files {
            bundle
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            bundle.write_all(text.as_bytes()).unwrap();
        }

        bundle.finish().unwrap();
    }

    #[test]
    fn a_zip_is_read_and_unpacked_with_its_one_top_folder() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("x-main.zip");
        zip_with(
            &archive,
            &[("x-main/libnima.yml", "name: x\n"), ("x-main/a.h", "h")],
        );

        let (data, name) = peek_zip(&archive).unwrap();
        assert_eq!(data, b"name: x\n");
        assert_eq!(name, "libnima.yml");

        let (staging, library) = unpack(&archive).unwrap();
        assert!(library.join("a.h").is_file());
        assert!(library.ends_with("x-main"));
        cleanup(&staging);
        assert!(!staging.exists());
    }

    #[test]
    fn a_zip_holding_no_library_is_said_to_and_leaves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("x.zip");
        zip_with(&archive, &[("a.h", "h")]);

        assert_eq!(peek_zip(&archive), None);
        assert!(
            unpack(&archive)
                .unwrap_err()
                .message
                .contains("holds no libnima.yml")
        );
    }

    #[test]
    fn something_that_is_no_zip_is_said_to_be() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("x.zip");
        fs::write(&archive, "not a zip").unwrap();

        assert_eq!(peek_zip(&archive), None);
        assert!(
            unpack(&archive)
                .unwrap_err()
                .message
                .contains("not a usable zip")
        );
    }

    #[test]
    fn a_name_in_a_zip_that_climbs_out_never_lands_outside() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("x.zip");
        zip_with(
            &archive,
            &[("libnima.yml", "name: x\n"), ("../../evil.txt", "e")],
        );

        let (staging, _) = unpack(&archive).unwrap();

        assert!(!staging.parent().unwrap().join("evil.txt").exists());
        assert!(!root.path().join("evil.txt").exists());
        cleanup(&staging);
    }

    #[test]
    fn the_root_is_the_folder_given_or_the_only_one_inside() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("x-main");
        fs::create_dir_all(&inner).unwrap();

        assert_eq!(find_root(root.path()), None);

        fs::write(inner.join("libnima.yml"), "").unwrap();
        assert_eq!(find_root(root.path()), Some(inner.clone()));
        assert_eq!(find_root(&inner), Some(inner.clone()));

        let other = root.path().join("y-main");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("installer.yml"), "").unwrap();
        assert_eq!(find_root(root.path()), None);
    }
}
