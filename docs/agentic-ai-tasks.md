# Agentic Mail — AI & External-Agent Task Breakdown

> Companion to [`agentic-ai-plan.md`](./agentic-ai-plan.md). Status: **not started.**
> Legend — Size: **S** ≤½ day · **M** ~1–2 days · **L** ~3–5 days. Each task is meant to be roughly one PR.
> File line numbers are anchors from the codebase as of 2026-06-23 and may drift.

## Critical path & parallelism

```
P0 Foundations ──▶ P1 Service refactor ──┬──▶ P2 Classification ──┐
                                          ├──▶ P3 Drafting ────────┼──▶ P4 MCP
                                          └────────────────────────┘
```

- **P0** and **P1** are mostly independent and can run in parallel (P1 touches `lib.rs` command bodies; P0 adds new tables/commands/UI).
- **P2** and **P3** both depend on P1 (service layer) + P0.5 (LLM client). They can run in parallel once those land.
- **P4** depends on P1 (tools wrap service fns) and P3 (`mail.draft` tool) + P2 (`mail.classify` tool).
- Suggested **first slice** (one sitting): P0.1 + P0.2 + P0.5 → you can persist config and ping the model. Everything else builds on that.

---

## Phase 0 — Foundations

| ID | Task | Files | Size |
|----|------|-------|------|
| P0.1 | Add `ai_config` (singleton) + `account_ai_features` tables; seed the singleton row; create a feature row on `add_account` | `src-tauri/src/db.rs` (CREATE TABLE block near `:14`; migration alongside `migrate_encrypt_passwords` `:626`); `lib.rs` `add_account` `:843` | S |
| P0.2 | `AiConfig` struct (serde `camelCase`) + load/save with **api_key encrypted** on save / decrypted on load | new `src-tauri/src/ai_config.rs` (mirror `sync_state::save_mail_settings` `:176`); `crypto::encrypt_password`/`decrypt_password` | M |
| P0.3 | `LlmClient` trait + `HttpLlmClient` (OpenAI-compatible `/chat/completions` via `reqwest`); `chat_completion(messages, tools?, response_format?)`; timeouts + error mapping | new `src-tauri/src/llm_client.rs` | M |
| P0.4 | Commands `get_ai_config` (return **masked** key / `hasKey: bool`, never plaintext), `set_ai_config`, `test_ai_config` (1-token ping via P0.3); register in `generate_handler!`; add `settings:ai-config-changed` event | `lib.rs` `:5628` (handler list), `:360` (command pattern); `events.rs` (`:38` consts + emit helper) | M |
| P0.5 | Commands `get_account_ai_features`, `set_account_ai_features`; `settings:ai-features-changed` event; register | `lib.rs`, `events.rs` | S |
| P0.6 | Frontend store: `fireSetAiConfig`/`fireTestAiConfig`/`fireSetAccountAiFeatures` + `subscribeSettings*` echoes; TS types | `src/lib/stores/settingsStore.svelte.ts` (mirror `fireSetAccountOfflineDownload` `:310` + `subscribe…` `:149`); `src/lib/types.ts` | M |
| P0.7 | SettingsView: new **"AI" tab** (global: enabled, baseUrl, apiKey, classifyModel, draftModel, **Test**) + per-account toggles (junkFilter, priorityDetect, autoDraft, externalAccess); **privacy disclosure** copy | `src/lib/components/SettingsView.svelte` (`accountForm` `:79`, tab switch); subscribe echoes in `+page.svelte` `:878`, unlisten in `onDestroy` `:944` | L |

**Notes.** P0.1: `ai_config` uses `CHECK (id = 1)` singleton; `account_ai_features` FK `ON DELETE CASCADE`. Decision: **no AppState cache** in v1 — load config from DB per use (cheap, avoids invalidation bugs); revisit only if profiling says so. P0.4: the masked-key rule is a security acceptance criterion, test it.

**Phase DoD:** AI config persists (key encrypted at rest, never returned plaintext), **Test** button does a live round-trip, per-account toggles round-trip through echoes, all default **off**.

---

## Phase 1 — Service-layer refactor (behavior-preserving)

| ID | Task | Files | Size |
|----|------|-------|------|
| P1.1 | Create `mail_service.rs`; move the **core** of `move_email` into `mail_service::move_email(db,&app,seq,…)`; command becomes a thin wrapper | `src-tauri/src/lib.rs` `move_email` `:2015`; new `mail_service.rs` | M |
| P1.2 | Same extraction for `delete_email` `:1774` and `send_email` `:2595` (async; keep the spawn inside the service fn) | `lib.rs`, `mail_service.rs` | M |
| P1.3 | New `mail_service::search(db, account_id, query, limit)` → compact rows `{id, from, subject, date, snippet}` over the `emails` table (no backend search exists today) | `mail_service.rs`; `emails` schema `db.rs:60` | M |
| P1.4 | New `mail_service::get(db, account_id, email_id)` → full email (+ body) | `mail_service.rs` | S |

**Notes.** Keep each `#[tauri::command]` as a 2–3 line wrapper calling the service fn so the UI path is untouched. Service fns take `&AppHandle` (available in commands and in spawned tasks via clone) so they emit the same echoes. **This phase ships no user-visible change** — its DoD is "all existing flows behave identically" (manually exercise move/delete/send + flags, confirm echoes still arrive).

---

## Phase 2 — Junk + priority classification

| ID | Task | Files | Size |
|----|------|-------|------|
| P2.1 | `email_ai_classification` table + migration | `db.rs` | S |
| P2.2 | `ai_service::classify_new_mail(db,&app,account_id)` — select inbox emails **not in** `email_ai_classification` (cap ~50), build compact batch, call `LlmClient` with JSON/tool output → `[{id,is_junk,is_priority,reason}]` | new `src-tauri/src/ai_service.rs`; `llm_client` (P0.3) | L |
| P2.3 | Apply verdicts: junk → `mail_service::move_email(…,"junk")`; priority → set `is_focused` **only if `is_focused_manual=0`** + emit `mail:email-flag-changed`; insert classification rows; stamp `next_seq()` | `ai_service.rs`; `lib.rs` reclassifier ref `:1685`, focused flag `:1220` | M |
| P2.4 | Hook into sync: after `emit_mail_sync_completed` `:1747`, `spawn(classify_new_mail)` when `ai_config.enabled` AND account has `junk_filter`/`priority_detect` | `lib.rs` `sync_mail` end | S |
| P2.5 | Manual backfill command `classify_inbox(account)` + register (reused by MCP `mail.classify`) | `lib.rs`, `ai_service.rs` | S |
| P2.6 | Prompt + structured-output schema; body truncation (~500 chars); batch-size constant; reason logging | `ai_service.rs` | M |
| P2.7 | Tests: inject `MockLlmClient`; idempotency on re-sync; `is_focused_manual` preserved; junk-then-user-unjunk not re-junked | `ai_service.rs` tests | M |

**Notes.** Reuses the **existing** `applyFolderEcho`/`applyFlagEcho` (`+page.svelte`) — **no new frontend listeners** for this phase. The rule-based reclassifier (`lib.rs:1685`) remains the zero-config fallback; LLM verdicts take precedence for unseen mail only.

**Phase DoD:** with a configured endpoint (or local Ollama), newly-synced junk moves to Junk and priority flags flip **live** in the UI; re-sync does not reclassify; manual overrides survive.

---

## Phase 3 — Draft reply + agentic reference retrieval

| ID | Task | Files | Size |
|----|------|-------|------|
| P3.1 | `ai_service::find_reference_mail(db,account_id,seed)` — DB heuristic candidates (same sender; thread via `reply_to`/`message_id`/subject; keyword match) → compact list | `ai_service.rs`; `emails` schema | M |
| P3.2 | `ai_service::draft_reply(db,&app,account_id,email_id,instructions?)` — load thread, run reference retrieval + **LLM `pick_references` tool call**, build prompt (persona + `accounts.signature` + thread + refs), call `draft_model` → HTML | `ai_service.rs` | L |
| P3.3 | `mail:draft-generated` event + payload `{accountId,inReplyTo?,to,cc,subject,bodyHtml,references[]}` + emit helper | `events.rs` | S |
| P3.4 | Command `ai_draft_reply(account_id,email_id,instructions?)`; register; spawns P3.2; emits P3.3 | `lib.rs` | S |
| P3.5 | Frontend: subscribe `mail:draft-generated` → insert `Email`(folder `drafts`, `draftData`) via a `createDraftEmail`-shaped builder → open in `ComposePane`; unlisten in `onDestroy` | `+page.svelte` (`createDraftEmail` `:1976`, subscribe block `:878`, `onDestroy` `:944`) | M |
| P3.6 | "AI draft" button (reading view / compose toolbar) → `fireAiDraftReply` + loading state; errors via `syncErrors` | `+page.svelte`, reading-view component, `settingsStore`/`mailStore` | M |
| P3.7 | Tests: reference SQL heuristics; prompt build; draft-insertion echo | tests | M |

**Notes.** Drafts are **frontend-only/ephemeral** today — a generated draft behaves like any other unsaved draft. DB persistence of drafts is deliberately out of scope (tracked in Open questions). The agentic loop's simple form is one `pick_references(ids)` call; the richer form is a multi-turn `search_mail(query)` tool loop — start simple.

**Phase DoD:** "AI draft" on an email yields a Drafts entry that opens in compose with relevant prior mail folded into context.

---

## Phase 4 — MCP server (external agent surface)

| ID | Task | Files | Size |
|----|------|-------|------|
| P4.1 | `agent_api_keys` table + migration; add `sha2` to `Cargo.toml` | `db.rs`, `src-tauri/Cargo.toml` | S |
| P4.2 | Key commands: `generate_agent_key(label)` (returns plaintext **once**, stores `sha256`), `list_agent_keys`, `revoke_agent_key(id)`; register | `lib.rs` | M |
| P4.3 | `agent_mcp.rs` — Axum server cloned from `dav_server.rs` (`start_server` `:67`); bind `127.0.0.1:<port>`; `McpServerHandle` in `AppState` (mirror `dav_handle` `:262`); `start_agent_server`/`stop_agent_server`/`get_agent_server_status` | new `agent_mcp.rs`; `lib.rs` AppState + `.manage()` `:5614` | L |
| P4.4 | Auth middleware: `Authorization: Bearer` → sha256 **constant-time** compare vs `agent_api_keys`; enforce loopback (adapt `authenticate` `dav_server.rs:169`) | `agent_mcp.rs` | M |
| P4.5 | MCP/JSON-RPC 2.0 over HTTP POST: `initialize`, `tools/list`, `tools/call`; tool registry (SSE streaming optional) | `agent_mcp.rs` | L |
| P4.6 | Tool handlers → service layer, **gated by `external_access=1` per account**: `mail.list_accounts`, `mail.search`/`get`/`send`/`move`/`delete` (P1), `mail.draft` (P3), `mail.classify` (P2.5) | `agent_mcp.rs` | M |
| P4.7 | Frontend "Agent API" panel: start/stop + port, generate/list/revoke keys (show once), surface per-account `externalAccess` (from P0.5) | `SettingsView.svelte`, `+page.svelte`, store | M |
| P4.8 | Tests: `tools/list`; `tools/call` with/without valid key; per-account gating; end-to-end `mail.send` reflected in UI via echoes | tests | M |

**Phase DoD:** an external client with a valid app key can list/search/send/draft/classify against **only** `external_access` accounts; the desktop UI reflects agent actions live; missing/invalid keys and disabled accounts are rejected/invisible.

---

## Cross-cutting (do alongside, not last)

| ID | Task | Size |
|----|------|------|
| X.1 | i18n strings for all new UI (follow `src/lib/i18n`) | S per phase |
| X.2 | Privacy/disclosure copy where content leaves the device; document local-endpoint (Ollama) setup in README | S |
| X.3 | Structured logging via `log::` for every LLM call (latency, token estimate, model) + redact content | S |
| X.4 | Error surfacing through the existing `syncErrors` / toast paths (don't swallow LLM failures) | S |
| X.5 | Update `agentic-ai-plan.md` if decisions change during build | — |

## Deferred (explicitly out of scope for v1)

- DB persistence of drafts (currently ephemeral).
- Embedding-based reference retrieval for large mailboxes.
- Streaming draft tokens into the composer.
- MCP **stdio** transport variant.
- Per-account-scoped app keys; rate limiting; token-budget dashboard.

---

### When implementation starts
Ask me to load a phase into the live task list (TaskCreate) and I'll track it task-by-task. I'd open with the **first slice** (P0.1 → P0.2 → P0.3) to get a working model ping before anything depends on it.
