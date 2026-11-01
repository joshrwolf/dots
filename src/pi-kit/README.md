# pi-kit

Shared model validation, dots CLI access and presentation APIs for Pi-based
packages. Consumers use Pi's public host libraries; the root workspace catalog
owns their exact release pins.

- `@joshrwolf/pi-kit/models`: validate models/thinking against Pi's runtime catalog.
- `@joshrwolf/pi-kit/dots`: call the installed dots CLI with structured output.
- `@joshrwolf/pi-kit/ui`: shared panels, rows and safe text rendering.

Code runs from source. Add shared code when a second consumer needs it, not
before. Use `mise run //src/pi-kit:check`; see [build setup](../../docs/build.md).
