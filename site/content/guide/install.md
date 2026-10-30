+++
title = "Install OxidGene"
description = "Download and start the desktop application on Linux, macOS or Windows, find where your data is kept, or self-host the web version."
weight = 10
+++

This page explains how to install the OxidGene desktop application, where it keeps your genealogy on your computer, and where to look if you would rather build it yourself or run the web version for your family.

The desktop application is a single program that contains everything it needs: your trees are stored in a database on your own computer, and it works offline. It only needs the network to open a cited register on an archive's website and to import a tree from Geneanet.

## Check your computer

- A 64-bit Linux, macOS (11 or later) or Windows system, with a graphical desktop.
- The system's web view component: WebKitGTK on Linux, WebView2 on Windows, WebKit on macOS.
- At least 4 GiB of memory and 500 MiB of free disk space, plus room for your trees and all the photos and documents you import.
- For large trees or large media collections, an SSD and 8 GiB of memory are recommended.

You do not need a database server, a container system or any other software.

## Download the application

OxidGene is published on its [GitHub Releases page](https://github.com/trois-six/oxidgene/releases). Download it only from there.

1. Open the [releases page](https://github.com/trois-six/oxidgene/releases) and pick the latest version.
2. Download the file for your system:

   | System | File | What it contains |
   |---|---|---|
   | Linux (x86-64) | `oxidgene-desktop-linux-x86_64.tar.gz` | The program |
   | macOS 11 or later (Apple silicon and Intel) | `oxidgene-desktop-macos-universal.dmg` | `OxidGene.app` |
   | Windows (x86-64) | `oxidgene-desktop-windows-x86_64.zip` | The program |

3. Also download `SHA256SUMS` and check that your file matches it before you run it. On Linux, for example, run `sha256sum -c SHA256SUMS --ignore-missing` in the download folder. If you use the GitHub CLI, `gh attestation verify <file> --repo trois-six/oxidgene` also confirms that the file was built by the project's own release workflow.

These are portable programs, not installers: nothing is added to your system besides the program itself and the folders described below.

If the releases page does not list any version yet, build the application from source instead (see below).

## Start it on Linux

1. Extract the archive, for example with `tar -xzf oxidgene-desktop-linux-x86_64.tar.gz`.
2. Move the `oxidgene-desktop` program wherever you keep your applications.
3. Run it, for example with `./oxidgene-desktop`.

## Start it on macOS

1. Open the `.dmg` file and drag `OxidGene.app` into your Applications folder.
2. Open OxidGene once. macOS refuses to open it, because the application is not yet signed with an Apple Developer ID. Close the warning.
3. Open **System Settings → Privacy & Security**, find the message about OxidGene and choose **Open Anyway**.
4. Confirm. macOS remembers your choice, and OxidGene opens normally from then on.

## Start it on Windows

1. Extract the `.zip` file and move `oxidgene-desktop.exe` wherever you like.
2. Double-click it.
3. The program is not signed yet (each release's notes say when it is), so Windows SmartScreen may warn about it. Choose **More info**, then **Run anyway**.

## Where your data is kept

OxidGene sorts the files it writes into four folders. On Linux they are:

| Folder | What it holds | Can you delete it? |
|---|---|---|
| `~/.local/share/oxidgene/` | Your database (`oxidgene.db`) and the `media/` folder with your photos and documents | **No.** This is your genealogy and it cannot be rebuilt |
| `~/.config/oxidgene/` | Your own themes (`themes/`) and languages (`languages/`) | No: you wrote them |
| `~/.local/state/oxidgene/` | The window's preferences (language, theme, pedigree defaults) and the cookies of the archive websites you visited | Only while OxidGene is closed; your preferences are reset |
| `~/.cache/oxidgene/` | Temporary files of imports and exports | Yes, while OxidGene is closed |

On macOS your database and media are in `~/Library/Application Support/oxidgene/`; on Windows, in `%APPDATA%\oxidgene\`. The appearance and language settings show the exact folder for your own themes and languages (see [Settings](@/guide/settings.md)).

Keep the data folder in your regular backups. For a portable copy of a single tree with its media, export it as a GEDZIP archive (see [Import and export](@/guide/import-export.md)).

## Build it from source

If you prefer to compile the application yourself, you need Git, the stable Rust toolchain and the development packages of your system's web view. Then:

```bash
git clone https://github.com/trois-six/oxidgene.git
cd oxidgene
just build-desktop-release
./target/release/oxidgene-desktop
```

Without `just`, run `cargo build --locked --release --package oxidgene-desktop`. On Windows the program is `target\release\oxidgene-desktop.exe`. The full list of required packages per system is in the technical quickstart.

## Run the web version for your family

OxidGene can also run as a web application, used from a browser, backed by PostgreSQL and S3-compatible storage. There are two ways to deploy it:

- **Docker Compose**, to try the web version on one machine.
- **Kubernetes with Helm**, for a server deployment.

The web version has no user accounts yet: anyone who can reach it can read, change and delete every tree. Keep it on a private network or behind an authenticating gateway. A few features are desktop-only, among them importing photos from Geneanet and connecting an AI assistant.

Learn more: [technical quickstart](@/tech/quickstart.md).
