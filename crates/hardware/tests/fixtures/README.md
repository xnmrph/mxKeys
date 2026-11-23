# Detection fixtures

| File | Origin |
|---|---|
| `owner_*` | The project owner's real config files (`xnmrph/corne`, `xnmrph/corne-macOS-Optimised`). |
| `uf2/*.txt` | **Synthesised** INFO_UF2.TXT contents in the documented Adafruit nRF52 / RP2040 bootloader format. Values (Board-IDs, versions) are from memory — replace with real captures once hardware is available (see docs/PROGRESS.md). |
| `usb/*.json` | **Synthesised** USB descriptor lists. `7504:24926` = ZMK's `1D50:615E` (from ZMK `app/Kconfig`); `9114:179` = `239A:00B3`, believed to be the nice!nano bootloader — unverified. |
