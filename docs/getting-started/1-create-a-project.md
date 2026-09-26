# Step 1: Create a project

[Getting started](index.md) ▸ Step 1 of 5

A project is one `.qrate` file. The **New Project** wizard makes it in a few steps. The steps at
the top of the wizard show where you are: **Name**, **Files**, **Link**, **Columns**, and
**Create**. A blank project has no **Link** step.

To open the wizard, click a **Create New** card in the launcher. **File ▸ New Project…** and
**Ctrl+N** open the wizard for a blank project. To start from a spreadsheet while a project is
open, press **Ctrl+O** to show the launcher.

Pick the way to start that matches what you have:

| You have | Choose | Read |
|---|---|---|
| A spreadsheet and a folder of files | **Spreadsheet + folder** | [From a spreadsheet](#from-a-spreadsheet) |
| A Google Sheet | **Google Sheet** | [From a Google Sheet](#from-a-google-sheet) |
| Only a folder of files | **Blank** | [From a folder of files](#from-a-folder-of-files) |
| Nothing yet | **Blank** | [From nothing](#from-nothing) |

## Name the project

Every way to start begins with this step.

1. In the launcher, click one of the **Create New** cards.
2. Type a name in **Project name**.
3. Optional: to save the project in a different folder, click **Browse…** next to **Save
   project to**.
4. Click **Next →**.

qrate suggests the folder where you last made a project. The first time, it suggests a `qrate`
folder in your Documents folder.

## From a spreadsheet

1. On the **Files** step, click **Browse…** next to **Spreadsheet (CSV, Excel, ODS)**.
2. Select your spreadsheet.
3. Click **Browse…** next to **Files folder**.
4. Select the folder that holds the files.
5. Read the message under the folder. It tells you how many files matched.
6. If the wizard shows **Files named by more than one row**, choose what to do with those files.
7. Click **Next →**.
8. On the **Link** step, keep **Match by exact filename**. See [Step 2](2-link-files.md).
9. Click **Next →**.
10. On the **Columns** step, keep **Auto-created from this spreadsheet**.
11. Under **Required columns**, set the **Title column** and the **File column**.
12. Click **Next →**.
13. Examine the summary, then click **Create Project**.

The **Title column** gives each row its name. The **File column** names each row's file. They
must be different columns.

## From a Google Sheet

The sheet must be shared as "Anyone with the link" with Viewer access. qrate reads the first
tab once. After that, the project does not change when the sheet changes.

1. Copy the sheet's link from your browser.
2. On the **Files** step, paste the link in **Sheet link**.
3. Click **Check**.
4. Choose a files folder, or select **I'll add a files folder later**.
5. Click **Next →**.
6. If the wizard shows the **Link** step, keep **Match by exact filename** and click **Next →**.
7. On the **Columns** step, set the **Title column** and the **File column**.
8. Click **Next →**, then click **Create Project**.

## From a folder of files

qrate makes one row for each file. Each folder becomes a parent row, and the files in it
become its child rows. The grid keeps the folder tree as an archival hierarchy.

1. In the launcher, click **Blank**.
2. On the **Files** step, click **Browse…** next to **Files folder**.
3. Select the folder.
4. Read the message under the folder. It tells you how many files become rows.
5. Under **Archival description standard**, choose **RAD**, **DACS**, **ISAD(G)**, **Records in
   Contexts**, or **Custom**.
6. Optional: change **Folders are called** and **Files are called**, for example to Series and
   Item.
7. Click **Next →**.
8. On the **Columns** step, keep **Use default Title and File columns**.
9. Click **Next →**.
10. Examine the **Hierarchy preview**, then click **Create Project**.

Leave **Create a row for the selected files folder** off when the folder holds only this
collection. Turn it on to get one top row for the folder itself.

You can also drag a folder onto the launcher to start this way.

## From nothing

1. In the launcher, click **Blank**.
2. On the **Files** step, select **I'll add a files folder later**.
3. Click **Next →**.
4. On the **Columns** step, keep **Use default Title and File columns**.
5. Click **Next →**, then click **Create Project**.

The project starts with a **Title** column and a **File** column. You add rows and columns in
the grid. To add files later, choose **File ▸ Import Files or Folders…**.

## Use the columns of an earlier project

On the **Columns** step, click **Load from a file or Sheet link…**. Select a column
configuration file, such as the `column_config.csv` from **Settings ▸ Columns ▸ Config file ▸
Export…**. The new project gets the same column types and descriptions.

## The project window

When you click **Create Project**, the project opens in the project window.

- **Title bar.** It shows the project name. A dot means that some changes are not saved yet.
- **Menu bar.** **File**, **Edit**, **Insert**, **View**, **Data**, **Plugins**, and **Help**.
  On macOS, these menus are in the system menu bar.
- **Center.** The grid of rows. The **Table** and **Gallery** tabs above it switch between the
  grid and a view of thumbnails. **Ctrl+1** and **Ctrl+2** do the same.
- **Docks.** Panels open at the left, right, and bottom of the center. In the table view, the
  **Details** panel starts in the left dock and the **Problems** panel starts in the bottom
  dock. The **History** and **Agent** panels start in the right dock.
- **Status bar.** It has one button for each panel. Click a button to show or hide its panel.
  The **Problems** button shows the number of errors and warnings.

**Ctrl+B**, **Ctrl+Alt+B**, and **``Ctrl+` ``** show or hide the left, right, and bottom docks.

## Learn more

[Projects](../projects.md) has every way to create, open, and add to a project, and the
project settings.

Next: [Step 2: Link rows to files](2-link-files.md) · Back to [Getting started](index.md)
