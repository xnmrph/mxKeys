# App icon

`icon.html` draws the icon (a raised keycap with a blackletter "mx" in
Grenze Gotisch 900, OFL). To regenerate:

1. Render `icon.html` at 1024×1024 with a transparent background (e.g. a headless
   browser screenshot) to `src-tauri/icons/source-1024.png`.
2. `npx tauri icon src-tauri/icons/source-1024.png`, then delete the generated
   `android/` and `ios/` folders.
