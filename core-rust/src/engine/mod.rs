//! The engine: single authority over the document.
//!
//! All writers (human UI, MCP agents, REST clients) go through `Engine::execute`, which is
//! called under one lock by the FFI layer — a single-writer command queue. Optimistic
//! concurrency comes from `expectedRevision`: a writer that saw an old revision is rejected
//! instead of overwriting newer work. `operationId` makes retries idempotent.

use crate::commands::json::Json;
use crate::commands::{self, CmdError, CmdResult, Kind, Outcome};
use crate::history::{History, LogEntry, now_ms};
use crate::project::{Document, validate_size};
use crate::storage;

pub const MAX_TRANSACTION_COMMANDS: usize = 10_000;

pub struct Response {
    pub json: String,
    pub binary: Vec<u8>,
}

pub struct Engine {
    doc: Document,
    history: History,
    revision: u64,
}

struct Envelope<'a> {
    op: &'a str,
    args: &'a Json,
    actor: String,
    operation_id: Option<String>,
    expected_revision: Option<u64>,
}

fn parse_envelope(req: &Json) -> CmdResult<Envelope<'_>> {
    if !matches!(req, Json::Obj(_)) {
        return Err(CmdError::new("invalid_request", "request must be a JSON object"));
    }
    let op = req.get("op").and_then(Json::as_str).ok_or_else(|| CmdError::new("invalid_request", "missing string field 'op'"))?;
    let args = match req.get("args") {
        Some(a @ Json::Obj(_)) => a,
        Some(Json::Null) | None => req,
        Some(_) => return Err(CmdError::new("invalid_request", "'args' must be an object")),
    };
    let actor = match req.get("actor") {
        Some(Json::Str(s)) if !s.trim().is_empty() => s.trim().chars().filter(|c| !c.is_control()).take(64).collect(),
        None | Some(Json::Null) => "unknown".to_string(),
        Some(_) => return Err(CmdError::new("invalid_request", "'actor' must be a non-empty string")),
    };
    let operation_id = match req.get("operationId") {
        Some(Json::Str(s)) if !s.is_empty() && s.len() <= 128 => Some(s.clone()),
        None | Some(Json::Null) => None,
        Some(_) => return Err(CmdError::new("invalid_request", "'operationId' must be a string of 1-128 characters")),
    };
    let expected_revision = match req.get("expectedRevision") {
        None | Some(Json::Null) => None,
        Some(v) => Some(
            v.as_i64()
                .filter(|r| *r >= 0)
                .ok_or_else(|| CmdError::new("invalid_request", "'expectedRevision' must be a non-negative integer"))? as u64,
        ),
    };
    Ok(Envelope { op, args, actor, operation_id, expected_revision })
}

impl Engine {
    pub fn new(width: u32, height: u32) -> Engine {
        Engine { doc: Document::new(width, height), history: History::default(), revision: 0 }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    fn error_json(&self, op: &str, e: &CmdError) -> String {
        Json::obj()
            .with("ok", false)
            .with("op", op)
            .with("revision", self.revision)
            .with("error", Json::obj().with("code", e.code).with("message", e.message.as_str()))
            .to_json_string()
    }

    /// Executes one request. Never panics on bad input; every failure is a JSON error.
    pub fn execute(&mut self, request: &[u8], payload: &[u8]) -> Response {
        let text = match std::str::from_utf8(request) {
            Ok(t) => t,
            Err(_) => return self.fail("", CmdError::new("invalid_request", "request is not valid UTF-8")),
        };
        let req = match Json::parse(text) {
            Ok(j) => j,
            Err(e) => return self.fail("", CmdError::new("invalid_request", format!("malformed JSON: {e}"))),
        };
        let env = match parse_envelope(&req) {
            Ok(e) => e,
            Err(e) => return self.fail(req.get("op").and_then(Json::as_str).unwrap_or(""), e),
        };
        let Some(kind) = commands::kind_of(env.op) else {
            return self.fail(env.op, CmdError::new("unknown_op", format!("unknown operation '{}'", env.op)));
        };

        if kind == Kind::Read {
            return match commands::run_query(&self.doc, &self.history, self.revision, env.op, env.args) {
                Ok(q) => Response {
                    json: Json::obj()
                        .with("ok", true)
                        .with("op", env.op)
                        .with("revision", self.revision)
                        .with("result", q.result)
                        .to_json_string(),
                    binary: q.binary,
                },
                Err(e) => self.fail(env.op, e),
            };
        }

        // Idempotent replay: the same operationId never applies twice.
        if let Some(id) = &env.operation_id
            && let Some(cached) = self.history.cached_response(id)
        {
            let mut j = Json::parse(std::str::from_utf8(cached).unwrap_or("{}")).unwrap_or(Json::obj());
            j.set("replayed", true);
            return Response { json: j.to_json_string(), binary: Vec::new() };
        }
        if let Some(expected) = env.expected_revision
            && expected != self.revision
        {
            return self.fail(
                env.op,
                CmdError::new(
                    "conflict",
                    format!("expected revision {expected} but the canvas is at revision {}; re-read the canvas and retry", self.revision),
                ),
            );
        }

        let before = self.doc.clone();
        let outcome = match kind {
            Kind::Special => self.apply_special(env.op, env.args, payload),
            _ => commands::apply_mutation(&mut self.doc, env.op, env.args),
        };
        match outcome {
            Err(e) => {
                self.doc = before;
                self.fail(env.op, e)
            }
            Ok(outcome) => {
                self.revision += 1;
                if !matches!(env.op, "history_undo" | "history_redo") {
                    self.history.record(before, self.revision, env.op, &env.actor);
                }
                self.history.log(LogEntry {
                    revision: self.revision,
                    actor: env.actor.clone(),
                    op: env.op.to_string(),
                    operation_id: env.operation_id.clone(),
                    changed_pixels: outcome.changed,
                    timestamp_ms: now_ms(),
                });
                let json = Json::obj()
                    .with("ok", true)
                    .with("op", env.op)
                    .with("revision", self.revision)
                    .with("actor", env.actor.as_str())
                    .with("result", outcome.result)
                    .to_json_string();
                if let Some(id) = &env.operation_id {
                    self.history.cache_response(id, json.clone().into_bytes());
                }
                Response { json, binary: Vec::new() }
            }
        }
    }

    fn fail(&self, op: &str, e: CmdError) -> Response {
        Response { json: self.error_json(op, &e), binary: Vec::new() }
    }

    fn apply_special(&mut self, op: &str, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
        match op {
            "history_undo" | "history_redo" => {
                let steps = commands_args_steps(a)?;
                let mut done = Vec::new();
                for _ in 0..steps {
                    let info = if op == "history_undo" {
                        self.history.undo(&mut self.doc, self.revision)
                    } else {
                        self.history.redo(&mut self.doc, self.revision)
                    };
                    match info {
                        Some(i) => done.push(Json::obj().with("op", i.label).with("actor", i.actor)),
                        None => break,
                    }
                }
                if done.is_empty() {
                    return Err(CmdError::new("nothing_to_do", if op == "history_undo" { "nothing to undo" } else { "nothing to redo" }));
                }
                Ok(Outcome::new(Json::obj().with("steps", done.len()).with("operations", Json::Arr(done)), 0))
            }
            "project_new" => {
                let w = a.get("width").and_then(Json::as_i64).unwrap_or(32);
                let h = a.get("height").and_then(Json::as_i64).unwrap_or(32);
                let (w, h) = validate_size(w, h)?;
                let mut doc = Document::new(w, h);
                if let Some(name) = a.get("name").and_then(Json::as_str).map(str::trim).filter(|n| !n.is_empty()) {
                    doc.name = name.chars().take(256).collect();
                }
                self.doc = doc;
                Ok(Outcome::new(Json::obj().with("width", w).with("height", h).with("name", self.doc.name.as_str()), 0))
            }
            "project_load" => {
                let loaded = storage::load(payload).map_err(|e| CmdError::new("invalid_file", e))?;
                self.doc = loaded.doc;
                let max_rev = loaded.log.iter().map(|e| e.revision).max().unwrap_or(0);
                self.history.replace_log(loaded.log);
                // Keep revisions monotonic so stale writers are still rejected after a load.
                self.revision = self.revision.max(max_rev);
                Ok(Outcome::new(
                    Json::obj()
                        .with("name", self.doc.name.as_str())
                        .with("width", self.doc.width)
                        .with("height", self.doc.height)
                        .with("frames", self.doc.frames.len()),
                    0,
                ))
            }
            "transaction" => {
                let cmds = a.get("commands").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'commands' must be an array"))?;
                if cmds.is_empty() || cmds.len() > MAX_TRANSACTION_COMMANDS {
                    return Err(CmdError::new("limit", format!("a transaction holds 1-{MAX_TRANSACTION_COMMANDS} commands")));
                }
                let mut results = Vec::with_capacity(cmds.len());
                let mut changed = 0;
                for (i, c) in cmds.iter().enumerate() {
                    let op =
                        c.get("op").and_then(Json::as_str).ok_or_else(|| CmdError::invalid(format!("commands[{i}] is missing 'op'")))?;
                    if commands::kind_of(op) != Some(Kind::Mutate) {
                        return Err(CmdError::invalid(format!("commands[{i}]: '{op}' cannot be used inside a transaction")));
                    }
                    let args = match c.get("args") {
                        Some(a @ Json::Obj(_)) => a,
                        _ => c,
                    };
                    let o = commands::apply_mutation(&mut self.doc, op, args).map_err(|e| {
                        CmdError::new(e.code, format!("commands[{i}] ({op}) failed, transaction rolled back: {}", e.message))
                    })?;
                    changed += o.changed;
                    results.push(o.result);
                }
                Ok(Outcome::new(
                    Json::obj().with("applied", results.len()).with("changedPixels", changed).with("results", Json::Arr(results)),
                    changed,
                ))
            }
            _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
        }
    }
}

fn commands_args_steps(a: &Json) -> CmdResult<usize> {
    match a.get("steps").filter(|v| !v.is_null()) {
        None => Ok(1),
        Some(v) => {
            v.as_i64().filter(|s| (1..=256).contains(s)).map(|s| s as usize).ok_or_else(|| CmdError::invalid("'steps' must be 1-256"))
        }
    }
}

#[cfg(test)]
mod tests;
