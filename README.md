# mxKeys

An all-in-one desktop app that makes [ZMK](https://zmk.dev) keyboards approachable:
visual keymap editing, settings, firmware builds **inside the app** (no Docker/Nix/SDK
installs), guided UF2 flashing, git sync — with your zmk-config repo as the single source
of truth.

> Early development. v1 targets one setup: **Corne + nice!nano v2 + nice!view**.
> See [docs/ROADMAP.md](docs/ROADMAP.md) and [docs/PROGRESS.md](docs/PROGRESS.md).

## Layout

| Path | What |
|---|---|
| `crates/zmk-config` | Lossless `.keymap` (devicetree) parser/editor, `build.yaml` parser |
| `src-tauri` | Tauri v2 shell (thin command layer) |
| `src` | React + TypeScript UI |
| `docs` | Architecture, roadmap, progress log |

## Develop

Requirements: Rust (stable), Node 22. On Linux also the WebKitGTK 4.1 dev packages
(Debian/Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libudev-dev libssl-dev`; NixOS:
`nix-shell`).

```bash
npm install
```

```bash
npm run tauri dev
```

```bash
cargo test --workspace
```

## License

MIT. Test fixture `corne_stock.keymap` is from ZMK (MIT, © The ZMK Contributors).
