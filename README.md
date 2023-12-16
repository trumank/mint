# mint

3rd party mod integration tool for Deep Rock Galactic to download and integrate mods completely
externally of the game. This enables more stable mod usage as well as offline mod usage. Works for
both Steam and Microsoft Store versions.

<img alt="Graphical User Interface" src="https://github.com/jieyouxu/drg-mod-integration/assets/39484203/a09700f6-1932-4bc0-a64c-0f4e0d2faf53">

Mods are added via URL to a .pak or .zip containing a .pak. Mods can also be pulled from mod.io.
Examples:

 - `C:\Path\To\Local\Mod.zip`
 - `https://example.org/some-online-mod-repository/public-mod.pak`
 - `https://mod.io/g/drg/m/sandbox-utilities`

Mods from mod.io will require an OAuth token which can be obtained from <https://mod.io/me/access>
when prompted.

Most mods work just as if they were loaded via the official integration, but there are still some
behavioural differences. If a mod is crashing or otherwise behaving differently than when using the
official integration, *please* create an
[issue](https://github.com/trumank/mint/issues/new) so it can be addressed.

## Usage

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

<img alt="Settings menu" src="https://github.com/jieyouxu/drg-mod-integration/assets/39484203/09d12b01-7d2d-449e-97bb-47e4b4cdd301">

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

<img alt="mod.io Access page" src="https://github.com/jieyouxu/drg-mod-integration/assets/39484203/67096a62-8a3d-46f3-a106-cf6c5066e296">

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
