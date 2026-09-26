# Microsoft Store release setup

GitHub Releases remain qrate's public release channel. On stable tags, the Windows build can also
produce a private `windows-store-msix` workflow artifact. Once Store submission is enabled, a later
job creates and commits a Partner Center submission. The unsigned MSIX is not attached to the
GitHub release; Microsoft signs it after certification.

## One-time Partner Center setup

1. Reserve qrate's name and create the app in Partner Center. Fill in the age ratings, listing and
   privacy policy URL. The submission API cannot create the first app submission; use Partner
   Center's UI for the first package submission after the workflow builds its MSIX artifact.
2. Deploy the privacy policy update in the qrate-site checkout, then use
   <https://qrate.dvnl.work/privacy>. The current policy now describes the optional Pi/OpenRouter
   data flow and app update/catalog requests.
3. Copy the exact Store identity values from Partner Center: `Identity/Name` and
   `Identity/Publisher`. These values are immutable after the first submission.
4. Associate an Entra application with Partner Center and give it the Manager role.

Set these repository variables:

| Variable | Value |
| --- | --- |
| `QRATE_STORE_APPLICATION_ID` | The app's Store ID from Partner Center |
| `QRATE_STORE_IDENTITY_NAME` | Exact `Identity/Name` from Partner Center |
| `QRATE_STORE_IDENTITY_PUBLISHER` | Exact `Identity/Publisher` from Partner Center |
| `QRATE_STORE_PUBLISHER_DISPLAY_NAME` | The publisher name shown to customers |
| `QRATE_STORE_SUBMISSIONS_ENABLED` | `true` only after the first submission is complete |

Set these secrets in the `release-signing` GitHub environment:

| Secret | Value |
| --- | --- |
| `QRATE_STORE_TENANT_ID` | Entra tenant ID |
| `QRATE_STORE_CLIENT_ID` | Entra application client ID |
| `QRATE_STORE_CLIENT_SECRET` | Entra application secret |

With identity variables set and submissions disabled, a stable release builds the MSIX artifact so
the first package can be submitted by hand in Partner Center. After that submission has completed
with age ratings, set
`QRATE_STORE_SUBMISSIONS_ENABLED=true`; later stable tags submit automatically. Prerelease tags do
not build or submit an MSIX. The submit job keeps Partner Center's existing publish mode and listing
metadata, and stops after Partner Center accepts the submission for processing. Certification and
any manual publication step remain visible in Partner Center.

## Package behavior

The Store package declares `windows-store` and `base` in `qrate-install.json`. It contains only the
app, the marker and manifest images. PDFium, ffmpeg and the agent runtime remain optional components
installed after the user chooses them, using the same component manifest as the other installers.
The full-trust manifest allows qrate's native PDF loader, child processes and agent runtime to work;
the Store install directory itself remains read-only, so qrate's updater is disabled for this kind.
The package manifest targets Windows 10 version 2004 (build 19041) or later for its `uap10`
full-trust declaration.

The Store reserves the fourth MSIX version field. It also requires a nonzero first version field,
while qrate is still `0.x`. The package script offsets qrate's major version by one (for example,
qrate `0.6.0` becomes Store package `1.6.0.0`), preserving version order. The app's own version in
the install marker remains the Cargo version.

The Store submission job uses Microsoft's MSIX app submission API. It needs the Store ID plus the
Entra tenant, client ID and secret; no Seller ID is used by this API. Store package signing is
provided after certification, so the build does not need a code-signing certificate.
