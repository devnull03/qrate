# Getting started

This section takes you from a new install to your first export. The walkthrough has five
pages, one for each stage of the work. Read them in order the first time. Each page ends with
a link to the next stage.

## What qrate is

qrate is a desktop app to describe a collection. You describe each item in a row of a
spreadsheet grid. qrate connects each row to the file that the row describes, such as a photo,
a scan, a document, or a recording. While you work, qrate checks the data and shows the
problems it finds.

The core idea has three parts:

- **Rows.** One row is one record. One column is one field, such as Title, Date, or Subject.
- **Linked files.** The files stay in a folder on your computer. A row names its file, and
  qrate finds the file in that folder. qrate does not move your files.
- **Checks.** qrate checks spelling, dates, file links, and headings. It shows each finding
  in the Problems panel and as a marker on the cell. You decide which corrections to apply.

When the catalog is ready, you export it to CSV, Excel, JSON-LD, CSL-JSON, a ZIP archive, or a
Google Sheet.

## What qrate does not do

- qrate does not copy your files into the project. The project file holds the metadata only.
  A ZIP export is the one place where qrate puts the files together with the data.
- qrate does not edit your photos or documents. It shows them next to their records.
- qrate does not need an account or a server. Your project is a file on your computer.
- qrate does not change a cell on its own. A check can suggest a correction, but you apply it.

## Words used in this guide

| Word | Meaning |
|---|---|
| Project | One `.qrate` file. It holds the grid, the column settings, the notes, and the ignored problems. |
| Files folder | The folder where the collection's files are. Each project has one. |
| Linked file | The file that a row names in its file column. |
| Finding | One problem that a check reports, for example a misspelled word in one cell. |
| Launcher | The window that opens when qrate starts. It lists your recent projects. |

On macOS, use Cmd where this guide says Ctrl, and Option where it says Alt.

## Before you begin

Answer these questions before you create your first project.

**Where are your files?** Put the collection's photos, scans, documents, and recordings in one
folder. Subfolders are fine. qrate reads the files where they are and does not move them.

**Do you already have a spreadsheet?** qrate can start from a CSV, TSV, Excel, or OpenDocument
spreadsheet. It can also start from a Google Sheet that anyone with the link can view. If you have no
spreadsheet, qrate can make one row for each file in your folder.

**Which column names your files?** If you start from a spreadsheet, one column must name each
row's file, for example `IMG_0042.jpg`. qrate uses this column to link rows to files.

## Install qrate

Start with the **base** download for your platform on the
[install page](https://qrate.dvnl.work/docs/install). It is the smaller package and offers to add
PDF previews and the assistant when you first use them. Windows and Linux can also add video
support on first use; macOS uses ffmpeg from Homebrew. Choose the full download
for a computer with limited connectivity; macOS video still needs a separate ffmpeg install.
Windows also has a portable ZIP and an MSI for managed deployments. Run the installer, or unpack
the portable build, then start qrate.

On macOS, you can also install the base app from the [qrate Homebrew tap](https://github.com/devnull03/homebrew-tap):

```sh
brew install --cask devnull03/tap/qrate
```

On Windows, you can also install the base app from the
[Microsoft Store](https://apps.microsoft.com/detail/9NL2FCHXTBMM). The Store keeps it up to date,
so qrate's own updater is switched off in that install.

WinGet is a planned Windows install option. It will be available after its first package
submission is accepted. Once the WinGet listing is live, install it with:

```powershell
winget install --id devnull03.qrate --exact
```

The Homebrew Cask installs the base app. Optional components can be installed from within qrate.
The tap may lag a prerelease; use the install page for the newest beta. The WinGet package uses
the per-machine MSI.

The builds do not have a Windows publisher signature or an Apple Developer ID signature and
notarization. Windows SmartScreen can ask you to confirm. On macOS, try to open qrate once, then
choose **Open Anyway** under System Settings → Privacy & Security.

The full downloads include PDF previews. Base downloads offer to install PDF support when you
need it. Windows and Linux can also install ffmpeg on first use, or carry it in the full download.
On macOS, install ffmpeg and put it on your `PATH` to see video previews. Without it, qrate shows
a file-type icon for a video, and everything else works.

## What you see first

qrate opens the launcher. The left side lists **Recent Projects**. The first time, the list is
empty. The right side, **Create New**, has three ways to start a project:

- **Blank**: start with no rows, or with a folder of files.
- **Spreadsheet + folder**: start from a spreadsheet and the folder of its files.
- **Google Sheet**: start from a shared spreadsheet link.

To open the launcher again later, choose **File ▸ Open Projects…** or press **Ctrl+O**.
**Help ▸ User Guide** opens this guide.

## The walkthrough

1. [Create a project](1-create-a-project.md): name the project, choose where its rows come
   from, and learn the project window.
2. [Link rows to files](2-link-files.md): set the files folder and see how rows find their
   files.
3. [Describe items in the grid](3-describe-items.md): edit cells, undo, find, filter, group
   rows, and use the Details panel and the viewer.
4. [Check your work with Problems](4-check-your-work.md): read the findings, apply a fix, or
   ignore a finding.
5. [Save and export](5-save-and-export.md): how saving works, and your first export.

The last page lists the reference pages to read next.
