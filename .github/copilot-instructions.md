# Copilot Instructions — Mail (Tauri v2 + SvelteKit + Svelte 5)

## Project Overview

Desktop email client styled after **Outlook for Windows 11**, built with:

- **Frontend**: SvelteKit (static adapter, SSG) + Svelte 5 (runes: `$state`, `$derived`, `$props`)
- **Backend**: Rust (Tauri v2) — `src-tauri/`
- **Build**: Vite 6 + SvelteKit, Cargo for Rust

## Architecture — Layout (New Outlook for Windows 11)

```
┌──────────────────────────────────────────────────────┐
│  Mail ✉  [New mail] Reply Fwd Del │ Search │ 👤 ─□✕  │ 40px (custom title bar)
├────┬────────────┬──────────────┬─────────────────────┤
│Nav │ FolderPane │ MessageList  │    ReadingPane       │
│Rail│  (240px)   │   (360px)    │     (flex: 1)        │
│40px│            │              │                      │
└────┴────────────┴──────────────┴─────────────────────┘
```

- **TitleBar** (top, full-width, 40px): Custom title bar (no native decorations). Left: app icon + "New mail" + command actions (Reply, Forward, Delete, Archive, Move). Center: search box. Right: account avatar + window controls (minimize, maximize, close). Entire bar is draggable (`-webkit-app-region: drag`) except interactive elements. Uses `@tauri-apps/api/window` for `minimize()`, `toggleMaximize()`, `close()`.
- **NavigationRail** (left, 40px): Icon-only vertical rail — Mail / Calendar / Contacts + Settings gear at bottom
- **FolderPane** (left, 240px): Favorites, Folder tree with unread counts
- **MessageList** (center, 360px): Focused/Other tabs, email cards with avatars
- **ReadingPane** (right, flex): Email header, attachments, HTML body, empty state

## Key Files

| Path | Purpose |
|---|---|
| `src/routes/+page.svelte` | Main page — wires all 5 components together, state management |
| `src/routes/+layout.svelte` | Root layout — imports `app.css` |
| `src/routes/+layout.ts` | SvelteKit SSG config (`prerender: true`, `ssr: false`) |
| `src/app.css` | Global styles — Outlook color tokens, scrollbars, resets |
| `src/lib/components/TitleBar.svelte` | Custom title bar — commands, search, account, window controls |
| `src/lib/components/NavigationRail.svelte` | Left icon-only nav rail (40px) |
| `src/lib/components/FolderPane.svelte` | Folder sidebar — Favorites, folder tree, unread counts |
| `src/lib/components/MessageList.svelte` | Email list — Focused/Other tabs, email cards |
| `src/lib/components/ReadingPane.svelte` | Email reader — header, body, empty state |
| `src/lib/types.ts` | TypeScript types: `Email`, `Folder`, `Contact`, `NavItem` |
| `src/lib/data/mockData.ts` | Mock emails and folders |
| `src/lib/utils.ts` | Date formatting helpers |
| `src-tauri/` | Rust backend (Tauri v2 commands, config, capabilities) |
| `vite.config.js` | Vite + SvelteKit plugin config, Tauri dev server settings |

## Conventions

- **Svelte 5 runes**: Use `$state()`, `$derived()`, `$props()` — no legacy `export let` or `$:` reactive statements
- **Component props**: Use `interface Props { ... }` + `let { ... }: Props = $props()`
- **Callbacks**: Pass event handler functions as props (`onSelectEmail`, `onSelectFolder`)
- **CSS**: Scoped `<style>` blocks per component, reference CSS custom properties from `app.css`
- **Color tokens**: `--accent: #0078d4`, `--bg-primary: #fff`, `--text-primary: #242424` (see `app.css`)
- **Icons**: Inline SVGs (Fluent-style outlined, 1.5 stroke) — no icon library dependency
- **HTML email body**: Rendered via `{@html email.body}` in ReadingPane

## Development

```bash
npm install           # Install frontend dependencies
npm run dev           # Start SvelteKit dev server on :2040
npm run tauri dev     # Start full Tauri app (Rust + frontend)
npm run build         # Build frontend for production
npm run tauri build   # Build distributable Tauri app
```

## Tauri IPC

- Commands defined in `src-tauri/src/lib.rs` as `#[tauri::command]` functions
- Invoke from frontend: `import { invoke } from "@tauri-apps/api/core"`
- Window config in `src-tauri/tauri.conf.json` (1280×800, min 960×600)
