# Plugins

A plugin adds checks and commands to qrate. Install a plugin from the official catalog, review an
unlisted public GitHub release, or add a local plugin folder.

To write a plugin, see [Develop plugins](developing.md).

## Install a plugin

Open **Plugins ▸ Discover Plugins…** to browse qrate's official catalog in your web browser.
Select **Open in qrate** on a plugin's page, and qrate opens a review screen for that release.
qrate checks the catalog signature and package hash before it installs a plugin. An official
listing means that qrate maintainers reviewed the published metadata and exact package bytes.
It does not mean that third-party code is safe.

For an unlisted public GitHub repository or release, open **Settings ▸ Plugins**, select
**Install from GitHub…**, and paste the link. qrate downloads the release ZIP, checks its static
package manifest, and shows its source, permissions, size, and SHA-256 before you select
**Install unlisted plugin**.

New package installs are enabled. Optional permissions, such as network access, stay off until you
grant them under **Settings ▸ Plugins**, which **Plugins ▸ Manage Plugins…** also opens. The same
page shows whether qrate manages the package and can safely remove it.

For an offline installation, open **Plugins ▸ Plugins Folder** and copy a plugin folder there.
Manual plugins are unmanaged. qrate does not update or remove them.

## The official plugin registry

The [plugin catalog](https://qrate.dvnl.work/plugins/) shows releases from the official plugin
registry. qrate reads the signed catalog when it opens a plugin's review screen. The registry
stores the release metadata, package hash, and signature. It does not host a plugin's source code.

An official listing means that qrate maintainers reviewed the listed release metadata and package
hash. It does not make third-party code safe. Review the source and requested permissions before
you install a plugin.

To add your own plugin to the registry, read [Develop plugins](developing.md#publish-a-plugin).

## What a plugin can do

- **Check a column.** A plugin reports bad column values in the Problems panel and grid markers.
- **Add right-click entries.** A plugin adds commands to a cell, row, or column header menu.
- **Add a bar item.** A plugin adds text and commands to the status bar or title bar.
- **Suggest values.** A plugin offers completions while you edit a cell.
- **Add an export format.** A plugin adds an entry to **File ▸ Export** that writes JSON.
- **Map columns onto a list.** A plugin can offer a list picker in Settings and column menus.
- **Declare settings.** A plugin declares settings. qrate renders and stores their values.

A plugin can use the network only after you grant that permission. The runtime has no `io`, `os`,
or `package` library.

## See also

- [Develop plugins](developing.md): create, test, and publish a plugin.
- [API reference](api-reference.md): hooks, host functions, and declarations.
- [Islandora plugin](islandora.md): an official catalog plugin and a larger worked example.
