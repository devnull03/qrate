# Files and photos

A row can link to a file on disk: a photo, document, audio, or video file. qrate does not
move your files. It stores the files folder's path and finds each row's file by matching it
against that folder when you open the project, and again whenever a file in the folder
changes (see [When the files folder changes](#when-the-files-folder-changes)). The only time
qrate copies a file is when you add one from outside the files folder and choose to copy it
in (see below).

## Linking a row to a file

Set the files folder in **Settings ▸ Project**. qrate then matches each row to a file
through the column set to the `Filename` type (see [Columns](columns.md)), by one of two
rules chosen when the project is created:

- **Exact filename.** A `Filename` cell that matches a file's name links that row to that
  file.
- **Custom pattern.** A pattern such as `{id}_*.jpg` matches files that do not share an
  exact name with the cell.

To link one row by hand, drop a file onto its `Filename` cell. To add new files as rows, see
[Add files to an open project](projects.md#add-files-to-an-open-project). A file inside the
files folder is linked by its path relative to that folder.

## Files from outside the files folder

When you drop a file or folder that is not inside the files folder, onto the grid or onto a
`Filename` cell, qrate asks what to do with it:

- **Copy into the project.** qrate copies it into the `imported` folder inside the files
  folder and links the row to the copy. qrate never overwrites a file: if the name is taken,
  the copy is named `photo (2).jpg`, `photo (3).jpg`, and so on. Large files copy in the
  background, and the grid updates when the copy is done.
- **Link where it is.** The row stores the file's full path. The **Problems** panel shows a
  warning for each of these rows ("outside the project's files folder"), because the file
  is not with the rest of the collection and does not move with it.
- **Cancel.** Nothing is added.

To stop the question, set **Settings ▸ Project ▸ Import ▸ Files from outside the files
folder** to **Copy into the files folder** or **Link where it is**. The default is **Ask each
time**. A project with no files folder set does not ask; the dropped file is linked where it
is.

A ZIP export includes the files that are linked from outside the files folder. They are in
`files/outside/` in the archive, next to the files from the files folder.

## When the files folder changes

While a project is open, qrate watches its files folder, including every subfolder. You do
not have to reopen the project after you add, rename, replace, or delete a file there:

- **A file that a row names appears.** The row links to it, its preview shows, and its
  missing-file problem goes away.
- **A linked file is deleted or renamed.** The row's missing-file problem comes back.
- **A linked file is replaced.** The preview shows the new version.

qrate waits until a file has stopped changing for about half a second before it uses it, so
a large file that is still copying is not read half-written. It ignores hidden files, Office
lock files (`~$…`), `Thumbs.db`, `desktop.ini`, and unfinished downloads or copies
(`.tmp`, `.part`, `.crdownload`).

A project with no files folder set, or one whose files folder does not exist, is not
watched.

### New files

A file in the files folder that no row links to is a new file. The status bar shows
**New files (N)** on the left while there are any. qrate also looks for new files when you
open a project, so files added while qrate was closed are counted too.

Click **New files (N)** to import them as rows. The prompt is the same one you see when you
drop files on the grid, with the project's duplicate policy as the default button (see
[Add files to an open project](projects.md#add-files-to-an-open-project)).

- **Ignore** takes these files out of the count. The project remembers them, so they do not
  come back when you open it again.
- **Cancel** imports nothing and leaves the files in the count.

Files that qrate copies into `imported` for you are never counted as new.

## Missing files

If a linked file cannot be found, for example because it was renamed or deleted, qrate
reports it as a diagnostic. See [Diagnostics](diagnostics.md).

### When the whole files folder moved

When you open a project whose files folder does not exist, or holds almost none of the files
the rows link to, qrate shows one banner above the grid ("Files folder not found" or "Most
linked files are missing from"). It does not add a problem for every row. Click **Relink…**
in the banner, or choose **File ▸ Relink Missing Files…**, and pick the folder's new
location. Click the close button to hide the banner for this session.

Before it changes anything, qrate counts the linked files in the folder you picked and shows
"N of M linked files found in" that folder:

- **Relink** uses that folder.
- **Choose another…** lets you pick a different folder.
- **Cancel** keeps the current folder.

If the folder you picked holds none of the files, qrate also looks in its parent folder and
in the parent's other subfolders, and offers the one that holds the most ("Found N in …
instead").

### One row's file

When the selected row's file is missing, the Details panel shows "File not found" with a
**Locate file…** button. Pick the file, and qrate links the row to it. If the same folder
also holds the files of other missing rows, qrate asks whether to relink those rows too.
All of these changes are one step, so one **Undo** reverses them.

## Viewing a file

The Details panel, in the right dock, shows the file linked to the selected row. It
previews images, documents, audio, and video directly. Click the preview to open it
fullscreen, where you can zoom, pan, page through a multi-page document, and search inside
it.

The arrows in the bottom-right corner of the fullscreen view move to the file of the
previous or next row, in the order the view shows them. During a search that means the
previous or next result.

PDF previews need PDFium and video frame previews need ffmpeg. qrate looks for both beside
its own executable, then on your system `PATH`. Every release download includes PDFium, and
the Windows downloads also include ffmpeg; on macOS and Linux, install ffmpeg yourself.
Without them, qrate shows a file-type icon instead of a preview, and the rest of the app
works as normal.

qrate keeps downscaled copies of your files so they open faster the second time. The cache
rebuilds itself as you browse. **Settings ▸ Table ▸ Previews ▸ Cache size** sets how large it
may grow: 512 MB, 1 GB, 2 GB (the default), or 5 GB. Past that, the oldest thumbnails are
dropped. To reclaim the space now, choose **Clear cache** in the same group; qrate then reports
how many thumbnails it removed and how much space it freed.

## Gallery view

Switch to the gallery with **View ▸ Switch View ▸ Gallery** to browse a collection as
thumbnails instead of a grid. See [The grid](grid.md#gallery-view).
