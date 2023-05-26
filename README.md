# drg-mod-integration
3rd party mod integration tool to download and integrate mods completely
externally of the game. This enables more stable mod usages as well as offline
mod usages.

```
$ ./drg_mod_integration integrate --help
Command line integration tool

Usage: drg_mod_integration integrate [OPTIONS]

Options:
  -d, --drg <DRG>
          Path to the "Deep Rock Galactic" installation directory. Only necessary if it cannot be found automatically

  -u, --update
          Update mods. By default all mods and metadata are cached offline so this is necessary to check for updates

  -m, --mods [<MODS>...]
          Paths of mods to integrate

          Can be a file path or URL to a .pak or .zip file or a URL to a mod on https://mod.io/g/drg
          Examples:
              ./local/path/test-mod.pak
              https://mod.io/g/drg/m/custom-difficulty
              https://example.org/some-online-mod-repository/public-mod.zip

  -h, --help
          Print help (see a summary with '-h')
```
