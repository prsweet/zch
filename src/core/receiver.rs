use std::{path::PathBuf, sync::mpsc::Sender, thread};

use tiny_http::{Method, Response, Server};

use crate::core::{
    ActiveProblem, HelperCommand, RunAction,
    PassingCommand::{self, ToHelper},
};

/// Listener port, overridable with `ZCH_PORT` (default 10043).
pub fn listen_port() -> u16 {
    std::env::var("ZCH_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(10043)
}

pub fn spawn_server(tx: Sender<PassingCommand>) {
    thread::spawn(move || {
        let port = listen_port();
        let server = match Server::http(("127.0.0.1", port)) {
            Ok(server) => server,
            Err(e) => {
                let _ = tx.send(ToHelper(HelperCommand::Error(format!(
                    "listener failed to bind 127.0.0.1:{port} ({e}) — is another instance running?"
                ))));
                return;
            }
        };

        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);

            if request.method() == &Method::Post {
                match serde_json::from_str::<ActiveProblem>(&body) {
                    Ok(problem) => {
                        let _ = tx.send(ToHelper(HelperCommand::New(problem)));
                    }
                    Err(e) => {
                        let _ = tx.send(ToHelper(HelperCommand::Error(format!(
                            "malformed problem payload: {e}"
                        ))));
                    }
                }
            } else {
                handle_command_url(&url, &tx);
            }

            let _ = request.respond(Response::empty(200));
        }
    });
}

fn handle_command_url(url: &str, tx: &Sender<PassingCommand>) {
    let (path, query) = match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    };

    let mut action: Option<RunAction> = None;
    let mut file: Option<PathBuf> = None;

    match path {
        "/command" => {
            for (key, value) in parse_query(query) {
                match key.as_str() {
                    "action" => action = parse_action(&value),
                    "file"
                        if !value.is_empty() => {
                            file = Some(PathBuf::from(value));
                        }
                    _ => {}
                }
            }
        }
        "/cmd/r" => action = Some(RunAction::Run),
        "/cmd/a" => action = Some(RunAction::Add),
        "/cmd/e" => action = Some(RunAction::Edit),
        "/cmd/s" => action = Some(RunAction::Submit),
        _ => {}
    }

    if let Some(action) = action {
        let _ = tx.send(ToHelper(HelperCommand::Command { file, action }));
    }
}

fn parse_action(raw: &str) -> Option<RunAction> {
    match raw.split_whitespace().next()?.to_lowercase().as_str() {
        "r" | "run" => Some(RunAction::Run),
        "a" | "add" => Some(RunAction::Add),
        "e" | "edit" => Some(RunAction::Edit),
        "s" | "submit" => Some(RunAction::Submit),
        _ => None,
    }
}

fn parse_query(query: Option<&str>) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let Some(query) = query else { return params };
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            params.push((percent_decode(k), percent_decode(v)));
        }
    }
    params
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Percent-encode a query parameter value (file paths contain `/` and spaces).
pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encoded_query_values() {
        assert_eq!(percent_decode("runner"), "runner");
        assert_eq!(percent_decode("%2Ftmp%2Fa%20b.cpp"), "/tmp/a b.cpp");
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn encodes_url_values() {
        assert_eq!(url_encode("/tmp/a b.cpp"), "%2Ftmp%2Fa%20b.cpp");
        assert_eq!(url_encode("run"), "run");
    }

    #[test]
    fn parses_actions() {
        assert_eq!(parse_action("r"), Some(RunAction::Run));
        assert_eq!(parse_action("Run"), Some(RunAction::Run));
        assert_eq!(parse_action("cd cses"), None);
        assert_eq!(parse_action("submit"), Some(RunAction::Submit));
    }
}
