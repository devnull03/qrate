export const REPO = 'devnull03/qrate';

// This fetch runs at BUILD time (in CI), not in the browser.
// In Actions, GH_TOKEN is the job's GITHUB_TOKEN -> 1000 req/hr, no public limit,
// and nothing secret ends up in the shipped HTML. Locally the header is omitted,
// so the anonymous 60 req/hr applies (fine for occasional dev builds).
// The module is evaluated once per build, so all pages share this one request.
let cached;

export async function getReleases() {
  if (cached) return cached;

  const token = import.meta.env.GH_TOKEN;
  const res = await fetch(
    `https://api.github.com/repos/${REPO}/releases?per_page=20`,
    {
      headers: {
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
    },
  );

  if (!res.ok && import.meta.env.PROD) {
    // Fail the CI build loudly rather than shipping a silently-empty page.
    throw new Error(`GitHub API ${res.status} ${res.statusText} fetching releases`);
  }

  cached = res.ok ? await res.json() : [];
  return cached;
}

// The API returns newest first. The newest release wins even when it is a
// pre-release — the alphas are the current build, so a stale stable must not
// drive the hero button or the download table.
export function latest(releases) {
  return releases[0] ?? null;
}

// Friendly labels keyed off the asset filenames produced by release.yml.
export function labelFor(name) {
  if (/-setup\.exe$/i.test(name)) return 'Windows installer';
  if (/-x86_64\.zip$/i.test(name)) return 'Windows (portable)';
  if (/\.dmg$/i.test(name)) return 'macOS (Intel + Apple Silicon)';
  if (/SHA256SUMS\.txt$/i.test(name)) return 'Checksums';
  return name;
}

// Only the shipped installers get a platform. Anything else in the release
// (sample data, checksums, source archives) must stay out of the download table.
export function platformOf(name) {
  if (/\.dmg$/i.test(name)) return 'macOS';
  if (/(-setup\.exe|\.msi|(x64|x86_64)[^/]*\.zip)$/i.test(name)) return 'Windows';
  // release.yml ships Linux as a tarball, not a .deb or .AppImage. Matching only
  // the latter two dropped the Linux build from the download table and the hero
  // button entirely, and made the changelog count two platforms out of three.
  if (/(\.AppImage|\.deb|linux[^/]*\.tar\.(gz|xz))$/i.test(name)) return 'Linux';
  return null;
}

export function humanSize(bytes) {
  if (bytes == null) return '';
  const units = ['B', 'KB', 'MB', 'GB'];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${i === 0 ? n : n.toFixed(1)} ${units[i]}`;
}

export function formatDate(iso) {
  return new Date(iso).toLocaleDateString('en-GB', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });
}
