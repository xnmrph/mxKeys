# App icon

`icon.html` draws the icon (a raised keycap with a blackletter "mx" in
Grenze Gotisch 900, OFL). To regenerate:

1. Render `icon.html` at 1024×1024 with a transparent background (e.g. a headless
   browser screenshot) to `src-tauri/icons/source-1024.png`.
2. `npx tauri icon src-tauri/icons/source-1024.png`, then delete the generated
   `android/` and `ios/` folders.

# Logo / banner

`banner.html` draws the banner (the mx key with a yellow homing bar, then K E Y S
keycaps). Render `#banner` with a transparent background to `docs/assets/mxkeys-banner.png`.
The app draws the same logo with `src/Logo.tsx`.
