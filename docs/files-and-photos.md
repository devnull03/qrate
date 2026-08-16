# Files and photos

A row can link to a file on disk: a photo, document, audio, or video file. qrate never
copies these files into the project. It stores the files folder's path and finds each
row's file by matching it against that folder every time you open the project.

## Linking a row to a file

Set the files folder in **Settings ▸ Project**. qrate then matches each row to a file by
one of two rules:

- **Exact filename.** A column value that matches a file's name, such as an identifier
  column, links that row to that file.
- **Your own pattern.** Configure a column as the `Filename` type to control which column
  and which matching rule qrate uses. See [Columns](columns.md).

If a linked file cannot be found, for example because the files folder moved or the file
was renamed, qrate reports it as a diagnostic. See [Diagnostics](diagnostics.md).

## Viewing a file

The Details panel, in the right dock, shows the file linked to the selected row. It
previews images, documents, audio, and video directly. Click the preview to open it
fullscreen, where you can zoom, pan, page through a multi-page document, and search inside
it.

PDF previews need PDFium and video frame previews need ffmpeg. qrate looks for both beside
its own executable, then on your system `PATH`. Without them, qrate shows a file-type icon
instead of a preview, and the rest of the app works as normal.

## Gallery view

Switch to the gallery from the view menu to browse a collection as thumbnails instead of a
grid. See [The grid](grid.md#gallery-view).
