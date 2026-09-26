# Step 2: Link rows to files

[Getting started](index.md) ▸ Step 2 of 5

A row describes a file, and qrate shows that file next to the row. This page explains how a
row finds its file, and what to do when it does not.

## How linking works

Each project has one files folder. qrate stores the path to this folder, not the files. When
you open the project, qrate looks in the folder for the file that each row names.

The row names its file in the column of the **Filename** type. The wizard gives this type to
the column that you chose as the **File column**. The link uses one of two rules, which you
choose on the **Link** step of the wizard:

- **Match by exact filename.** The cell holds the name of the file, for example `IMG_0042.jpg`.
  Select **Also search subfolders** when the files are in subfolders of the files folder.
- **Use a custom pattern.** A pattern such as `{id}_*.jpg` finds files that do not have the
  exact name in the cell.

When qrate makes rows from a folder, a file in a subfolder gets its path from the files folder,
for example `box-1/IMG_0042.jpg`.

While a project is open, qrate watches the files folder. When you add, rename, or delete a
file there, the grid shows the change without a restart.

## Set or change the files folder

The wizard sets the files folder. If you skipped it, or if you moved the folder, set it here.

1. Press **Ctrl+,** to open Settings.
2. Click **Project**.
3. Next to **Files folder**, click the folder button.
4. Select the folder.

qrate never moves the files. If you move the folder yourself, set its new location here.

## Link one row by hand

1. Find the row in the grid.
2. Drag the file from your file manager onto the row's cell in the file column.

If the file is not in the files folder, qrate asks what to do by default. **Copy into the project** copies
the file into the `imported` folder inside the files folder. **Link where it is** keeps the file
where it is and stores its full path.

## Add more files as rows

1. Choose **File ▸ Import Files or Folders…**.
2. Select the files or folders.
3. Read how many rows qrate will add, then confirm.

You can also drag files and folders onto the grid. When there are files in the files folder
that no row names, the status bar shows **New files (N)**. Click it to add those files as rows.

## When a file is missing

If a row names a file that qrate cannot find, the Problems panel shows a finding for that row.
The Details panel shows "File not found" and a **Locate file…** button. Click it and select the
file to link the row again.

If the whole files folder moved, qrate shows one banner above the grid. Click **Relink…** in the
banner, or choose **File ▸ Relink Missing Files…**, and select the folder's new location.

## Learn more

[Files and photos](../files-and-photos.md) has the full rules for linking, files from outside
the files folder, new files, and missing files. [Columns](../columns.md) explains the
**Filename** column type.

Next: [Step 3: Describe items in the grid](3-describe-items.md) · Back to [Getting started](index.md)
