# Change Log

<!-- next-header -->

## [Unreleased] - ReleaseDate

- Microsoft Store: Fix mods being unable to write custom save files

## [0.2.9] - 2023-08-11

- Update `egui_dnd` which makes dragging and re-ordering mods significantly smoother
- Restore modding subsystem config upon uninstalling to prevent all mods getting enabled and kicking the user to sandbox
- Fix regression introduced by case sensitive path fix ([#36](https://github.com/trumank/drg-mod-integration/issues/36))

## [0.2.8] - 2023-08-05

- Fix `*.ushaderbytecode` files not being filtered out and causing crash on load
- Fix including same asset paths with different casings causing Unreal Engine to load neither ([#29](https://github.com/trumank/drg-mod-integration/issues/29))

<!-- next-url -->
[Unreleased]: https://github.com/trumank/drg-mod-integration/compare/v0.2.9...HEAD
[0.2.9]: https://github.com/trumank/drg-mod-integration/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/trumank/drg-mod-integration/compare/v0.2.7...v0.2.8
