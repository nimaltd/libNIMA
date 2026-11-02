//! Fetching a library from GitHub, a private repository included, and over a link that is not well.
//!
//! Nothing here goes near the internet. A server on this machine answers as GitHub does, and remembers what it was
//! asked, so a test can see where each request went and what it carried with it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use libnima_core::download::{Client, DownloadError, cleanup};

const MANIFEST: &str = "name: demo\nplatform: stm32\nheaders:\n  - demo.h\nsources:\n  - demo.c\n";
const SHA: &str = "3f2a9c10000000000000000000000000000000000";

/// A repository on disk that the server holds as someone/demo.
fn repository(root: &Path, manifest: &str, more: &[(&str, &str)]) -> PathBuf {
    let folder = root.join("repository");

    put(&folder, &[("demo.h", "/* h */\n"), ("demo.c", "/* c */\n")]);
    put(&folder, more);

    if !manifest.is_empty() {
        fs::write(folder.join("libnima.yml"), manifest).unwrap();
    }

    folder
}

fn serving(root: &Path, manifest: &str, more: &[(&str, &str)]) -> Github {
    let folder = repository(root, manifest, more);

    Github::serving(&[("someone/demo", &folder), ("someone/private", &folder)])
}

/// A download, removed when the test is done with it.
struct Got(PathBuf);

impl Drop for Got {
    fn drop(&mut self) {
        cleanup(&self.0);
    }
}

impl std::ops::Deref for Got {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

fn fetched(client: &mut Client, source: &str) -> Got {
    Got(client.fetch(source, None, None).unwrap())
}

fn refused(client: &mut Client, source: &str) -> DownloadError {
    client.fetch(source, None, None).unwrap_err()
}

// ----------------------------------------------------------------------------
// Where each request goes, and what it carries
// ----------------------------------------------------------------------------

#[test]
fn without_a_token_nothing_rides_along() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);

    let got = fetched(&mut server.client(), "someone/demo");
    let seen = server.seen();

    assert!(!seen.is_empty());
    assert!(
        seen.iter()
            .all(|one| one.url.starts_with("/raw/someone/demo/HEAD/")),
        "{seen:?}"
    );
    assert!(seen.iter().all(|one| one.header("Authorization").is_none()));
    assert_eq!(fs::read_to_string(got.join("demo.c")).unwrap(), "/* c */\n");
}

#[test]
fn with_a_token_every_file_comes_from_the_api_with_it() {
    // raw.githubusercontent cannot see a private repository. The API can, given the token.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);

    let got = fetched(
        &mut server.client_with("GITHUB_TOKEN", "t0ken"),
        "someone/private",
    );
    let seen = server.seen();

    assert!(
        seen.iter()
            .all(|one| one.url.starts_with("/repos/someone/private/contents/")),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .all(|one| one.header("Authorization") == Some("Bearer t0ken"))
    );
    assert!(
        seen.iter()
            .all(|one| one.header("Accept") == Some("application/vnd.github.raw"))
    );
    assert_eq!(fs::read_to_string(got.join("demo.h")).unwrap(), "/* h */\n");
}

#[test]
fn a_private_repository_without_a_token_says_what_to_do() {
    // GitHub answers 404 for a private repository, so "does not exist" alone would mislead.
    let root = tempfile::tempdir().unwrap();
    let server = Github::serving(&[("someone/private", root.path())]);

    let error = refused(&mut server.client(), "someone/private");

    assert!(error.message.contains("private"), "{}", error.message);
    assert!(
        error
            .message
            .contains("give a token that can read it with --key, or in GITHUB_TOKEN"),
        "{}",
        error.message
    );
}

#[test]
fn a_refused_token_is_named_but_never_shown() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server.holding().status = Some(401);

    let error = refused(
        &mut server.client_with("GITHUB_TOKEN", "t0ken-secret"),
        "someone/private",
    );

    assert!(error.message.contains("GITHUB_TOKEN"), "{}", error.message);
    assert!(!error.message.contains("t0ken-secret"), "{}", error.message);
}

#[test]
fn a_403_is_not_reported_as_a_missing_file() {
    // A token that may not read the repository, or a limit on requests, is not "does not exist".
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server.holding().status = Some(403);

    let error = refused(
        &mut server.client_with("GITHUB_TOKEN", "t0ken"),
        "someone/private",
    );

    assert!(error.message.contains("HTTP 403"), "{}", error.message);
    assert!(
        error.message.contains("token in GITHUB_TOKEN"),
        "{}",
        error.message
    );
    assert!(
        !error.message.contains("does not exist"),
        "{}",
        error.message
    );
    assert_eq!(
        server.seen().len(),
        1,
        "a refusal is reported as it is, for the first file"
    );
}

#[test]
fn a_redirect_to_another_host_leaves_the_token_behind() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = serving(root.path(), MANIFEST, &[]);
    let here = Github::serving(&[]);

    // The two listen on one address, which only the name tells apart.
    here.holding().redirect = Some(elsewhere.origin().replace("127.0.0.1", "localhost"));

    // Only the manifest: on Windows a name that also means ::1, where nobody listens, costs two seconds a request.
    here.client_with("GITHUB_TOKEN", "t0ken")
        .peek("someone/private", None)
        .unwrap();

    assert!(
        here.seen()
            .iter()
            .all(|one| one.header("Authorization") == Some("Bearer t0ken"))
    );
    assert!(
        !elsewhere.seen().is_empty(),
        "the redirect was not followed"
    );
    assert!(
        elsewhere
            .seen()
            .iter()
            .all(|one| one.header("Authorization").is_none())
    );
}

#[test]
fn a_redirect_on_the_same_host_keeps_the_token() {
    // What a renamed repository does.
    let root = tempfile::tempdir().unwrap();
    let renamed = serving(root.path(), MANIFEST, &[]);
    let here = Github::serving(&[]);

    here.holding().redirect = Some(renamed.origin().to_string());

    fetched(
        &mut here.client_with("GITHUB_TOKEN", "t0ken"),
        "someone/private",
    );

    assert!(
        renamed
            .seen()
            .iter()
            .all(|one| one.header("Authorization") == Some("Bearer t0ken"))
    );
}

#[test]
fn the_repository_listing_carries_the_token() {
    // A wildcard in a private repository's manifest needs the listing, and so the token.
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nheaders:\n  - '*.h'\n",
        &[],
    );

    fetched(
        &mut server.client_with("GITHUB_TOKEN", "t0ken"),
        "someone/private",
    );

    let listing = server
        .seen()
        .into_iter()
        .find(Seen::is_listing)
        .expect("nothing listed the repository");

    assert!(
        listing
            .url
            .starts_with("/repos/someone/private/git/trees/HEAD"),
        "{}",
        listing.url
    );
    assert_eq!(listing.header("Authorization"), Some("Bearer t0ken"));
}

#[test]
fn a_branch_with_a_slash_stays_one_ref() {
    // feature/x is one ref. Unescaped in a path it would be two parts of the path.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);

    Got(server
        .client_with("GITHUB_TOKEN", "t0ken")
        .fetch("someone/private", Some("feature/x"), None)
        .unwrap());

    assert!(
        server.seen()[0]
            .url
            .ends_with("/contents/libnima.yml?ref=feature%2Fx"),
        "{}",
        server.seen()[0].url
    );
}

// ----------------------------------------------------------------------------
// With no ref, the default branch, pinned to the commit it is
// ----------------------------------------------------------------------------

#[test]
fn no_ref_takes_the_default_branch_whatever_its_name() {
    // HEAD is git's name for it: main, master or develop, the one GitHub marks default.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    let mut client = server.client();

    let got = fetched(&mut client, "someone/demo");

    assert!(server.seen().iter().all(|one| one.reference() == "HEAD"));
    assert_eq!(
        serde_json::Value::Object(client.origin(&got)),
        serde_json::json!({"kind": "github", "source": "someone/demo", "ref": null, "commit": null, "manifest": null})
    );
}

#[test]
fn every_file_comes_from_the_commit_the_ref_was() {
    // A push half way through a download cannot mix two versions.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server
        .holding()
        .commits
        .insert("HEAD".to_string(), SHA[..40].to_string());
    let mut client = server.client();

    let got = fetched(&mut client, "someone/demo");

    assert!(
        server.seen().iter().all(|one| one.reference() == SHA[..40]),
        "{:?}",
        server.seen()
    );
    assert_eq!(client.origin(&got)["commit"], SHA[..40]);
    assert_eq!(
        server.holding().lookups[0].header("Accept"),
        Some("application/vnd.github.sha")
    );
}

#[test]
fn a_tag_is_pinned_the_same_way() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server
        .holding()
        .commits
        .insert("v1.2.0".to_string(), SHA[..40].to_string());
    let mut client = server.client();

    let got = fetched(&mut client, "someone/demo@v1.2.0");

    assert!(server.seen().iter().all(|one| one.reference() == SHA[..40]));
    assert_eq!(client.origin(&got)["ref"], "v1.2.0");
}

#[test]
fn a_ref_given_is_the_only_one_asked() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server.holding().branches = Some(vec!["main".to_string()]);

    assert!(
        server
            .client()
            .fetch("someone/demo", Some("master"), None)
            .is_err()
    );
    assert!(server.seen().iter().all(|one| one.reference() == "master"));
}

#[test]
fn a_missing_manifest_names_the_default_branch() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server.holding().branches = Some(vec!["develop".to_string()]);

    let error = refused(&mut server.client(), "someone/demo");

    assert!(
        error.message.contains("on its default branch"),
        "{}",
        error.message
    );
    assert!(error.message.contains("GITHUB_TOKEN"), "{}", error.message);
    assert!(error.not_found);
}

#[test]
fn a_missing_manifest_names_the_ref_given() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server.holding().branches = Some(vec!["develop".to_string()]);

    let error = server
        .client()
        .fetch("someone/demo", Some("v9"), None)
        .unwrap_err();

    assert!(
        error
            .message
            .contains("does not exist in someone/demo at v9"),
        "{}",
        error.message
    );
}

#[test]
fn two_refs_for_one_library_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);

    let error = server
        .client()
        .fetch("someone/demo@v1.0.0", Some("v2.0.0"), None)
        .unwrap_err();

    assert!(
        error.message.contains("Give the ref once"),
        "{}",
        error.message
    );
}

#[test]
fn a_ref_that_is_no_ref_is_refused_without_a_request() {
    for source in ["someone/demo@", "someone/demo@../x", "someone/demo@-rf"] {
        let root = tempfile::tempdir().unwrap();
        let server = serving(root.path(), MANIFEST, &[]);

        let error = refused(&mut server.client(), source);

        assert!(error.message.contains("after the @"), "{}", error.message);
        assert!(server.seen().is_empty() && server.holding().lookups.is_empty());
    }
}

#[test]
fn a_listed_file_outside_the_library_is_never_fetched() {
    // Each file is written by its own path, so this one would land outside the download.
    for entry in ["../evil.h", "..\\evil.h", "/etc/passwd", "C:/x.h"] {
        let root = tempfile::tempdir().unwrap();
        let text =
            format!("name: demo\nplatform: stm32\nheaders:\n  - demo.h\nsources:\n  - '{entry}'\n");
        let server = serving(root.path(), &text, &[]);

        let error = refused(&mut server.client(), "someone/demo");

        assert!(error.message.contains("never fetched"), "{}", error.message);
        assert!(
            server.seen().iter().all(|one| one.file() == "libnima.yml"),
            "{:?}",
            server.seen()
        );
    }
}

#[test]
fn a_manifest_for_a_newer_libnima_fetches_nothing_else() {
    let root = tempfile::tempdir().unwrap();
    let text = format!("{MANIFEST}requires:\n  libnima: 99.0.0\n");
    let server = serving(root.path(), &text, &[]);

    let error = refused(&mut server.client(), "someone/demo");

    assert!(error.message.contains("99.0.0"), "{}", error.message);
    assert!(
        server.seen().iter().all(|one| one.file() == "libnima.yml"),
        "{:?}",
        server.seen()
    );
}

#[test]
fn a_carried_manifest_stands_in_for_one_the_repository_lacks() {
    // littlefs as its authors ship it, with a libnima.yml spif carries for it.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), "", &[]);
    let mut client = server.client();

    let got = Got(client
        .fetch("someone/demo", None, Some(MANIFEST.as_bytes()))
        .unwrap());

    assert_eq!(
        fs::read_to_string(got.join("libnima.yml")).unwrap(),
        MANIFEST
    );
    assert_eq!(fs::read_to_string(got.join("demo.c")).unwrap(), "/* c */\n");
    assert_eq!(client.origin(&got)["manifest"], "carried");
    assert!(
        server.seen().iter().all(|one| one.file() != "libnima.yml"),
        "asked for a manifest it carries"
    );
}

#[test]
fn a_repository_with_only_an_installer_yml_is_read() {
    // A library not yet moved over, as most are on the day libNIMA ships.
    let root = tempfile::tempdir().unwrap();
    let old = "lib_installer: 4.0.0\nname: demo\nplatform: stm32\nheaders:\n  - demo.h\n";
    let server = serving(root.path(), "", &[("installer.yml", old)]);

    let (bytes, name) = server.client().peek("someone/demo", None).unwrap();

    assert_eq!(name, "installer.yml");
    assert_eq!(String::from_utf8(bytes).unwrap(), old);

    let files: Vec<String> = server.seen().iter().map(Seen::file).collect();

    assert_eq!(
        files,
        ["libnima.yml", "installer.yml"],
        "libnima.yml is asked for first"
    );
}

#[test]
fn what_was_read_first_is_not_asked_for_again() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    let mut client = server.client();

    client.peek("someone/demo", None).unwrap();
    fetched(&mut client, "someone/demo");

    assert_eq!(
        server
            .seen()
            .iter()
            .filter(|one| one.file() == "libnima.yml")
            .count(),
        1
    );
    assert_eq!(
        server.holding().lookups.len(),
        1,
        "asked twice which commit the branch is"
    );
}

// ----------------------------------------------------------------------------
// A token that cannot read
// ----------------------------------------------------------------------------

#[test]
fn a_token_that_can_see_but_not_read_says_how_to_fix_it() {
    // A fine-grained token with the repository chosen and only Metadata, the usual mistake.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    {
        let mut holding = server.holding();
        holding.status = Some(403);
        holding.permissions = Some("contents=read".to_string());
    }

    let error = refused(&mut server.client_with("--key", "t0ken"), "someone/private");

    assert!(
        error.message.starts_with(
            "The token given with --key can see someone/private, but cannot read its files"
        ),
        "{}",
        error.message
    );
    assert!(
        error
            .message
            .contains("https://github.com/settings/personal-access-tokens")
    );
    assert!(error.message.contains("set Contents to Read-only"));
    assert!(!error.message.contains("t0ken"));
}

#[test]
fn a_token_that_cannot_see_the_repository_says_where_to_add_it() {
    let server = Github::serving(&[]);

    let error = refused(&mut server.client_with("--key", "t0ken"), "someone/hidden");

    assert!(
        error.message.contains(
            "the token given with --key cannot see it: add it under the token's Repository access"
        ),
        "{}",
        error.message
    );
}

#[test]
fn an_answer_that_is_no_commit_is_not_used() {
    // A proxy's error page, say, where a SHA should be: the ref itself is used.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);
    server
        .holding()
        .commits
        .insert("HEAD".to_string(), "<html>not a commit</html>".to_string());
    let mut client = server.client();

    let got = fetched(&mut client, "someone/demo");

    assert!(server.seen().iter().all(|one| one.reference() == "HEAD"));
    assert_eq!(client.origin(&got)["commit"], serde_json::Value::Null);
}

// ----------------------------------------------------------------------------
// A folder under extras, with no wildcard to have the repository listed first
// ----------------------------------------------------------------------------

fn listings(server: &Github) -> usize {
    server.seen().iter().filter(|one| one.is_listing()).count()
}

#[test]
fn a_folder_under_extras_is_fetched_file_by_file() {
    // raw.githubusercontent serves files, not folders, so only the listing finds what doc and dts hold. Their being
    // optional must not skip them unlooked.
    let root = tempfile::tempdir().unwrap();
    let text = format!("{MANIFEST}extras:\n  - LICENSE.md\n  - doc\n  - dts\n");
    let server = serving(
        root.path(),
        &text,
        &[
            ("LICENSE.md", "licence\n"),
            ("doc/guide.md", "# guide\n"),
            ("dts/demo.overlay", "/ { };\n"),
        ],
    );

    let got = fetched(&mut server.client(), "someone/demo");

    assert_eq!(
        fs::read_to_string(got.join("doc/guide.md")).unwrap(),
        "# guide\n"
    );
    assert_eq!(
        fs::read_to_string(got.join("dts/demo.overlay")).unwrap(),
        "/ { };\n"
    );
    assert_eq!(
        listings(&server),
        1,
        "the listing is asked for once, for both folders"
    );
}

#[test]
fn an_extra_the_repository_lacks_is_passed_over() {
    // Named in extras but in neither the files nor the listing: optional, so no error.
    let root = tempfile::tempdir().unwrap();
    let text = format!("{MANIFEST}extras:\n  - NOTICE\n");
    let server = serving(root.path(), &text, &[]);

    let got = fetched(&mut server.client(), "someone/demo");

    assert!(!got.join("NOTICE").exists());
    assert!(got.join("demo.c").exists());
}

#[test]
fn the_extras_every_library_may_carry_cost_no_listing() {
    // With no extras named, a missing LICENSE.md or NOTICE is passed over at once: most libraries lack NOTICE, and the
    // API allows 60 calls an hour without a token.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), MANIFEST, &[]);

    fetched(&mut server.client(), "someone/demo");

    assert_eq!(listings(&server), 0);
}

#[test]
fn a_listed_file_the_repository_lacks_still_stops_the_fetch() {
    // Only extras are optional. A source that is neither a file nor a folder is an error.
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nsources:\n  - demo.c\n  - gone.c\n",
        &[],
    );

    let error = refused(&mut server.client(), "someone/demo");

    assert!(error.not_found);
    assert!(
        error
            .message
            .contains("gone.c does not exist in someone/demo"),
        "{}",
        error.message
    );
}

#[test]
fn a_listing_cut_off_by_github_counts_as_none() {
    // Literal paths still work. Patterns would quietly miss files, so it is better to know nothing.
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nheaders:\n  - '*.h'\nsources:\n  - demo.c\n",
        &[],
    );
    server.holding().truncated = true;

    let error = refused(&mut server.client(), "someone/demo");

    assert!(error.message.contains("*.h"), "{}", error.message);
}

#[test]
fn every_option_is_fetched_chosen_or_not() {
    // Which are taken is asked once the library is on disk, and the answer must not need a second download.
    let root = tempfile::tempdir().unwrap();
    let text = format!(
        "{MANIFEST}options:\n  lfs:\n    description: the file system\n    headers: [lfs/demo_lfs.h]\n"
    );
    let server = serving(root.path(), &text, &[("lfs/demo_lfs.h", "")]);

    let got = fetched(&mut server.client(), "someone/demo");

    assert!(got.join("lfs/demo_lfs.h").is_file());
}

// ----------------------------------------------------------------------------
// A link that is not well
// ----------------------------------------------------------------------------
//
// From a user's machine, installing spif with littlefs: GitHub closed the connection while osal and littlefs were
// fetched. Each file is tried five times, taking turns between the two hosts, and what still fails is said in words,
// with what to do about it.

fn faulty(root: &Path, faults: &[Fault]) -> Github {
    let server = serving(root, MANIFEST, &[]);
    server.holding().faults = faults.to_vec();

    server
}

#[test]
fn a_dropped_connection_is_tried_again_on_the_other_host() {
    let root = tempfile::tempdir().unwrap();
    let server = faulty(root.path(), &[Fault::Dropped]);

    let got = fetched(&mut server.client(), "someone/demo");
    let seen = server.seen();

    assert_eq!(fs::read_to_string(got.join("demo.c")).unwrap(), "/* c */\n");
    assert!(
        seen[0].is_raw() && seen[0].file() == "libnima.yml",
        "{:?}",
        seen[0]
    );
    assert!(
        seen[1].is_api() && seen[1].file() == "libnima.yml",
        "{:?}",
        seen[1]
    );
    assert_eq!(seen[1].header("Accept"), Some("application/vnd.github.raw"));
    assert_eq!(
        seen[0]
            .header("Accept")
            .filter(|accept| accept.contains("github")),
        None
    );
}

#[test]
fn what_a_bad_link_does_is_tried_again() {
    for fault in [
        Fault::Dropped,
        Fault::CutShort,
        Fault::Status(503),
        Fault::Status(500),
    ] {
        let root = tempfile::tempdir().unwrap();
        let server = faulty(root.path(), &[fault]);

        let got = fetched(&mut server.client(), "someone/demo");

        assert_eq!(
            fs::read_to_string(got.join("libnima.yml")).unwrap(),
            MANIFEST,
            "{fault:?}"
        );
    }
}

#[test]
fn the_api_limit_without_a_token_lets_the_other_host_answer() {
    let root = tempfile::tempdir().unwrap();
    let server = faulty(root.path(), &[Fault::Dropped, Fault::Status(403)]);

    fetched(&mut server.client(), "someone/demo");

    let seen = server.seen();

    assert!(
        seen[0].is_raw() && seen[1].is_api() && seen[2].is_raw(),
        "{seen:?}"
    );
}

#[test]
fn a_refusal_to_a_token_is_not_tried_again() {
    // The same answer every time, so it is reported at once, from the API only.
    let root = tempfile::tempdir().unwrap();
    let server = faulty(root.path(), &[Fault::Status(403)]);

    let error = refused(
        &mut server.client_with("GITHUB_TOKEN", "t0ken"),
        "someone/private",
    );
    let seen = server.seen();

    assert!(error.message.contains("HTTP 403"), "{}", error.message);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].is_api());
}

#[test]
fn a_link_that_keeps_breaking_is_reported_with_what_to_do() {
    let root = tempfile::tempdir().unwrap();
    let server = faulty(root.path(), &[Fault::Dropped; 5]);

    let error = refused(&mut server.client(), "someone/demo");

    assert_eq!(server.seen().len(), 5);
    assert!(
        error
            .message
            .contains("Could not fetch libnima.yml from someone/demo, after 5 tries"),
        "{}",
        error.message
    );
    assert!(error.message.contains("HTTPS_PROXY"), "{}", error.message);
    assert!(error.message.contains("Download ZIP"), "{}", error.message);
    assert!(!error.not_found);
}

#[test]
fn a_404_is_not_tried_again() {
    // GitHub's answer, the same every time, and the manifest's fallback runs on it.
    let root = tempfile::tempdir().unwrap();
    let server = serving(root.path(), "", &[]);

    assert!(refused(&mut server.client(), "someone/demo").not_found);

    let names: Vec<String> = server.seen().iter().map(Seen::file).collect();

    assert_eq!(names, ["libnima.yml", "installer.yml", "library.yml"]);
}

#[test]
fn a_listing_cut_short_counts_as_no_listing() {
    // Patterns then fall back to plain paths, as with no listing at all, and this one is no file.
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nheaders:\n  - '*.h'\n",
        &[],
    );

    // The manifest, the listing cut short, the pattern as a plain path, and the listing asked for again, for what may be
    // a folder.
    server.holding().faults = vec![Fault::Fine, Fault::CutShort, Fault::Fine, Fault::CutShort];

    let error = refused(&mut server.client(), "someone/demo");
    let seen = server.seen();

    assert!(seen[1].is_listing(), "{seen:?}");
    assert_eq!(seen[2].file(), "*.h", "{seen:?}");
    assert!(error.message.contains("*.h"), "{}", error.message);
}

#[test]
fn every_file_named_by_only_a_to_or_a_root_is_fetched() {
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nheaders:\n  - demo.h\nextras:\n  - root: tools\n  - to: doc\nonce:\n  - to: boards/chip.overlay\n  - VERSION\n",
        &[
            ("tools/a.py", ""),
            ("doc/b.md", ""),
            ("boards/chip.overlay", ""),
            ("VERSION", "1\n"),
        ],
    );

    let got = fetched(&mut server.client(), "someone/demo");

    for file in ["tools/a.py", "doc/b.md", "boards/chip.overlay", "VERSION"] {
        assert!(got.join(file).is_file(), "{file}");
    }
}

#[test]
fn the_commit_and_the_ref_reach_the_record() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let littlefs = root.path().join("littlefs");
    put(
        &littlefs,
        &[("lfs.h", "/* lfs */\n"), ("lfs.c", "/* lfs */\n")],
    );
    let carried = "name: littlefs\nplatform: stm32\nversion: 2.9.3\nheaders:\n  - lfs.h\nsources:\n  - lfs.c\n";
    let needer = library(
        root.path(),
        "spif",
        &format!(
            "{DEMO_CODE}requires:\n  libraries:\n    - source: littlefs-project/littlefs@v2.9.3\n      manifest: manifests/littlefs.yml\n"
        ),
        &[("manifests/littlefs.yml", carried)],
    );
    let server = Github::serving(&[("littlefs-project/littlefs", &littlefs)]);
    server
        .holding()
        .commits
        .insert("v2.9.3".to_string(), SHA[..40].to_string());
    let mut asked = request(&project, &[&needer]);
    asked.folder = Some("Libs/spif".to_string());

    let (code, ui) = run_with(server.client(), &asked);
    let written = record(&project);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(written["littlefs"]["origin"]["commit"], SHA[..40]);
    assert_eq!(written["littlefs"]["origin"]["ref"], "v2.9.3");
    assert_eq!(written["littlefs"]["origin"]["manifest"], "carried");
    assert_eq!(written["spif"]["origin"]["kind"], "folder");
    assert_eq!(
        written["spif"]["origin"]["path"],
        libnima_core::paths::posix(&dunce::canonicalize(&needer).unwrap())
    );
}

#[test]
fn a_run_says_it_in_words_and_installs_nothing() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  libraries: [osal]\n"),
        &[],
    );
    let osal = root.path().join("osal");
    put(&osal, &[("libnima.yml", MANIFEST)]);
    let server = Github::serving(&[("osal", &osal)]);
    server.holding().faults = vec![Fault::Dropped; 5];
    let mut asked = request(&project, &[&source]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run_with(server.client(), &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("Could not fetch libnima.yml from nimaltd/osal"),
        "{}",
        ui.err()
    );
    assert!(
        !project.join("demo").exists(),
        "installed with a library it needs missing"
    );
}

#[test]
fn half_a_library_leaves_nothing_behind() {
    // Nobody else knows the folder exists, since it was never returned.
    let root = tempfile::tempdir().unwrap();
    let server = serving(
        root.path(),
        "name: demo\nplatform: stm32\nsources:\n  - demo.c\n  - half-gone.c\n",
        &[],
    );
    let temporary = std::env::temp_dir();
    let before: Vec<_> = fs::read_dir(&temporary)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();

    assert!(server.client().fetch("someone/demo", None, None).is_err());

    // Other tests run beside this one and make folders of their own, so only what holds this library's files counts.
    for entry in fs::read_dir(&temporary).unwrap().flatten() {
        let path = entry.path();

        if !before.contains(&path) && path.join("libnima.yml").is_file() {
            let text = fs::read_to_string(path.join("libnima.yml")).unwrap_or_default();

            assert!(
                !text.contains("half-gone.c"),
                "left {} behind",
                path.display()
            );
        }
    }
}
