# Simple Mail
by Cortexist

A privacy-focused desktop mail, calendar, and contacts client built with Tauri, Svelte 5, and Rust. All data stays on your machine — no third-party cloud, no telemetry. Ships with light, dark, and system color themes, full internationalization in nine languages, and runs natively on Windows, macOS, and Linux.

[![Buy Me A Coffee](https://img.shields.io/badge/-buy_me_a_coffee-gray?logo=buy-me-a-coffee)](https://www.buymeacoffee.com/cortexist)

![Simple Mail by Cortexist](./screenshots/theme-i18n.png)

## Features

### Mail
- IMAP sync and SMTP sending with multiple accounts
- Focused Inbox (automatic importance bucketing)
- Rich HTML compose with attachments, recipient autocomplete, and draft saving
- Folder management (create, delete, empty, favorites)
- Message actions: star, pin, move, delete, mark read/unread
- Autodiscover for automatic server configuration
- Offline outbox queue with retry

### Calendar
- CalDAV sync with full CRUD
- Day / Week / Month views
- Recurring events (daily, weekly, monthly, yearly) with exception dates
- Event alerts via OS notifications
- Attendees with roles, iMIP meeting invitations over email
- All-day events, online meeting detection

### Contacts
- CardDAV sync with full CRUD
- Contact favorites, photos, distribution lists
- Mute contacts (hide without deleting)
- Quick "Meet" action to schedule a meeting with a contact

### General
- Multi-account support
- Embedded CalDAV/CardDAV server for local-network client access
- Secure credential storage via OS keyring + AES-256-GCM
- Light, dark, and system theme
- Custom titlebar, keyboard-driven navigation
- Command bar for quick actions

## Recent Enhancements

Highlights since late March 2026:

### Reliability & Performance
- **Crash-safe send.** Every outbound mail is queued with a stable `Message-ID` before SMTP submission. If the app is killed mid-send, the next sync reconciles against the IMAP Sent folder by `Message-ID` to decide whether to retry — no more silent duplicates.
- **Parallel account sync.** IMAP, CalDAV, CardDAV, and outbox flush run concurrently instead of sequentially when clicking Sync.
- **Faster initial sync** and large-folder rendering; per-folder sync wrapped in SQLite transactions; hot-path indexes added.
- **Non-blocking send.** Compose window closes immediately; the send runs in the background.
- **Global network activity indicator** — a thin progress bar under the command bar shows whenever any backend call is in flight.

### Mail
- **Replied status** persisted via the IMAP `\Answered` flag with push sync; reply timestamp shown in ReadingPane.
- **Priority / Regular** replaces Focused / Other.
- **Batch selection mode**, batch mark read/unread, multi-select actions.
- **Junk mail filter** with sender blocklist.
- **SPF / DKIM / DMARC** authentication results parsed, surfaced in ReadingPane with inline warning styling.
- **Extra headers** fetch & display.
- **Attachment context menu** for save / open.
- **Autodiscover** improvements for mail server configuration.

### Calendar
- Keyboard-driven event editing and navigation.
- Calendar search navigation; active-day and month-today visibility improvements.
- Online meeting detection and iMIP invitation layout polish.
- **Default-calendar rename heuristic is now ambiguity-safe.** Providers often name the user's primary calendar after the account holder (e.g. "Nakamoto, Takeshi"); we rename it to "Calendar" for a cleaner sidebar. When the server advertises `schedule-default-calendar-URL` (RFC 6638), that one calendar is renamed authoritatively. Without it, we fall back to a name heuristic — but only rename when **exactly one** calendar is a candidate. Two or more ambiguous candidates are left with their server-provided names instead of all collapsing to "Calendar" and producing duplicate sidebar entries.

### Contacts
- **Multiple emails, phones, and addresses** per contact with labels, subtypes, and default selection.
- **Pinned Favorites section** at the top of the contact list (collapsible, order-preserving, no alphabet separators).
- Default field text cleaned up; font size tuned.

### Folders
- **Pin / Unpin folders** (replacing "Favorites") — pinned folders float to the top of a single unified Folders pane with a pin indicator.
- Folder actions menu always visible in the Folders header.

### Settings
- Settings content header now hosts icon-button **Save**, **Delete**, **Start/Stop Server** actions with shortcuts (Ctrl+S, Ctrl+D / Del, Alt+S, Alt+Shift+S).
- **Account ordering** via up/down buttons.
- **Local Sync server** user adds are staged until Save, for consistency with other editors.
- Hover avatar to add or remove a photo; extra buttons removed.

### Internationalization
- Full UI i18n across Mail, Calendar, Contacts, Compose, DatePicker, TimePicker, and Settings in **English, Spanish, French, German, Italian, Japanese, Korean, Simplified Chinese, and Traditional Chinese**.

### Keyboard & Navigation
- Consistent keyboard navigation across Calendar, MessageList, Contacts modal, and Settings.
- Global shortcuts: Ctrl+F5 (sync), Alt+S (settings), Esc (clear search), and more.
- **Alt+1 … Alt+9** to switch between accounts (no on-screen affordance for this today — documenting it here until a visual hint is added).

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop shell | [Tauri 2](https://tauri.app) |
| Frontend | [Svelte 5](https://svelte.dev) + [SvelteKit](https://kit.svelte.dev) + TypeScript |
| Backend | Rust 2021 + [Tokio](https://tokio.rs) async runtime |
| Database | SQLite (bundled via rusqlite) |
| Protocols | IMAP, SMTP, CalDAV (RFC 4791), CardDAV (RFC 6352), Autodiscover |

## Prerequisites

- [Node.js](https://nodejs.org) >= 18
- [Rust](https://rustup.rs) >= 1.77
- Platform build tools for [Tauri](https://v2.tauri.app/start/prerequisites/)

## Getting Started

```bash
# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

The production binary and installer are output to `src-tauri/target/release/bundle/`.

## Project Structure

```
src/                      # Svelte 5 frontend
  routes/+page.svelte     # Main app shell
  lib/components/         # UI components (CalendarView, ComposePane, ContactsView, ...)
  lib/data/               # Data service and mock data
  lib/types.ts            # Shared TypeScript types
src-tauri/                # Rust backend
  src/lib.rs              # Tauri commands and app state
  src/imap_client.rs      # IMAP protocol client
  src/smtp_client.rs      # SMTP protocol client
  src/caldav_client.rs    # CalDAV client
  src/carddav_client.rs   # CardDAV client
  src/dav_server.rs       # Embedded DAV server
  src/autodiscover.rs     # Mail server autodiscovery
  src/db.rs               # SQLite schema and migrations
  src/crypto.rs           # AES-256-GCM encryption utilities
```

## License

[MIT](LICENSE) &copy; 2026 Cortexist, LLC
