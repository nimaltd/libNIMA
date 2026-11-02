// Whether a newer libNIMA is out, from its releases on GitHub. It has no
// imports, so its tests run in Node as they are.

/** The newest release, not a draft or a prerelease, from GitHub's API. */
const LATEST = "https://api.github.com/repos/nimaltd/libNIMA/releases/latest";

/** A release: its version, such as 1.0.1, and its page, with the downloads and what changed. */
export type Release = { version: string; url: string };

/** What looking for a newer version found. */
export type UpdateCheck =
  | { kind: "newer"; release: Release }
  | { kind: "newest" }
  | { kind: "failed"; problem: string };

/** The newest release on GitHub. Rejects with why when there is none, or GitHub cannot be reached. */
export async function latestRelease(): Promise<Release> {
  let response: Response;
  try {
    response = await fetch(LATEST, { headers: { Accept: "application/vnd.github+json" } });
  } catch {
    throw new Error("GitHub could not be reached. Is this computer online?");
  }
  if (response.status === 404) {
    throw new Error("libNIMA has no release on GitHub yet.");
  }
  if (!response.ok) {
    throw new Error(`GitHub answered ${response.status}. Try again later.`);
  }
  const release = (await response.json()) as { tag_name: string; html_url: string };
  return { version: release.tag_name.replace(/^v/, ""), url: release.html_url };
}

/** Looks for a release newer than `current`, and says what it found. */
export async function checkForUpdate(current: string): Promise<UpdateCheck> {
  try {
    const release = await latestRelease();
    return isNewer(release.version, current) ? { kind: "newer", release } : { kind: "newest" };
  } catch (error) {
    return { kind: "failed", problem: error instanceof Error ? error.message : String(error) };
  }
}

/** Whether `version` is newer than `current`, both as 1.2.3: a v in front, and a -beta after, are left out. */
export function isNewer(version: string, current: string): boolean {
  const numbers = (text: string) =>
    text
      .replace(/^v/, "")
      .split("-")[0]
      .split(".")
      .map((part) => Number.parseInt(part, 10) || 0);
  const these = numbers(version);
  const those = numbers(current);
  for (let index = 0; index < Math.max(these.length, those.length); index++) {
    const difference = (these[index] ?? 0) - (those[index] ?? 0);
    if (difference !== 0) {
      return difference > 0;
    }
  }
  return false;
}
