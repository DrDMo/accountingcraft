# Attribution

Every non-code asset AccountingCraft uses, with its author, source and license. AccountingCraft's own
code and original assets are MIT OR Apache-2.0 (see [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE)). When you add an asset, add a row here in the same change.

| Path | What | Author | Source | License |
|---|---|---|---|---|
| `src/icon.rs`, `assets/app-icon/hicolor/128x128/apps/accountingcraft.png` | The abacus app icon, drawn in code (the PNG and the Windows `.ico` are rendered from it) | AccountingCraft contributors | Original work | MIT OR Apache-2.0 |
| `docs/screenshots/*.png` | Screenshots of AccountingCraft with its made-up sample books | AccountingCraft contributors | Captured from the app | MIT OR Apache-2.0 |
| (build input, not in this repo) `fonts/inter/Inter-Regular.ttf`, `Inter-SemiBold.ttf` | Inter 4.1, the PDF font | The Inter Project Authors (Rasmus Andersson) | [storytold/craft-fonts](https://github.com/storytold/craft-fonts), via `CRAFT_FONTS_DIR` (see `build.rs`) | SIL OFL 1.1, `fonts/inter/OFL.txt` in craft-fonts |
| (dependency) `epaint_default_fonts/fonts/Ubuntu-Light.ttf` | egui's default UI font; also the PDF font when built without craft-fonts | Canonical Ltd. | Bundled by `egui 0.37` (`epaint_default_fonts`) | Ubuntu Font Licence 1.0 |
| (dependency) other `epaint_default_fonts` fonts | Hack, Noto Emoji, emoji-icon-font, egui-icons: egui's built-in fallbacks | See the `epaint_default_fonts` crate | Bundled by `egui 0.37` | MIT, OFL-1.1, Bitstream Vera (see that crate) |
| (system, not distributed) Segoe UI | The UI font on Windows, read from `C:\Windows\Fonts` when present | Microsoft | The user's Windows install | Not redistributed |
