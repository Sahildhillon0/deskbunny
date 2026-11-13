# Third-party notices

DeskBunny itself is MIT licensed (see `LICENSE`). It builds on open-source
components that keep their own licenses. The authoritative list for each part
is generated from its package manager:

- **Rust crates** (`crates/`, `app/src-tauri`): `cargo install cargo-license && cargo license`
- **Svelte/Tauri frontend** (`app/`): `pnpm licenses list`
- **Flutter app** (`mobile/`): the app's *Open source licenses* page, or `flutter pub deps`;
  Flutter, Dart and the plugins used are BSD-3-Clause / MIT / Apache-2.0.
- **Weather and air-quality data**: [Open-Meteo](https://open-meteo.com/) (CC BY 4.0, non-commercial free API).
- **5x7 pixel font** in `mobile/lib/app/pixel_font.dart`: drawn for this project.
