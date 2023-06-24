# drg-mod-integration
3rd party mod integration tool to download and integrate mods completely
externally of the game. This enables more stable mod usage as well as offline
mod usage. Works for both Steam and Microsoft Store versions.

![gui-interface](https://github.com/trumank/drg-mod-integration/assets/1144160/92262061-eb05-42f5-973c-7f55888ee7e6)

Mods are added via URL to a .pak or .zip containing a .pak. Mods can also be pulled from mod.io. Examples:
 - `C:\Path\To\Local\Mod.zip`
 - `https://example.org/some-online-mod-repository/public-mod.pak`
 - `https://mod.io/g/drg/m/sandbox-utilities`

 Mods from mod.io will require an OAuth token which can be obtained from https://mod.io/me/access when prompted.

