# Files and photos

A row can link to a file on disk: a photo, document, audio, or video file. qrate never
copies these files into the project. It stores the files folder's path and finds each
row's file by matching it against that folder every time you open the project.

## Linking a row to a file

Set the files folder in **Settings ▸ Project**. qrate then matches each row to a file
through the column set to the `Filename` type (see [Columns](columns.md)), by one of two
rules chosen when the project is created:

- **Exact filename.** A `Filename` cell that matches a file's name links that row to that
  file.
- **Custom pattern.** A pattern such as `{id}_*.jpg` matches files that do not share an
  exact name with the cell.

To link one row by hand, drop a file onto its `Filename` cell. To add new files as rows, see
[Add files to an open project](projects.md#add-files-to-an-open-project).

If a linked file cannot be found, for example because the files folder moved or the file
was renamed, qrate reports it as a diagnostic. See [Diagnostics](diagnostics.md). If the
whole folder moved, choose **File ▸ Relink Missing Files…** and pick its new location.

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
