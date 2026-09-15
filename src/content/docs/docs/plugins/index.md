---
title: 'Plugins'
description: 'find, install, and manage plugins'
sidebar:
  order: 7
---

A plugin adds checks and commands to qrate. Install a plugin from the official catalog, review an
unlisted public GitHub release, or add a local plugin folder.

To write a plugin, see [Develop plugins](/docs/plugins/developing).

## Install a plugin

Open **Plugins ▸ Discover Plugins…** to browse qrate's signed official catalog. qrate checks the
catalog signature and package hash before it installs a plugin. An official listing means that
qrate maintainers reviewed the published metadata and exact package bytes. It does not mean that
third-party code is safe.

Use **Plugins ▸ Install Plugin from Link…** for an unlisted public GitHub repository or release.
qrate downloads the release ZIP, checks its static package manifest, and shows its source,
permissions, size, and SHA-256 before it asks for confirmation.

New package installs are enabled. Optional permissions, such as network access, stay off until you
grant them under **Settings ▸ Plugins**. The same page shows whether qrate manages the package and
can safely remove it.

For an offline installation, open **Plugins ▸ Plugins Folder** and copy a plugin folder there.
Manual plugins are unmanaged. qrate does not update or remove them.

## The official plugin registry

The [plugin catalog](https://qrate.dvnl.work/plugins/) shows releases from the official plugin
registry. qrate reads the signed catalog when you open **Discover Plugins**. The registry stores
the release metadata, package hash, and signature. It does not host a plugin's source code.

An official listing means that qrate maintainers reviewed the listed release metadata and package
hash. It does not make third-party code safe. Review the source and requested permissions before
you install a plugin.

To add your own plugin to the registry, read [Develop plugins](/docs/plugins/developing#publish-a-plugin).

## What a plugin can do

- **Check a column.** A plugin reports bad column values in the Problems panel and grid markers.
- **Add right-click entries.** A plugin adds commands to a cell, row, or column header menu.
- **Add a bar item.** A plugin adds text and commands to the status bar or title bar.
- **Suggest values.** A plugin offers completions while you edit a cell.
- **Map columns onto a list.** A plugin can offer a list picker in Settings and column menus.
- **Declare settings.** A plugin declares settings. qrate renders and stores their values.

A plugin can use the network only after you grant that permission. The runtime has no `io`, `os`,
or `package` library.

## See also

- [Develop plugins](/docs/plugins/developing) — create, test, and publish a plugin.
- [API reference](/docs/plugins/api-reference) — hooks, host functions, and declarations.
- [Islandora plugin](/docs/plugins/islandora) — an official catalog plugin and a larger worked example.

