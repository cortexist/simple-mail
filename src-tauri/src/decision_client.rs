//! decision_client.rs — Client for System One decision models over TypeSafe
//! Jev's `POST {base_url}/systemone` protocol.
//!
//! One wire format serves TypeSafe Jev (`https://api.typesafe.ai/v1`), Liquid
//! D1 and a local Laya server (`http://127.0.0.1:8000/v1`): a `state` (any
//! JSON), a `model` id, and a map of typed questions answered in one forward
//! pass with calibrated probabilities. Only the answer fields this app consumes
//! are parsed; the rest of a response is ignored.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// One typed question. `criteria` is optional for `noul`, a map of option →
/// description for `choice`, and an ordered array of levels for `score`.
#[derive(Debug, Clone, Serialize)]
pub struct Question {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<serde_json::Value>,
}

impl Question {
    /// A yes/no question; the answer is the probability of yes.
    pub fn noul(instructions: impl Into<String>, yes: &str, no: &str) -> Self {
        Question {
            kind: "noul",
            instructions: instructions.into(),
            criteria: Some(serde_json::json!({ "true": yes, "false": no })),
        }
    }
}

#[derive(Serialize)]
struct Request<'a, S: Serialize> {
    state: &'a S,
    model: &'a str,
    questions: &'a HashMap<String, Question>,
}

/// One answer. Only `noul` is read today; `choice` is kept for callers that
/// pick among options.
#[derive(Debug, Clone, Deserialize)]
pub struct Answer {
    #[serde(default)]
    pub noul: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub choice: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    answers: HashMap<String, Answer>,
}

pub struct DecisionClient {
    http: reqwest::Client,
    url: String,
    api_key: String,
    model: String,
}

/// The `/systemone` endpoint for a configured URL. Accepts the API root
/// (`https://api.typesafe.ai/v1`) or the full endpoint as copied from the docs
/// (`…/v1/systemone`), so pasting either works.
fn endpoint_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let root = base.strip_suffix("/systemone").unwrap_or(base);
    format!("{root}/systemone")
}

impl DecisionClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            // A decision is one forward pass; a slow reply means the server is
            // down or overloaded, and the caller has a fallback.
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(DecisionClient {
            http,
            url: endpoint_url(base_url),
            api_key: api_key.to_string(),
            model: model.to_string(),
        })
    }

    /// Ask every question about `state` in one request.
    ///
    /// `state` serializes in field order, and order matters: a small local
    /// model (Laya) truncates the state from the right to fit its context, so
    /// decisive fields must come first. Pass a struct, not a `json!` object —
    /// without serde_json's `preserve_order` feature the latter is sorted
    /// alphabetically.
    pub async fn ask<S: Serialize + Sync>(
        &self,
        state: &S,
        questions: &HashMap<String, Question>,
    ) -> Result<HashMap<String, Answer>, String> {
        let body = Request { state, model: &self.model, questions };
        let mut req = self.http.post(&self.url).json(&body);
        // A local Laya server needs no key; only send one when set.
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await.map_err(|e| format!("decision request failed: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("failed to read decision response: {e}"))?;
        if !status.is_success() {
            return Err(format!("decision API error {}: {}", status.as_u16(), text));
        }
        let parsed: Response = serde_json::from_str(&text)
            .map_err(|e| format!("failed to parse decision response: {e} — body: {text}"))?;
        Ok(parsed.answers)
    }

    /// Ask, then read each question's yes-probability. A question the server
    /// left unanswered is an error: the caller must not act on a guess.
    pub async fn ask_nouls<S: Serialize + Sync>(
        &self,
        state: &S,
        questions: &HashMap<String, Question>,
    ) -> Result<HashMap<String, f64>, String> {
        let answers = self.ask(state, questions).await?;
        let mut out = HashMap::with_capacity(questions.len());
        for id in questions.keys() {
            let p = answers
                .get(id)
                .and_then(|a| a.noul)
                .ok_or_else(|| format!("decision response has no noul answer for '{id}'"))?;
            out.insert(id.clone(), p);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_matches_the_jev_wire_shape() {
        let mut qs = HashMap::new();
        qs.insert("spam".to_string(), Question::noul("Is this spam?", "spam", "not spam"));
        let state = serde_json::json!({ "subject": "hi" });
        let body = serde_json::to_value(Request { state: &state, model: "jev-latest", questions: &qs }).unwrap();
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["state"]["subject"], "hi");
        assert_eq!(body["questions"]["spam"]["type"], "noul");
        assert_eq!(body["questions"]["spam"]["criteria"]["true"], "spam");
    }

    #[test]
    fn endpoint_accepts_root_or_full_url() {
        for input in [
            "https://api.typesafe.ai/v1",
            "https://api.typesafe.ai/v1/",
            "https://api.typesafe.ai/v1/systemone",
            " https://api.typesafe.ai/v1/systemone/ ",
        ] {
            assert_eq!(endpoint_url(input), "https://api.typesafe.ai/v1/systemone", "{input}");
        }
    }

    #[test]
    fn response_parses_noul_and_ignores_extras() {
        let raw = r#"{"model":"jev-1.13.0","answers":{"spam":{"type":"noul","noul":0.93}},"usage":{"input_tokens":3,"output_tokens":1}}"#;
        let r: Response = serde_json::from_str(raw).unwrap();
        assert_eq!(r.answers["spam"].noul, Some(0.93));
    }
}
