# Lumen Source desktop

Tauri 2 + React + TypeScript desktop operator for discovering, installing, and
running private local AI models.

## Development

Install the packages listed in `package.json`, then run:

```sh
npm run tauri dev
```

The Vite UI is intentionally a presentation layer. Typed calls live in
`src/commands.ts`; the corresponding Tauri command handlers live in
`src-tauri/src/commands.rs`.

`src-tauri/src/bridge.rs` is the only adaptation seam for shared Rust APIs. The
shared crates are still scaffolds, so it currently provides deterministic
bootstrap responses that make the complete wizard usable. Replace those method
bodies with calls into `lumen-source-core` and its adapters when their public
interfaces land.

## Command surface

- Hardware detection
- Cached catalog load and network refresh
- Intent-based recommendations
- Installation preflight
- Installation with `install-progress` events
- Runtime start, stop, and status
- OpenAI-compatible endpoint details
