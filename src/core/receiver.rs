use std::{
    io::Read,
    path::PathBuf,
    sync::mpsc::{Sender, channel},
    thread,
    time::Duration,
};

use tiny_http::{Method, Response, Server};

use crate::core::{
    ActiveProblem, HelperCommand,
    PassingCommand::{self, ToHelper},
    RunAction,
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
/// Competitive Companion payloads are a few KB; huge samples stay far below.
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

fn header(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// Web pages can reach 127.0.0.1 too. Problems may only come from a browser
/// extension (or a non-browser client), and commands only from `zch send`,
/// which sends neither header.
fn refusal(is_post: bool, origin: Option<&str>, fetch_site: Option<&str>) -> Option<&'static str> {
    if is_post {
        let from_web_page = origin.is_some_and(|origin| {
            let origin = origin.trim().to_ascii_lowercase();
            origin == "null"
                || origin.starts_with("http://")
                || origin.starts_with("https://")
                || origin.starts_with("file://")
        });
        from_web_page.then_some("problems are only accepted from Competitive Companion")
    } else {
        (origin.is_some() || fetch_site.is_some())
            .then_some("commands are only accepted from `zch send`")
    }
}

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
                let _ = tx.send(ToHelper(HelperCommand::ListenerDown(format!(
                    "cannot listen on 127.0.0.1:{port} — is another zch running? ({e})"
                ))));
                return;
            }
        };

        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let origin = header(&request, "Origin");
            let fetch_site = header(&request, "Sec-Fetch-Site");
            let is_post = request.method() == &Method::Post;

            if let Some(reason) = refusal(is_post, origin.as_deref(), fetch_site.as_deref()) {
                let _ = request.respond(Response::from_string(reason).with_status_code(403));
                continue;
            }

            let mut body = Vec::new();
            let read = request
                .as_reader()
                .take(MAX_BODY_BYTES + 1)
                .read_to_end(&mut body);
            if read.is_err() || body.len() as u64 > MAX_BODY_BYTES {
                let _ = request
                    .respond(Response::from_string("payload too large").with_status_code(413));
                continue;
            }

            let (status, message) = if is_post {
                match serde_json::from_slice::<ActiveProblem>(&body) {
                    Ok(mut problem) => {
                        // A payload must never choose a program to execute.
                        problem.checker = None;
                        let _ = tx.send(ToHelper(HelperCommand::New(problem)));
                        (200, "problem received".to_string())
                    }
                    Err(e) => {
                        let message = format!("malformed problem payload: {e}");
                        let _ = tx.send(ToHelper(HelperCommand::Error(message.clone())));
                        (400, message)
                    }
                }
            } else {
                match handle_command_url(&url) {
                    Some((action, file)) => {
                        let (reply_tx, reply_rx) = channel();
                        let _ = tx.send(ToHelper(HelperCommand::Command {
                            file,
                            action,
                            reply: reply_tx,
                        }));
                        match reply_rx.recv_timeout(COMMAND_TIMEOUT) {
                            Ok(Ok(message)) => (200, message),
                            Ok(Err(error)) => (400, error),
                            Err(_) => (504, "the TUI did not answer in time".to_string()),
                        }
                    }
                    None => (400, "unknown command".to_string()),
                }
            };

            let _ = request.respond(Response::from_string(message).with_status_code(status));
        }
    });
}

fn handle_command_url(url: &str) -> Option<(RunAction, Option<PathBuf>)> {
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
                    "file" if !value.is_empty() => {
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

    action.map(|action| (action, file))
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
            // Work on bytes: slicing the `str` here would panic when a
            // multi-byte character follows the `%`.
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(high << 4 | low);
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

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|digit| digit as u8)
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
    fn malformed_escapes_never_panic() {
        assert_eq!(percent_decode("%aé"), "%aé");
        assert_eq!(percent_decode("%é"), "%é");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn web_pages_cannot_drive_the_listener() {
        // Competitive Companion posts from an extension origin (or none).
        assert_eq!(refusal(true, None, None), None);
        assert_eq!(refusal(true, Some("chrome-extension://abc"), None), None);
        assert_eq!(
            refusal(true, Some("moz-extension://abc"), Some("none")),
            None
        );
        assert!(refusal(true, Some("https://evil.example"), None).is_some());
        assert!(refusal(true, Some("null"), None).is_some());

        // `zch send` sends neither header; any browser request does.
        assert_eq!(refusal(false, None, None), None);
        assert!(refusal(false, None, Some("cross-site")).is_some());
        assert!(refusal(false, Some("https://evil.example"), None).is_some());
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
