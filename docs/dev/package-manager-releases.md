# Package manager releases

GitHub Releases remain the source for signed update metadata and downloadable assets. Once a
release is published, the package-manager workflow updates qrate's Homebrew Cask and opens a WinGet
manifest update pull request. It runs on stable releases only; beta tags do not change either
package manager.

## Homebrew

The Cask lives in [`devnull03/homebrew-tap`](https://github.com/devnull03/homebrew-tap), at
`Casks/qrate.rb`. It installs the base universal app from the release DMG. qrate installs optional
components when the user requests them. The Cask declares `auto_updates true` because qrate also
has an in-app updater. The app is not notarized; the Cask shows a first-launch Gatekeeper note.

Add the `HOMEBREW_TAP_TOKEN` repository secret. The token needs Contents read/write access to
`devnull03/homebrew-tap`. The job fails until the secret exists. To publish a release that was
published before the secret existed, run **Update package managers** by hand with its tag. For each published stable release, the workflow reads the DMG checksum
from `SHA256SUMS.txt`, updates the Cask version and checksum, and pushes a commit to the tap's
`main` branch. It does not push a release that is still a draft.

The tap entry pointed at qrate 0.6.0-beta.1 so the Cask could be used before the first stable
release. The update workflow replaces it with each stable version.

## WinGet

The WinGet listing uses the per-machine MSI, which is the Windows package-manager build and updates
through WinGet. The community repository must accept the first manifest before the release workflow
can update it.

For the first stable release, create and submit the initial manifest on Windows with WinGetCreate:

```powershell
wingetcreate new https://github.com/devnull03/qrate/releases/download/vX.Y.Z/qrate-X.Y.Z-x86_64.msi
```

Use package identifier `devnull03.qrate`, publisher `devnull03`, and package name `qrate`; submit
the manifest to the [WinGet community repository](https://github.com/microsoft/winget-pkgs). Wait
for that PR to merge before enabling automation. Microsoft documents that new manifests are
submitted through a pull request and then validated by the repository.

After the first manifest is accepted, set the `WINGET_PACKAGE_ID` repository variable to
`devnull03.qrate` and add the `WINGET_CREATE_GITHUB_TOKEN` repository secret. The token must be a
GitHub personal access token (classic) with the `repo` scope, as required by WinGetCreate's
submission flow. On each published stable release, the workflow downloads the official
WinGetCreate CLI, reads the version and public MSI URL from the release tag, and asks WinGetCreate
to calculate the MSI metadata and open an update PR. The PR still goes through Microsoft's normal
validation and review.

The first WinGet manifest is an external prerequisite, so the update job is skipped while
`WINGET_PACKAGE_ID` is unset. The GitHub release and the Homebrew update do not depend on WinGet.
