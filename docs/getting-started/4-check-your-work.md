# Step 4: Check your work with Problems

[Getting started](index.md) ▸ Step 4 of 5

qrate checks the project while you work. You do not start the checks. When you change a cell,
qrate checks it again.

## What qrate checks

- **Spelling and capitalization** in the columns that you choose, in the languages that you
  choose in **Settings ▸ Spelling**.
- **Dates** in the columns of the **Date** type. A malformed or ambiguous date is a finding.
- **File links.** A row whose file qrate cannot find is a finding.
- **Headings** against LCSH, Wikidata, or GeoNames, for the columns that you connect to one of
  them.
- **Value variants**, in the columns where you turn this check on. qrate shows values that are
  almost the same, such as two spellings of one name.
- **Plugin checks**, if you install plugins that add them.

qrate shows each finding in two places. A marker on the cell shows that the cell has a finding.
The Problems panel lists all the findings in the project.

## Open the Problems panel

Press **Ctrl+Shift+M**, choose **View ▸ Toggle Problems Panel**, or click the **Problems**
button in the status bar. The button shows the number of errors and warnings.

The tabs at the top of the panel are **All**, **Errors**, **Warnings**, and **Notes**. The
**Notes** tab shows the notes that people added to cells. The other tabs show findings.

qrate puts findings of the same kind in one group, with a count. Expand a group to see each
place where it occurs. Click a place to select that cell in the grid.

## Apply a fix

Some findings have a suggested correction.

1. Right-click the cell that has the marker.
2. Point to **Problems**. The item shows the number of findings, for example **Problems (2)**.
3. Point to the finding.
4. Click the suggestion.

The suggestion replaces the full value of the cell. **Ctrl+Z** undoes it. You can also
right-click a finding in the Problems panel to see the same suggestions.

To correct all the occurrences of a spelling finding at once, right-click its group in the
Problems panel and pick the correction.

## Accept a word or ignore a finding

A finding is not always an error. A name or a local term can be correct and still be
unknown to the dictionary. The menu of each finding has these choices:

- **Add “word” to dictionary** accepts the word in every project. Only a spelling finding has
  this choice.
- **Ignore this occurrence** hides the finding in this one cell.
- **Ignore in column**, followed by the column name, hides the same finding everywhere in that
  column.

qrate keeps what you ignore in the project. To see ignored findings again, turn on **Show
ignored** in the Problems panel.

## Learn more

- [Diagnostics](../diagnostics.md): every check, the Problems panel, fixes, and ignores
- [Columns](../columns.md): date formats, spell check per column, value variants, and
  authority lists
- [The Agent panel](../agent-panel.md): let a local AI agent add findings for you to review

Next: [Step 5: Save and export](5-save-and-export.md) · Back to [Getting started](index.md)
