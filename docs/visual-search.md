# Visual search

Visual search finds records by what their pictures show, rather than by what their
metadata says. Search for `a wooden bridge` and qrate ranks the collection's images by how
well each one matches that description, even when no cell holds any of those words.

It also answers the other direction: pick a record and ask for the items that look like
it.

Everything runs on your own computer. No image and no search text leaves it.

## What it can search

Visual search reads the same thumbnails the gallery shows, so it covers every file type
qrate can preview:

- Photographs and other images.
- PDFs and documents, by their first page.
- Video, by its poster frame.
- Audio, by its cover art, if it has any.

A row with no linked file cannot be found this way. See
[Files and photos](files-and-photos.md) for how rows link to files.

## Installing the model

The first time you turn visual search on, qrate offers a **Download model** button in the
search bar. The download is about 605 MB and happens once per computer, not once per
project. qrate checks the download against a published checksum before installing it.

The model is stored beside qrate's other data, never inside a project, and it is shared by
every project you open.

## Searching

1. Open search with **Ctrl+F**.
2. Turn on the picture toggle in the search bar.
3. Type what you are looking for, in plain words: `a group of people outside a building`.

While a visual search is running:

- **The view shows only the results**, best match first. This applies to both the grid and
  the gallery, and the search stays as you switch between them.
- **The slider** beside the result count widens or narrows the results. Drag it left for
  only the closest matches and right to include weaker ones. Results are scored relative to
  the best match, so the slider means the same thing for any search.
- **The arrows** step through the results in order. If a file is open fullscreen, stepping
  changes the file it shows.
- **Replace is switched off**, along with match case, whole word and regular expressions.
  None of them mean anything for a picture.

Clear the search box or close the search bar to bring every row back.

### Writing a good query

- **Describe the picture, not the record.** `a horse-drawn cart on a dirt road` works;
  `accession 1994.12` does not.
- **Plain descriptions beat keywords.** `two people shaking hands` beats `handshake photo
  formal`.
- **Expect near misses.** The model ranks every image, so weaker matches follow the good
  ones. Tighten the slider rather than reading further down.
- **It reads the picture, not the text in it.** Searching for words written on a document
  is a job for the linked-file text search, below.

## Find similar items

Right-click a row, or a gallery tile, and choose **Find similar items**. qrate ranks every
other file by how much it looks like that one, and narrows the view to the closest.

This is how you find the rest of a photo shoot, the other copies of a form, or the pages
that came from the same album.

## Indexing

Before a file can be found, qrate reads its thumbnail once and stores what the model made
of it. This is the indexing step, and the search bar shows its progress.

- **It takes about a tenth of a second per file**, so a few thousand files take a few
  minutes the first time.
- **The results are stored in the project file**, so the next time you open the project
  there is nothing to redo.
- **A file is read again only if it changes.** Copying a project and its files to another
  computer keeps the index.
- **It runs in the background.** You can keep working, though the app may feel slower while
  it works through a large collection for the first time.

If you change the files folder or add new files, only the new files are read.

## Searching inside linked files

The book toggle in the search bar, beside the picture one, searches the *text inside*
linked PDFs rather than their appearance. It is off by default. Turn it on and an ordinary
search also matches words in a linked document, narrowing the view to the rows whose files
contain them.

- The text of each document is read once and kept, so later searches are immediate.
- A hit lands on the cell that names the file.
- Replace does not change text inside a file. It only rewrites cells.
- A scan with no text layer has nothing to search. qrate does not yet read text from
  scanned images.

## Limits

- **It is a ranking, not a verdict.** The model has no idea what your collection is about,
  and it can be confidently wrong. Treat a result as a suggestion to check.
- **It reads one page or frame per file.** Page 40 of a PDF is not searched.
- **It does not read handwriting or printed text.**
- **Terms are English.** The model was trained on English descriptions, so queries in other
  languages match poorly.
- **Nothing is written to your data.** Visual search only finds and orders rows. It never
  fills a cell in.
