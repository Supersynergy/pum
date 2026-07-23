//! Minimal, dependency-free MCP stdio transport for PUM.
//!
//! stdout is reserved exclusively for newline-delimited JSON-RPC. Human
//! diagnostics belong on stderr so any MCP client can parse every response.

use std::io::{self, BufRead, BufReader, Write};

use anyhow::Result;
use serde_json::{Value, json};

use crate::adapters::{Adapter, all_adapters, get_adapter, live_adapters};

const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve() -> Result<()> {
    eprintln!("pum mcp: stdio server started; package changes remain explicit CLI actions");
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_io(BufReader::new(stdin.lock()), stdout.lock())
}

fn serve_io<R: BufRead, W: Write>(reader: R, mut writer: W) -> Result<()> {
    let mut initialized = false;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_message(request, &mut initialized),
            Err(_) => Some(error_response(Value::Null, -32700, "parse error")),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
    Ok(())
}

fn handle_message(message: Value, initialized: &mut bool) -> Option<Value> {
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return id.map(|id| error_response(id, -32600, "invalid request"));
    };

    let response = match method {
        "initialize" => {
            *initialized = true;
            Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "pum",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "PUM means Package Update Manager. It reports local package freshness and safe update plans; package mutations require an explicit terminal command."
            }))
        }
        "notifications/initialized" => return None,
        "ping" => Ok(json!({})),
        "tools/list" if *initialized => Ok(json!({ "tools": tools() })),
        "tools/call" if *initialized => {
            call_tool(message.get("params").cloned().unwrap_or_default())
        }
        "tools/list" | "tools/call" => Err((
            -32002,
            "initialize must complete before calling PUM tools".to_string(),
        )),
        _ => Err((-32601, format!("method not found: {method}"))),
    };

    match (id, response) {
        (Some(id), Ok(result)) => Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
        (Some(id), Err((code, message))) => Some(error_response(id, code, &message)),
        (None, _) => None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "pum_status",
            "description": "Read PUM's local DuckDB freshness ledger: inventory size, known newer candidates, source coverage, and whether the last refresh is stale. Does not contact package sources or modify packages.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false }
        }),
        json!({
            "name": "pum_refresh",
            "description": "Run PUM's read-only inventory and native-source checks, then append a local DuckDB snapshot. It never installs, upgrades, removes, or updates the operating system.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": true }
        }),
        json!({
            "name": "pum_update_plan",
            "description": "Generate the exact manager commands PUM would propose for updates. This is a dry plan only; it never executes a package command.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manager": {
                        "type": "string",
                        "description": "Optional PUM adapter name, such as brew, npm, cargo, uv, or rustup. Omit for every live adapter."
                    }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false }
        }),
        json!({
            "name": "pum_doctor",
            "description": "Show which of PUM's package-manager adapters are available on PATH and their resolved executable paths. Does not change the machine.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false }
        }),
    ]
}

fn call_tool(params: Value) -> std::result::Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
        (
            -32602,
            "tools/call requires a string params.name".to_string(),
        )
    })?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        return Err((
            -32602,
            "tools/call params.arguments must be an object".to_string(),
        ));
    }

    let result = match name {
        "pum_status" => {
            reject_arguments(&arguments, &[])?;
            let conn = crate::db_connect().map_err(tool_error)?;
            serde_json::to_value(crate::load_status(&conn).map_err(tool_error)?)
                .map_err(tool_error)?
        }
        "pum_refresh" => {
            reject_arguments(&arguments, &[])?;
            serde_json::to_value(crate::refresh_inventory().map_err(tool_error)?)
                .map_err(tool_error)?
        }
        "pum_update_plan" => update_plan(&arguments)?,
        "pum_doctor" => {
            reject_arguments(&arguments, &[])?;
            let adapters: Vec<Value> = all_adapters()
                .into_iter()
                .map(|adapter| {
                    let path = which::which(adapter.binary())
                        .ok()
                        .map(|path| path.display().to_string());
                    json!({
                        "manager": adapter.name(),
                        "binary": adapter.binary(),
                        "available": adapter.detect(),
                        "path": path,
                    })
                })
                .collect();
            json!({
                "package": "PUM — Package Update Manager",
                "adapters": adapters,
                "os_updates_supported": false,
            })
        }
        _ => return Err((-32602, format!("unknown PUM tool: {name}"))),
    };

    let text = serde_json::to_string_pretty(&result).map_err(tool_error)?;
    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

fn update_plan(arguments: &Value) -> std::result::Result<Value, (i64, String)> {
    reject_arguments(arguments, &["manager"])?;
    if arguments
        .get("manager")
        .is_some_and(|manager| !manager.is_string())
    {
        return Err((
            -32602,
            "pum_update_plan.manager must be a string".to_string(),
        ));
    }
    let manager = arguments.get("manager").and_then(Value::as_str);
    let adapters: Vec<Box<dyn Adapter>> = match manager {
        Some(name) => match get_adapter(name) {
            Some(adapter) if adapter.detect() => vec![adapter],
            Some(_) => {
                return Err((
                    -32602,
                    format!("PUM adapter is not available on PATH: {name}"),
                ));
            }
            None => return Err((-32602, format!("unknown PUM adapter: {name}"))),
        },
        None => live_adapters(),
    };
    let plan: Vec<Value> = adapters
        .iter()
        .map(|adapter| {
            json!({
                "manager": adapter.name(),
                "command": adapter.upgrade_cmd(None),
                "report_only": adapter.report_only(),
                "requires_apply_flag": adapter.report_only(),
            })
        })
        .collect();
    Ok(json!({
        "dry_run": true,
        "package_changes_executed": false,
        "plan": plan,
        "next_step": "Review the exact command, then run pum update --dry-run --all or an explicit pum update command in a terminal."
    }))
}

fn reject_arguments(arguments: &Value, allowed: &[&str]) -> std::result::Result<(), (i64, String)> {
    let unknown: Vec<&str> = arguments
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, _)| key.as_str())
        .filter(|key| !allowed.contains(key))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err((
            -32602,
            format!("unsupported PUM tool argument(s): {}", unknown.join(", ")),
        ))
    }
}

fn tool_error(error: impl std::fmt::Display) -> (i64, String) {
    (-32000, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_then_lists_safe_tools() {
        let mut initialized = false;
        let init = handle_message(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            &mut initialized,
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "pum");

        let list = handle_message(
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            &mut initialized,
        )
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            ["pum_status", "pum_refresh", "pum_update_plan", "pum_doctor"]
        );
    }

    #[test]
    fn tools_require_initialize() {
        let mut initialized = false;
        let response = handle_message(
            json!({ "jsonrpc": "2.0", "id": "before", "method": "tools/list" }),
            &mut initialized,
        )
        .unwrap();
        assert_eq!(response["error"]["code"], -32002);
    }

    #[test]
    fn notifications_do_not_write_stdout() {
        let mut initialized = true;
        assert!(
            handle_message(
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                &mut initialized,
            )
            .is_none()
        );
    }

    #[test]
    fn update_plan_rejects_a_non_string_manager() {
        let error = update_plan(&json!({ "manager": 7 })).unwrap_err();
        assert_eq!(error.0, -32602);
        assert!(error.1.contains("must be a string"));
    }
}
