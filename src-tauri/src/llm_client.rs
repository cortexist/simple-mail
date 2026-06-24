//! llm_client.rs — Minimal OpenAI-compatible chat client (`/chat/completions`).
//!
//! Works with OpenAI, OpenRouter, Azure, and local servers (llama.cpp, Ollama,
//! vLLM) by changing `base_url`. To benefit from server-side prompt caching,
//! callers should keep any fixed system/few-shot block as a byte-identical
//! leading prefix and place variable content (e.g. the email) at the tail.
//!
//! Phase 0 exposes a concrete `HttpLlmClient`. A `LlmClient` trait + mock will
//! be introduced in Phase 2 alongside the classification unit tests.

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// One chat message. `role` is "system" | "user" | "assistant".
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage { role: "user".into(), content: content.into() }
    }
    #[allow(dead_code)]
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage { role: "assistant".into(), content: content.into() }
    }
}

/// Per-call options.
#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub model: String,
    pub max_tokens: i32,
    pub temperature: f64,
    /// When true, request a JSON-object response (`response_format`).
    pub json: bool,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: i32,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: String,
}

/// Abstraction over a chat-completion backend so callers (e.g. the
/// classification service) can be unit-tested against a mock without a live
/// endpoint. Object-safe: the method returns a boxed `Send` future rather than
/// using `async fn`, so `&dyn LlmClient` works and spawned tasks stay `Send`.
pub trait LlmClient: Send + Sync {
    fn complete<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        opts: &'a ChatOptions,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
}

impl LlmClient for HttpLlmClient {
    fn complete<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        opts: &'a ChatOptions,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(self.chat(messages, opts))
    }
}

/// OpenAI-compatible HTTP client.
pub struct HttpLlmClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl HttpLlmClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(HttpLlmClient {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        })
    }

    /// Send a chat completion and return the first choice's text content.
    pub async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String, String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = ChatRequest {
            model: &opts.model,
            messages,
            max_tokens: opts.max_tokens,
            temperature: opts.temperature,
            response_format: if opts.json {
                Some(ResponseFormat { kind: "json_object".into() })
            } else {
                None
            },
        };

        let mut req = self.http.post(&url).json(&body);
        // Local servers (llama.cpp/Ollama) need no key; only send when set.
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

        let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("failed to read response: {e}"))?;
        if !status.is_success() {
            return Err(format!("LLM API error {}: {}", status.as_u16(), text));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .map_err(|e| format!("failed to parse LLM response: {e} — body: {text}"))?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| "LLM response had no choices".to_string())
    }
}
