# DeskBunny

Rust + Tauri v2 desktop app that flashes animated OLED faces onto Arduino boards
(SSD1306 128x64 over bit-banged SPI, no display library). Read
`docs/ARCHITECTURE.md` first: it is the contract between engine, firmware,
flasher and UI. Change the contract before changing an interface.

- `engine/` C99 face engine shared by firmware and simulator (pixel-identical).
- `firmware/` Arduino sketch + `boards.json`; built by CI with arduino-cli.
- `crates/` Rust: engine FFI + blob codec (`deskbunny-core`), flasher (`deskbunny-flash`).
- `app/` Tauri app, Svelte 5 frontend, black theme.

## Skill routing

When the user's request matches an available skill, invoke it via the Skill tool. Route only to skills in the session's available-skills list; answer directly for quick questions or small scoped edits.

Key routing rules:
- Product ideas/brainstorming → invoke /office-hours
- Strategy/scope → invoke /plan-ceo-review
- Architecture → invoke /plan-eng-review
- Design system/plan review → invoke /design-consultation or /plan-design-review
- Full review pipeline → invoke /autoplan
- Bugs/errors → invoke /investigate
- QA/testing site behavior → invoke /qa or /qa-only
- Code review/diff check → invoke /review
- Visual polish → invoke /design-review
- Ship/deploy/PR → invoke /ship or /land-and-deploy
- Save progress → invoke /context-save
- Resume context → invoke /context-restore
- Author a backlog-ready spec/issue → invoke /spec
