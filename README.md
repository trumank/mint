# drg-mod-integration

3rd party mod integration tool for Deep Rock Galactic to download and integrate mods completely
externally of the game. This enables more stable mod usage as well as offline mod usage. Works for
both Steam and Microsoft Store versions.

![gui-interface](https://github.com/trumank/drg-mod-integration/assets/1144160/fbb7a77f-4347-4d3f-bfa3-ee35254a3867)

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
[issue](https://github.com/trumank/drg-mod-integration/issues/new) so it can be addressed.

## building

The `bindeps` unstable feature is required to build and include the DLL hook which requires both
nightly and the `bindeps` feature to be specified like so:

    cargo build --release -Z bindeps

Alternatively, the `bindeps` feature can enable in the user's `.cargo/config.toml` file:

```toml
[unstable]
bindeps = true
```
