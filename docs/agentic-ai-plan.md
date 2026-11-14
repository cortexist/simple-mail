# Simple Mail — AI & External-Agent Plan

> Status: **Planning** (not yet implemented). Saved 2026-06-23.
> Scope: turn the mail app into (1) a **tool** external agents can drive, and (2) an **agent** with its own model harness.
> Implementation-ready task breakdown: [`agentic-ai-tasks.md`](./agentic-ai-tasks.md).

## Locked decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| External surface (Role 1) | **MCP server**, local HTTP/SSE, app-key gated | Consumers (OpenClaw/Hermes) are LLM agents → native tool use |
| Model API (Role 2) | **OpenAI-compatible** `/chat/completions` | One shared config, `base_url`-swappable (OpenAI / OpenRouter / Azure / Ollama / vLLM); function-calling for reference retrieval |
| Classification timing | **Auto on sync, per-account toggle** | Reuses existing folder/flag echoes; background, no new frontend wiring |
| Model config scope | **One global config**, features toggle **per account** | Matches "all accounts use the same API, features set individually" |

## Core architectural principle: one service layer, three surfaces

Today, operation logic lives **inside** `#[tauri::command]` functions in `src-tauri/src/lib.rs`. To serve UI, the internal harness, and external agents without duplication, extract the cores into a plain-Rust **service layer**; each surface becomes a thin wrapper.

```
                       ┌──────────────────────────────────────┐
   Tauri UI ──────────▶│ Tauri commands (thin wrappers)        │
                       ├──────────────────────────────────────┤
   Internal agent ────▶│ SERVICE LAYER (plain Rust fns)        │──▶ DB (rusqlite)
   harness (no auth)   │   mail_service::{search,get,move,     │──▶ IMAP/SMTP
                       │     delete,send,draft}                │──▶ LLM (llm_client)
   External agents ───▶│   ai_service::{classify,draft_reply,  │
   (MCP + app key)     │     find_reference_mail}              │
                       └──────────────────────────────────────┘
```

- Service fns take `(db: &Arc<Mutex<Connection>>, app: &AppHandle, seq, args…)` so they can emit the existing push echoes regardless of caller.
- **"More efficient without auth"** = the internal harness calls these fns **in-process** (no app-key, no HTTP); only external agents pay the MCP + app-key cost.
- This refactor is mechanical and low-risk: each command body moves to a `*_service` fn; the command calls it. Do it incrementally (Phase 1) so existing behavior is preserved.

---

## Grounding: what already exists (reuse, don't rebuild)

| Capability | Location | Reuse for |
|-----------|----------|-----------|
| AES-256-GCM encryption, master key in OS keyring (`cortexist-mail`) | `src-tauri/src/crypto.rs` (`encrypt_password`/`decrypt_password`) | Encrypt LLM API key at rest |
| Axum + `tokio::net::TcpListener` server, start/stop, graceful shutdown, handle in `AppState` | `src-tauri/src/dav_server.rs` (`start_server`, `AppState.dav_handle`) | Template for the MCP server |
| Outbound HTTP client | `reqwest` 0.12 (rustls, http2) in `Cargo.toml` | LLM `/chat/completions` calls |
| KV settings + per-account settings tables + change echoes | `settings` table (`db.rs`), `settings:*` events (`events.rs`), `settingsStore.svelte.ts` | AI config + per-account toggles |
| Per-account toggle pattern (fire + subscribe echo) | `fireSetAccountOfflineDownload` + `subscribeSettingsOfflineDownloadChanged` (`settingsStore.svelte.ts:310/149`) | New per-account AI feature toggles |
| Priority columns + post-sync reclassifier (rule-based) | `is_focused`, `is_focused_manual` (`db.rs:74`); SQL CASE reclassifier (`lib.rs:1685-1729`) | LLM priority sets `is_focused`, respecting `is_focused_manual=1` |
| Junk routing: `sender_blocklist` + `is_sender_blocked` + auto-move on insert | `db.rs:157/240`, `lib.rs:1599-1611` | LLM junk verdict moves inbox→junk via same mechanism |
| Push echoes (idempotent set, seq-ordered) | `mail:email-folder-changed`, `mail:email-flag-changed` (`events.rs`), `applyFolderEcho`/`applyFlagEcho` (`+page.svelte`) | Classification results surface with **zero new frontend wiring** |
| Monotonic event seq | `AppState.next_seq()` (`lib.rs:279`) | Order classification echoes |
| Background task spawn | `tauri::async_runtime::spawn` (e.g. `lib.rs:987`, `offline_download.rs:81`) | Async classify pass after sync |

**Gaps to fill:** drafts are **frontend-only in-memory** (`emails` array, `draftData`; never persisted to DB) — a backend/agent-generated draft needs a new `mail:draft-generated` event to surface. Search is **purely client-side** (`searchQuery.ts`) — but bodies are in the DB, so backend reference-retrieval queries the `emails` table directly. **No existing AI/LLM code** anywhere.

---

## Data model changes

```sql
-- Singleton global model config (one API for all accounts).
CREATE TABLE IF NOT EXISTS ai_config (
  id             INTEGER PRIMARY KEY CHECK (id = 1),
  enabled        INTEGER NOT NULL DEFAULT 0,
  base_url       TEXT NOT NULL DEFAULT 'https://api.openai.com/v1',
  api_key_enc    TEXT NOT NULL DEFAULT '',          -- crypto::encrypt_password
  classify_model TEXT NOT NULL DEFAULT 'gpt-4o-mini',
  draft_model    TEXT NOT NULL DEFAULT 'gpt-4o-mini',
  max_tokens     INTEGER NOT NULL DEFAULT 1024,
  temperature    REAL NOT NULL DEFAULT 0.2
);

-- Per-account feature toggles (all default off).
CREATE TABLE IF NOT EXISTS account_ai_features (
  account_id      TEXT PRIMARY KEY,
  junk_filter     INTEGER NOT NULL DEFAULT 0,
  priority_detect INTEGER NOT NULL DEFAULT 0,
  auto_draft      INTEGER NOT NULL DEFAULT 0,   -- allow draft generation
  external_access INTEGER NOT NULL DEFAULT 0,   -- Role 1: MCP may act on this account
  FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

-- Idempotency + explainability for classification (skip already-seen mail).
CREATE TABLE IF NOT EXISTS email_ai_classification (
  email_id      TEXT PRIMARY KEY,
  is_junk       INTEGER,
  is_priority   INTEGER,
  reason        TEXT,
  model         TEXT,
  classified_at TEXT,
  FOREIGN KEY (email_id) REFERENCES emails(id) ON DELETE CASCADE
);

-- External app keys for the MCP server (store a hash, compare constant-time).
CREATE TABLE IF NOT EXISTS agent_api_keys (
  id         TEXT PRIMARY KEY,
  label      TEXT NOT NULL,
  key_hash   TEXT NOT NULL,        -- sha256(key)
  created_at TEXT NOT NULL,
  last_used  TEXT
);
```

`AppState` gains: `ai: Mutex<Option<AiConfig>>` (cached) and `mcp_handle: Mutex<Option<McpServerHandle>>` (mirrors `dav_handle`). New crate deps: `sha2` (hash app keys); LLM client uses existing `reqwest`/`serde_json`.

---

## Feature 1 — Junk + priority classification (auto on sync)

**Flow.** `sync_mail` already emits `mail:sync-completed` after inserting new mail (`lib.rs:1747`). Add, right after, when `ai_config.enabled` and the account has `junk_filter` or `priority_detect`:

```rust
tauri::async_runtime::spawn(ai_service::classify_new_mail(db.clone(), app.clone(), account_id));
```

`classify_new_mail`:
1. Select inbox emails for the account **not in** `email_ai_classification` (only new/unseen). Cap per pass (e.g. 50) to bound cost.
2. **Batch** them into one (or few) LLM calls — compact each to `{id, from, subject, snippet}` (truncate body to ~500 chars). Use JSON-mode/function-calling: model returns `[{id, is_junk, is_priority, reason}]`.
3. Apply, reusing existing mechanisms + echoes:
   - `is_junk` → `mail_service::move_email(... "junk")` → emits `mail:email-folder-changed`.
   - `is_priority` → set `is_focused` **only when `is_focused_manual = 0`** → emits `mail:email-flag-changed` (field `focused`).
   - Write verdict to `email_ai_classification`.
4. Frontend updates live via `applyFolderEcho`/`applyFlagEcho` — **no new listeners**.

**Correctness guards:** never re-junk a message the user manually moved out (it's already classified / no longer in inbox); never override `is_focused_manual=1`; idempotent on re-sync (classification table dedups). The existing rule-based reclassifier (`lib.rs:1685`) stays as the zero-config fallback; LLM runs only when enabled and takes precedence for unseen mail.

---

## Feature 2 — Draft response + reference retrieval (agentic)

**`ai_service::draft_reply(db, app, account_id, email_id, instructions?)`:**
1. Load target email + its thread (by `message_id` / `reply_to` / subject).
2. **Reference retrieval (the agentic step).** `find_reference_mail` builds a candidate set from the DB (same sender, same thread, subject/body keyword match) → compact list `{id, from, subject, date, snippet}`. Give the model a `pick_references(ids)` tool (function-calling): it selects which candidates are relevant; we fetch those bodies and add them to context. (One-shot "which ids are relevant?" call is the simple version; a multi-turn tool loop with `search_mail(query)` is the richer version.)
3. Build the prompt (system persona + signature from `accounts.signature` + thread + selected references + user instructions) → `draft_model` call → draft HTML.
4. **Surface it:** emit new `mail:draft-generated` { accountId, inReplyTo?, to, cc, subject, bodyHtml, references[] }. Frontend subscribes, inserts an `Email` (folder `'drafts'`, `draftData` populated via the existing `createDraftEmail` shape) and optionally opens it in `ComposePane`. Add an "AI draft" button in compose/reading view that calls a `ai_draft_reply` Tauri command.

> Note: drafts are not persisted to the DB today, so a generated draft is ephemeral like any other. Persisting drafts is a separate enhancement (see Open questions).

---

## Feature 3 — MCP server (external tool role)

**`agent_mcp.rs`** — clone the `dav_server.rs` scaffolding: Axum router on `127.0.0.1:<port>`, started/stopped via `start_agent_server`/`stop_agent_server` commands, handle in `AppState.mcp_handle`, graceful shutdown via `watch::channel`.

- **Protocol:** MCP / JSON-RPC 2.0 over HTTP POST (+ SSE for streaming): `initialize`, `tools/list`, `tools/call`.
- **Auth:** `Authorization: Bearer <app-key>` checked at the HTTP layer — `sha256(key)` constant-time compared against `agent_api_keys`. Loopback bind only.
- **Tools** (operate only on accounts with `external_access = 1`; each maps 1:1 to a service-layer fn):

  | Tool | Service fn |
  |------|-----------|
  | `mail.list_accounts` | enabled accounts only |
  | `mail.search(account, query, limit)` | `mail_service::search` |
  | `mail.get(account, email_id)` | `mail_service::get` |
  | `mail.send(account, to, cc, subject, body)` | `mail_service::send` |
  | `mail.draft(account, in_reply_to?, instructions)` | `ai_service::draft_reply` |
  | `mail.move(account, email_id, folder)` | `mail_service::move_email` |
  | `mail.delete(account, email_id)` | `mail_service::delete` |
  | `mail.classify(account)` | `ai_service::classify_new_mail` (backfill) |

External calls emit the same push echoes, so the desktop UI reflects agent actions live.

---

## Settings UI

- **New "AI" tab** (global): `enabled`, `base_url`, `api_key`, `classify_model`, `draft_model`, **Test** button (a `test_ai_config` command does a 1-token round trip). Mirror the mail-settings save/echo pattern.
- **Per-account** (in account settings): toggles for `junk_filter`, `priority_detect`, `auto_draft`, `external_access` — follow the offline-download toggle pattern (`fireSet… + subscribe…` echo).
- **Agent API** panel: start/stop MCP server + port, generate/revoke app keys (show key once), and the per-account `external_access` allowlist.

---

## Security, cost & privacy

- API key encrypted at rest (`crypto.rs`); app keys stored **hashed**; MCP **loopback-only** + app-key required + per-account allowlist.
- Cost: per-account toggles **default off**; classify only **new/unclassified** mail; **batch** per call; truncate bodies for classification; `max_tokens` cap; separate cheap `classify_model`. Optional daily token budget (future).
- **Privacy:** classification/drafting sends mail content to the configured endpoint. Make this explicit in the UI and support **local endpoints** (Ollama/vLLM via `base_url`) for fully-local operation.
- Rate-limit MCP per key (future enhancement).

---

## Phasing (each independently shippable)

0. **Foundations** — `ai_config` + `account_ai_features` tables; AI settings tab + per-account toggles; `llm_client.rs` (OpenAI-compatible) + `test_ai_config`. No behavior change.
1. **Service-layer refactor** — extract `mail_service::{search,get,move,delete,send}` from existing command bodies; commands become thin wrappers. Pure refactor, behavior-preserving.
2. **Classification** — `email_ai_classification` table; `ai_service::classify_new_mail`; auto-on-sync hook; reuse echoes; manual `classify_inbox` backfill command. (Delivers junk + priority.)
3. **Drafting + references** — `ai_service::draft_reply`, `find_reference_mail`, `mail:draft-generated` event + frontend insert/open, "AI draft" button.
4. **MCP server** — `agent_mcp.rs`, `agent_api_keys`, `external_access`, tool→service mapping, start/stop UI.

## Testing

- Unit: classification prompt build + JSON parse with an **injected mock LLM client** (trait); `find_reference_mail` SQL heuristics; app-key hash/compare.
- Integration: point `base_url` at a local mock or Ollama for end-to-end classify/draft; MCP `tools/list` + `tools/call` with/without a valid app key.
- Invariants: re-sync does **not** reclassify (dedup table); `is_focused_manual` preserved; agent actions emit correct echoes and update the UI.

## Open questions / future

- Persist drafts to the DB (currently ephemeral) so generated drafts survive restart.
- Embedding-based reference retrieval for large mailboxes (vs heuristic + LLM pick).
- Streaming draft tokens into `ComposePane`.
- MCP **stdio** transport variant for agents that prefer it.
- Per-account-scoped app keys + usage/token dashboard.
