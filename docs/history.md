# Change history

qrate records every change to a project's data and notes. The record stays in the `.qrate`
file, so it is still there after you close and open the project again. Undo only goes back
through the current session. The history goes back to the start of the project.

The **History** panel starts in the right dock. Click the **History** button in the status bar
to show or hide it.

## The list

The newest change is at the top. The panel groups changes by day, under **Today**,
**Yesterday**, or the date. Each change shows what it did and three details:

- **How** the change was made: **Typed**, **Pasted**, **Cleared**, **Replace**, **Details
  panel**, **Spelling fix**, **Fix:** and the name of the fix, **Rows and columns**,
  **Spreadsheet import**, **Undo**, **Redo**, or **Restored to #** and a change number.
- **Who** made it. qrate uses the name in **Application ▸ Identity ▸ Author name**. See
  [Projects](projects.md).
- **When** it was made.

Undo and Redo entries are dimmed. They are part of the record, but they only reverse other
changes.

Edits that you have not saved yet show at the top, under **Not saved yet**. They move into the
dated list when qrate saves the project.

The panel collapses a run of small edits into one row. The edits must come from the same
person, be made in the same way, and be less than five minutes apart. Only typed, pasted,
cleared, and Details panel edits collapse. Click the row to show or hide the edits in it.

The panel first loads the most recent 200 saved changes. Click **Load Older Changes** at the
bottom of the list to load 200 more.

## Filter the list

The button in the panel's title bar filters the list:

- **All changes**
- **Cell edits**
- **Rows and columns**
- **Notes**
- **Fixes**, which includes spelling fixes
- **Named versions**, which shows only the changes that you named

Changes to annotations appear as **Annotation added**, **moved**, **resized**, **edited**, and
**deleted**, with a crop of the part of the file they mark. A move or resize shows the crop
before and after.

## One cell's history

Right-click a cell in the grid and choose **Show edit history**. The History panel opens and
shows only the changes to that cell. This includes changes made while its column had a
different name. Click the close button beside **Changes to** to show every change again.

The Details panel also has a **History** section. It shows the 50 most recent changes to the
selected item. Click its header to expand or collapse it.

## Name a version

A name marks a point in the history that you want to find again, for example "Sent to the
registrar".

1. Right-click a saved change.
2. Choose **Name This Version…**.
3. Type the name and press **Enter**.

The name shows above the change. To remove it, right-click the change and choose **Remove
Name**.

## Restore

Right-click a saved change for these commands:

- **Restore Project to Here…** puts the whole project back to how it was after that change.
  qrate first asks you to confirm and says how many changes it will reverse. The restore is
  a new change in the history, so **Ctrl+Z** undoes it.
- **Restore This Value** puts one cell back to the value it had before the change. This command
  shows only for a change to one cell.
- **Restore This Annotation** puts an annotation back the way it was before the change: its
  place on the file, its words, and its kind. A deleted annotation comes back. For a note on a
  cell or row, the command is **Restore This Note**.

The **History** section of the Details panel has a restore button on each saved change to a
cell or an annotation.

None of these commands deletes history. The changes that a restore reverses stay in the list.

## Clear the history

Open the panel's menu and choose **Clear History…**. qrate asks you to confirm. After you
clear the history, you cannot restore the project to any point before that moment. Clearing
the history does not change your data.
