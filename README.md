# mint

> **Disclaimer**
>
> `mint` is not officially endorsed by Ghost Ship Games (GSG), not are `mint` contributors
> affiliated with GSG**. `mint` is a **third-party tool**, use at your own risk.

`mint` is third-party party mod integration tool for **Deep Rock Galactic** (DRG) to download and
integrate mods completely externally of the game. This enables more stable mod usage as well as
offline mod usage.

## Supported game versions

`mint` is intended to work for both Steam and Microsoft Store versions of DRG.

## Supported environments

Contributor bandwidth is very limited, so only select environments are intentionally supported
depending on what environments the contributors have access to test in. We are unable to help
diagnose your problems if you are using an environment that we don't have access to.

- `mint` is intended to run on:
    - Windows 11. Windows 7 is *not* supported as it is considered a [Tier 3 platform as per rustc's
      Target Tier Policy][win7-tier-3].
    - Common Linux distros. Ubuntu is what I have access to. Your mileage may vary depending on the
      exact distro and compositor.
- Running on macOS is *not* supported.

<img alt="Graphical User Interface" src="https://github.com/trumank/mint/assets/1144160/0305419f-a2af-4349-9d63-12e19d97102f">

Mods are added via URL to a .pak or .zip containing a .pak. Mods can also be pulled from mod.io.
Examples:

 - `C:\Path\To\Local\Mod.zip`
 - `https://example.org/some-online-mod-repository/public-mod.pak`
 - `https://mod.io/g/drg/m/sandbox-utilities`

Mods from mod.io will require an OAuth token which can be obtained from <https://mod.io/me/access>
when prompted.

Most mods work just as if they were loaded via the official integration, but there are still some
behavioural differences. If a mod is crashing or otherwise behaving differently than when using the
official mod.io integration, *please* create an [issue](https://github.com/trumank/mint/issues/new)
so it can be addressed.

## Usage etiquette

`mint` does not enforce sandbox saves. As such:

- **Use mods responsibly**.
- **Respect other people who wish to preserve progression**.
- **Do not host unmarked modded lobbies**. `mint` will automatically prepend a `[MODDED]` tag in
  front of your lobby name. We consider behaviors such as removing this prefix and hosting unmarked
  lobbies **disrespectful** to other players who probably did not consent to joining your modded
  lobby and has no means of telling otherwise.

For more details, please consult our [user guide](https://github.com/trumank/mint/wiki).

---

## Compiling from Source

`mint` can be tricky to build from source, because it has two components:

1. GUI and supporting library code (the "**app**")
2. DLL hook (the "**hook**")

The **app** needs to be built for your **host** target.

- If you are on Windows, this could be something like `x86_64-pc-windows-msvc`.
- If you are on Linux, this could be something like `x86_64-unknown-linux-gnu`.

The **hook** must be cross-compiled as a C dynamic library for the `x86_64-pc-windows-msvc` or
`x86_64-pc-windows-gnu` target depending on your environment. This means that you will need a
suitable cross-compiler to build the hook if your host environment is not one of
`x86_64-pc-windows-msvc` or `x86_64-pc-windows-gnu`.

### Requirements

You need a working Rust distribution to build `mint`, which can be acquired via the [`rustup`
installer][rustup].

`mint` requires a [**nightly** rust toolchain][rustup-nightly-toolchain].

You can acquire a nightly toolchain suitable for your host environment by:

```bash
$ rustup install nightly 
```

`mint` also requires [`gtk` dependencies][gtk].

#### Linux / WSL

On Linux, you will need a cross-compiler for `x86_64-pc-windows-gnu`. For example,
`x86_64-w64-mingw32-gcc`.

I only have access to Ubuntu, so the exact package names and installation instructions will depend
on your specific Linux distro.

##### Ubuntu

```bash
$ sudo apt-get update
$ sudo apt-get upgrade
# Basic build utils
$ sudo apt-get install build-essential pkg-config
# C cross-compiler
$ sudo apt-get install gcc-mingw-w64
# gtk3
$ sudo apt-get install libgtk-3-dev
```

#### Windows

On Windows, you'll need the latest Visual C++ build tools. It's easiest if you acquire it via
[`rustup-init`][rustup] and follow the recommended installation settings. See
<https://rust-lang.github.io/rustup/installation/windows-msvc.html> for more details.

### Compiling and running

For development builds,

```rs
$ cargo build
```

For actual usage, you should build with release profile

```rs
$ cargo build --release
```

[win7-tier-3]: https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html
[rustup]: https://rustup.rs/
[rustup-nightly-toolchain]: https://rust-lang.github.io/rustup/concepts/toolchains.html
[gtk]: https://www.gtk.org/docs/installations/

---

## Basic Usage

This section assumes that you are on Windows and is using the steam version of DRG, working with
either local `.pak`s or mod.io mods.

First, download the [latest release](https://github.com/trumank/mint/releases/latest)
compatible with your architecture. For windows, this will be the
`mint-x86_64-pc-windows-msvc.zip`. Extract this to anywhere you'd like to keep the
executable.

Then, we'll need to perform some first-time setup.

### First Time Setup

We need to provide the tool with the path to `FSD-WindowsNoEditor.pak` and a mod.io OAuth token if
you want to use mod.io mods. These can be configured in the settings menu (cogwheel located in the
bottom toolbar).

<img alt="Settings menu" src="https://github.com/trumank/mint/assets/1144160/b009a74c-b13a-4b84-95f9-4c59c6debb62">

#### Locating the DRG `FSD-WindowsNoEditor.pak`

If the tool fails to detect your DRG installation, then you can manually browse to add the path to
`FSD-WindowsNoEditor.pak`.

This file is located under the `FSD` folder inside your DRG installation directory, e.g.

```
E:\SteamLibrary\steamapps\common\Deep Rock Galactic\FSD\FSD-WindowsNoEditor.pak
```

#### Adding a mod.io OAuth Token

Inside the settings menu, there is a modio setting (cogwheel). If you click on that, it will prompt
for an mod.io OAuth token.

To generate a mod.io OAuth token, you'll need to visit <https://mod.io/me/access>. You'll need to
accept the API terms and conditions.

<img alt="mod.io Access page" src="https://github.com/trumank/mint/assets/1144160/2aeb6135-71c2-4c3c-8979-49e84b276bed">

Then, you'll need to add a new client under OAuth Access, call it e.g. "DRG Mod Integration".

For that client, create a new token named e.g. "modio-access" with Read-only scope. Copy the token
into the integration tool's prompt.

### Adding Mods

After these steps, you can now add local mods or mod.io mods.

#### Adding mod.io mods

Copy the URL to the mod into the "Add mods..." field and hit enter.

You can obtain a list of your subscribed mods list using the "Copy Mod URLs"
button via [A Better Modding Menu](https://mod.io/g/drg/m/a-better-modding-menu)
in game:

![Copy Mod URLs](https://github.com/trumank/mint/assets/1144160/375f441f-4762-4549-a241-1b54ed391b2f)

#### Adding a local mod

You can either drag and drop a local `.pak` file on to the tool window, or add the path to the
local `.pak` in the same "Add mods..." field.

### Updating Cache

The versioned mod.io mods are *cached*. If you want to update to the latest version of your mods,
you'll need to press the "Update cache" button.

### Installing/uninstalling mods

Once you are happy with your mod profile, you can install the mods by pressing the "Install mods"
button, and uninstall them with the "Uninstall mods" button. **This must be done while the game is
closed.**

## Using integrated mod support again

If you want to go back to the integrated mod support again, you must uninstall the mods installed by
mint. Then, launch the game normally.
