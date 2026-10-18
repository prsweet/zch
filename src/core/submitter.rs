use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{Sender, channel},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

use crate::{
    core::{
        HelperCommand,
        PassingCommand::{self, ToHelper},
        SubmitConfig, SubmitterCommand,
    },
    error::AppError,
};

const APP_NAME: &str = "Safari";

pub fn spawn_submitter(main_tx: Sender<PassingCommand>) -> Sender<SubmitterCommand> {
    let (tx, rx) = channel::<SubmitterCommand>();
    thread::spawn(move || {
        while let Ok(SubmitterCommand::SubmitCode { id, config }) = rx.recv() {
            // Each submission runs on its own thread: they progress in parallel
            // instead of waiting behind each other in a queue.
            let main_tx = main_tx.clone();
            thread::spawn(move || {
                let tx = main_tx.clone();
                let result = match submit(&config, move |msg| {
                    let _ = tx.send(ToHelper(HelperCommand::SubmitStatus { id, message: msg }));
                }) {
                    Ok(result) if result.is_empty() => {
                        "ERROR: no response from Safari (tab closed or browser crashed)".to_string()
                    }
                    Ok(result) => result,
                    Err(e) => format!("ERROR: {e}"),
                };
                let _ = main_tx.send(ToHelper(HelperCommand::SubmitFinished { id, result }));
            });
        }
    });
    tx
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected,
    Failed,
}

/// Judge verdict of the final `submitter::submit` string. Shared by the TUI
/// submission bar and the `submit` CLI so both color results identically.
pub fn classify(result: &str) -> Outcome {
    let lower = result.to_lowercase();
    if lower.contains("accepted") || lower.contains(": ac") {
        Outcome::Accepted
    } else if result.starts_with("RESULT:") || result.contains("REJECTED") {
        Outcome::Rejected
    } else {
        Outcome::Failed
    }
}

// ---------------------------------------------------------------------------
// Submission details (shared by the TUI confirmation and the CLI)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SubmissionInfo {
    pub platform: &'static str,
    pub submit_url: String,
    pub problem_code: String,
    pub language: &'static str,
    pub language_id: &'static str,
    pub file_name: String,
    pub browser: &'static str,
    pub safari_available: bool,
}

pub fn describe(config: &SubmitConfig) -> Result<SubmissionInfo, AppError> {
    plan(config).map(|(info, _)| info)
}

fn plan(config: &SubmitConfig) -> Result<(SubmissionInfo, Platform), AppError> {
    let code = fs::read_to_string(&config.file_path)
        .map_err(|e| AppError::Io(format!("cannot read {}: {e}", config.file_path.display())))?;

    let url = if config.url.is_empty() {
        url_from_file(&code).ok_or_else(|| {
            AppError::ParseError(
                "no problem URL found (receive the problem with Competitive Companion first)".to_string(),
            )
        })?
    } else {
        config.url.clone()
    };

    let platform = detect_platform(&url).ok_or(AppError::UnsupportedPlatform(url))?;
    let language = config.language;

    let info = SubmissionInfo {
        platform: "Codeforces",
        submit_url: platform.submit_url.clone(),
        problem_code: platform.problem_code.clone(),
        language: language.label(),
        language_id: language.cf_id(),
        file_name: config
            .file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        browser: APP_NAME,
        safari_available: safari_installed(),
    };

    Ok((info, platform))
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

struct Platform {
    submit_url: String,
    problem_code: String,
    ready_selector: &'static str,
}

fn detect_platform(url: &str) -> Option<Platform> {
    let (scheme, rest) = url.split_once("://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let origin = format!("{scheme}://{host}");

    if host.ends_with("codeforces.com") {
        // Contest / gym (also inside group URLs): .../contest/<id>/problem/<code>
        for i in 0..segments.len() {
            let is_contest = segments[i] == "contest" || segments[i] == "gym";
            if is_contest
                && segments.get(i + 2) == Some(&"problem")
                && segments.get(i + 3).is_some()
                && segments.get(i + 1).is_some_and(|id| !id.is_empty())
            {
                let prefix = segments[..=i + 1].join("/");
                return Some(Platform {
                    submit_url: format!("{origin}/{prefix}/submit"),
                    problem_code: segments[i + 3].to_string(),
                    ready_selector: "select[name=\"programTypeId\"]",
                });
            }
        }
        // Problemset: /problemset/problem/<id>/<index>
        if let Some(i) = segments.iter().position(|s| *s == "problemset")
            && segments.get(i + 1) == Some(&"problem")
                && let (Some(id), Some(index)) = (segments.get(i + 2), segments.get(i + 3))
            {
                return Some(Platform {
                    submit_url: format!("{origin}/problemset/submit"),
                    problem_code: format!("{id}{index}"),
                    ready_selector: "select[name=\"programTypeId\"]",
                });
            }
        return None;
    }

    None
}

// ---------------------------------------------------------------------------
// JavaScript injected into the page
// ---------------------------------------------------------------------------

const CF_FILL_JS: &str = r#"(function() {
    try {
        var code = atob('__CODE_B64__');

        // Problem selection: text input (problemset) or dropdown (contest)
        var probInput = document.querySelector('input[name="submittedProblemCode"]');
        if (probInput) {
            probInput.value = '__PROBLEM_CODE__';
            probInput.dispatchEvent(new Event('input', {bubbles: true}));
        }
        var probSelect = document.querySelector('select[name="submittedProblemIndex"]');
        if (probSelect) {
            var target = '__PROBLEM_CODE__'.toUpperCase();
            var found = false;
            for (var i = 0; i < probSelect.options.length; i++) {
                var val = probSelect.options[i].value.toUpperCase();
                var txt = probSelect.options[i].text.toUpperCase().trim();
                if (val === target || txt.indexOf(target + " -") === 0 || txt.indexOf(target + ".") === 0 || txt === target) {
                    probSelect.value = probSelect.options[i].value;
                    found = true; break;
                }
            }
            if (found) probSelect.dispatchEvent(new Event('change', {bubbles: true}));
        }

        // Code injection
        var ta = document.getElementById('sourceCodeTextarea');
        if (ta) ta.value = code;
        try { var ed = document.querySelector('.ace_editor'); if (ed && typeof ace !== 'undefined') ace.edit(ed).setValue(code, -1); } catch(e) {}

        // Language selection
        var sel = document.querySelector('select[name="programTypeId"]');
        if (sel) {
            for (var i = 0; i < sel.options.length; i++) {
                if (sel.options[i].value === '__LANG_ID__') { sel.value = '__LANG_ID__'; break; }
            }
            sel.dispatchEvent(new Event('change', {bubbles: true}));
        }

        // Submit
        var form = document.getElementById('submitForm') || document.querySelector('form.submit-form');
        if (!form) {
            var forms = document.querySelectorAll('form');
            for (var i = 0; i < forms.length; i++) {
                if ((forms[i].getAttribute('action') || '').indexOf('submit') > -1) { form = forms[i]; break; }
            }
        }
        if (form) {
            setTimeout(function() {
                var btn = form.querySelector('input[type="submit"], button[type="submit"], .submit');
                if (btn) btn.click();
                else form.submit();
            }, 800);
            return 'SUBMITTED';
        }
        return 'ERROR: Form not found.';
    } catch(e) { return 'ERROR: ' + e.message; }
})();"#;

const CF_RESULT_JS: &str = r#"(function() {
    try {
        var url = document.location.href;
        var hasCap = false;

        if (document.title.indexOf('Just a moment') > -1 || document.title.indexOf('Attention Required') > -1) hasCap = true;
        var eSub = document.querySelector('.error.for__submittedProblemCode, .error.for__source');
        if (eSub && eSub.textContent.toLowerCase().indexOf('captcha') > -1) hasCap = true;

        if (url.indexOf('/submit') > -1) {
            var ws = document.querySelectorAll('.cf-turnstile, #turnstile-wrapper, .g-recaptcha');
            for (var i = 0; i < ws.length; i++) { if (ws[i].innerHTML.trim() !== '') hasCap = true; }
            if (document.querySelector('iframe[src*="captcha"], iframe[src*="challenge"], iframe[src*="turnstile"]')) hasCap = true;
        }

        if (hasCap) return 'CAPTCHA';

        if (document.readyState !== 'complete') return 'WAIT';

        if (eSub) return 'REJECTED: ' + eSub.textContent.trim();
        var rows = document.querySelectorAll('tr[data-submission-id]');
        if (rows.length > 0) {
            var cell = rows[0].querySelector('td.status-verdict-cell, td:nth-child(6), .submissionVerdictWrapper, .verdict-waiting, .verdict-accepted, .verdict-rejected');
            var vt = cell ? cell.textContent.replace(/\s+/g, ' ').trim() : '';
            if (!vt || vt.indexOf('Running') > -1 || vt.indexOf('queue') > -1 || vt.indexOf('Judging') > -1 || vt.indexOf('testing') > -1) {
                return 'RELOAD: ' + (vt || 'In queue');
            }
            return 'RESULT: ' + vt;
        }
        if (url.indexOf('/my') > -1 || url.indexOf('/status') > -1) return 'RELOAD: Waiting for submission';
        if (url.indexOf('/submit') > -1) return 'WAIT';
        return 'WAIT';
    } catch(e) { return 'WAIT'; }
})();"#;

// ---------------------------------------------------------------------------
// AppleScript engine (Safari only — silent background submissions)
// ---------------------------------------------------------------------------

const APPLESCRIPT_TEMPLATE: &str = r#"tell application "System Events" to set frontAppName to name of first application process whose frontmost is true
tell application "__APP__"
    if (count of windows) is 0 then make new document with properties {URL:"about:blank"}
    tell window 1 to set submitTab to make new tab with properties {URL:"__SUBMIT_URL__#__TAG__"}
    set tabLostPolls to 0
    set captchaAlerted to false
    repeat 120 times
        delay 2
        set foundTab to my findSubmitTab(submitTab, "__TAG__", "__CONTEST_PREFIX__", "__RESULTS_MARK__")
        if foundTab is missing value then
            set tabLostPolls to tabLostPolls + 1
            if tabLostPolls is greater than 9 then return "INTERRUPTED: __APP__ tab was closed"
        else
            set tabLostPolls to 0
            set submitTab to foundTab
            try
                set pageCheck to do JavaScript "(function(){
                    var isC = false;
                    if(document.title.indexOf('Just a moment')>-1 || document.title.indexOf('Attention Required')>-1) isC=true;
                    if(isC) return 'CAPTCHA';
                    if(document.querySelector('__READY_SELECTOR__')) return 'READY';
                    return 'WAITING';
                })()" in submitTab
                if pageCheck is "READY" then exit repeat
                if pageCheck is "CAPTCHA" then
                    if captchaAlerted is false then
                        set captchaAlerted to true
                        my alertCaptcha(submitTab)
                    end if
                end if
            on error errMsg
                if errMsg contains "not allowed" then return "ERROR: Enable 'Allow JavaScript from Apple Events' in __APP__"
            end try
        end if
    end repeat
    delay 0.5
    set fillJS to read POSIX file "__FILL_JS_PATH__"
    set fillOK to false
    repeat 3 times
        try
            set submitResult to do JavaScript fillJS in submitTab
            set fillOK to true
            exit repeat
        on error errMsg
            if errMsg contains "not allowed" then return "ERROR: Enable 'Allow JavaScript from Apple Events' in __APP__"
            delay 2
        end try
    end repeat
    if fillOK is false then return "INTERRUPTED: __APP__ tab was closed"
    if submitResult does not start with "SUBMITTED" then return submitResult
    delay 4
    repeat 5 times
        set curURL to ""
        try
            set curURL to URL of submitTab
        end try
        if curURL contains "__RESULTS_MARK__" then exit repeat
        set pageMark to ""
        try
            set pageMark to do JavaScript "(function(){ if (document.readyState !== 'complete') return 'PENDING'; return document.querySelector('.alert-success, .alert-danger, .notice') ? 'DONE' : 'QUIET'; })()" in submitTab
        end try
        if pageMark is "DONE" then
            set URL of submitTab to "__RESULTS_URL__"
            delay 2
            exit repeat
        end if
        delay 2
    end repeat
    log "submitted — waiting for the judge's verdict"
    set resultJS to read POSIX file "__RESULT_JS_PATH__"
    set resultInfo to "UNKNOWN: Timed out"
    set resCaptchaAlerted to false
    repeat 120 times
        set foundTab to my findSubmitTab(submitTab, "__TAG__", "__CONTEST_PREFIX__", "__RESULTS_MARK__")
        if foundTab is missing value then
            set tabLostPolls to tabLostPolls + 1
            if tabLostPolls is greater than 9 then return "INTERRUPTED: __APP__ tab was closed"
            delay 2
        else
            set tabLostPolls to 0
            set submitTab to foundTab
            try
                set resultInfo to do JavaScript resultJS in submitTab
            on error
                set resultInfo to "WAIT"
            end try

            if resultInfo is "CAPTCHA" then
                if resCaptchaAlerted is false then
                    set resCaptchaAlerted to true
                    my alertCaptcha(submitTab)
                end if
                log "CAPTCHA: Waiting for you to solve and resubmit..."
            else
                log resultInfo
            end if

            if resultInfo starts with "RESULT:" or resultInfo starts with "REJECTED:" then exit repeat
            if resultInfo starts with "RELOAD:" then
                try
                    do JavaScript "window.location.reload()" in submitTab
                on error
                    set resultInfo to "WAIT"
                end try
                delay 1
                repeat 30 times
                    delay 1
                    try
                        if (do JavaScript "document.readyState" in submitTab) is "complete" then exit repeat
                    end try
                end repeat
            else
                delay 2
            end if
        end if
    end repeat
    if resultInfo does not start with "RESULT:" and resultInfo does not start with "REJECTED:" then
        set resultInfo to "TIMED OUT: the judge did not answer (last state: " & resultInfo & ")"
    end if
    try
        close submitTab
    end try
end tell
try
    tell application frontAppName to activate
end try
return resultInfo

on findSubmitTab(theTab, wantedTag, contestPrefix, resultsMark)
    tell application "__APP__"
        try
            set u to URL of theTab
            if u contains wantedTag then return theTab
            if u starts with contestPrefix and u contains resultsMark then return theTab
        end try
        repeat with w in windows
            try
                repeat with t in tabs of w
                    try
                        if (URL of t) contains wantedTag then return t
                    end try
                end repeat
            end try
        end repeat
        set fallback to missing value
        repeat with w in windows
            try
                repeat with t in tabs of w
                    try
                        set u to URL of t
                        if u starts with contestPrefix then
                            if u contains resultsMark then return t
                            if fallback is missing value then set fallback to t
                        end if
                    end try
                end repeat
            end try
        end repeat
        if fallback is not missing value then return fallback
    end tell
    return missing value
end findSubmitTab

on alertCaptcha(theTab)
    tell application "__APP__"
        try
            set targetURL to URL of theTab
            set targetIndex to index of theTab
            repeat with w in windows
                try
                    repeat with t in tabs of w
                        try
                            if (URL of t) is targetURL and (index of t) is targetIndex then
                                set current tab of w to t
                                try
                                    set index of w to 1
                                end try
                                exit repeat
                            end if
                        end try
                    end repeat
                end try
            end repeat
        end try
        activate
    end tell
    try
        display notification "Solve the CAPTCHA to continue this submission." with title "__APP__ CAPTCHA" sound name "Ping"
    end try
    log "CAPTCHA: Please solve the CAPTCHA in __APP__..."
end alertCaptcha"#;

fn temp_file(prefix: &str, suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("cphelper_{prefix}_{}_{nanos}{suffix}", std::process::id()))
}

fn status_for_line(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("CAPTCHA:") {
        Some(format!("⚠ CAPTCHA —{rest}"))
    } else if let Some(rest) = line.strip_prefix("RELOAD: ") {
        Some(format!("⌛ {rest}"))
    } else if line == "WAIT" {
        Some("⌛ waiting for the page…".to_string())
    } else if line.starts_with("WAIT") {
        Some(format!("⌛ {line}"))
    } else if !line.contains("RESULT") && !line.contains("REJECTED") {
        Some(format!("● {line}"))
    } else {
        None
    }
}

/// Where Safari should report verdicts for a submission, plus the URL pieces
/// used to re-identify the submission's tab on every poll. Safari's scripting
/// dictionary exposes no tab `id`, so the tab is tagged with a unique URL
/// fragment and, once navigation drops the fragment, matched by contest URL.
fn tracking(submit_url: &str) -> (String, String, &'static str) {
    let prefix = submit_url
        .strip_suffix("submit")
        .unwrap_or(submit_url)
        .to_string();
    if submit_url.ends_with("/problemset/submit") {
        (prefix.clone(), format!("{prefix}status?my=on"), "/status")
    } else {
        (prefix.clone(), format!("{prefix}my"), "/my")
    }
}

// ---------------------------------------------------------------------------
// Submission flow
// ---------------------------------------------------------------------------

/// Remove the `// URL:` header (and legacy embedded test blocks) before submitting.
fn strip_metadata(code: &str) -> String {
    let mut out = code.to_string();
    if let Some(start) = out.find("/* === TEST CASES ===")
        && let Some(end_rel) = out[start..].find("=== END TEST CASES === */") {
            let end = start + end_rel + "=== END TEST CASES === */".len();
            out.replace_range(start..end, "");
        }
    out.lines()
        .filter(|l| !(l.starts_with("// URL:") || l.starts_with("# URL:")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_from_file(code: &str) -> Option<String> {
    code.lines()
        .find_map(|l| l.strip_prefix("// URL: ").or_else(|| l.strip_prefix("# URL: ")))
        .map(|s| s.trim().to_string())
}

fn safari_installed() -> bool {
    Path::new("/Applications/Safari.app").exists()
        || Path::new("/System/Applications/Safari.app").exists()
        || Path::new("/System/Cryptexes/App/System/Applications/Safari.app").exists()
}

/// Run a full submission, calling `notify` with human-readable progress lines.
/// Used by both the TUI worker and the `submit` CLI subcommand.
pub fn submit(
    config: &SubmitConfig,
    notify: impl Fn(String) + Send + Clone + 'static,
) -> Result<String, AppError> {
    let (info, platform) = plan(config)?;

    if !info.safari_available {
        return Err(AppError::Io(
            "Safari not found — silent submissions require Safari".to_string(),
        ));
    }

    let code = fs::read_to_string(&config.file_path)
        .map_err(|e| AppError::Io(format!("cannot read {}: {e}", config.file_path.display())))?;

    let language = config.language;
    let lang_id = language.cf_id();

    let clean_code = strip_metadata(&code);
    let code_b64 = BASE64.encode(clean_code.as_bytes());

    let fill_js = CF_FILL_JS
        .replace("__CODE_B64__", &code_b64)
        .replace("__LANG_ID__", lang_id)
        .replace("__PROBLEM_CODE__", &platform.problem_code.to_uppercase());
    let result_js = CF_RESULT_JS.to_string();

    notify(format!(
        "Submitting {} as {} to {}",
        info.file_name, info.language, info.platform
    ));

    let tag = format!(
        "zch{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    run_applescript(&platform, &fill_js, &result_js, &tag, notify)
}

fn build_applescript(platform: &Platform, fill_path: &Path, result_path: &Path, tag: &str) -> String {
    // Quotes inside the inline `do JavaScript` string must be escaped for AppleScript.
    let safe_selector = platform.ready_selector.replace('"', "\\\"");
    let (prefix, results_url, results_mark) = tracking(&platform.submit_url);
    APPLESCRIPT_TEMPLATE
        .replace("__APP__", APP_NAME)
        .replace("__SUBMIT_URL__", &platform.submit_url)
        .replace("__TAG__", tag)
        .replace("__CONTEST_PREFIX__", &prefix)
        .replace("__RESULTS_URL__", &results_url)
        .replace("__RESULTS_MARK__", results_mark)
        .replace("__READY_SELECTOR__", &safe_selector)
        .replace("__FILL_JS_PATH__", &fill_path.to_string_lossy())
        .replace("__RESULT_JS_PATH__", &result_path.to_string_lossy())
}

fn run_applescript(
    platform: &Platform,
    fill_js: &str,
    result_js: &str,
    tag: &str,
    notify: impl Fn(String) + Send + Clone + 'static,
) -> Result<String, AppError> {
    let fill_path = temp_file("fill", ".js");
    let result_path = temp_file("result", ".js");
    let script_path = temp_file("run", ".applescript");

    fs::write(&fill_path, fill_js).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(&result_path, result_js).map_err(|e| AppError::Io(e.to_string()))?;

    let script = build_applescript(platform, &fill_path, &result_path, tag);
    fs::write(&script_path, script).map_err(|e| AppError::Io(e.to_string()))?;

    let result = (|| -> Result<String, AppError> {
        let mut child = Command::new("/usr/bin/osascript")
            .arg(&script_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Io(format!("failed to run osascript: {e}")))?;

        let stderr = child.stderr.take().expect("stderr piped");
        let notify_thread = notify.clone();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(msg) = status_for_line(line.trim()) {
                    notify_thread(msg);
                }
            }
        });

        let mut stdout = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut stdout);
        }
        let _ = child.wait();
        let _ = reader.join();
        Ok(stdout.trim().to_string())
    })();

    let _ = fs::remove_file(&fill_path);
    let _ = fs::remove_file(&result_path);
    let _ = fs::remove_file(&script_path);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_codeforces_contest() {
        let p = detect_platform("https://codeforces.com/contest/2266/problem/E").unwrap();
        assert_eq!(p.submit_url, "https://codeforces.com/contest/2266/submit");
        assert_eq!(p.problem_code, "E");
    }

    #[test]
    fn detects_codeforces_group_contest() {
        let p = detect_platform("https://codeforces.com/group/xyz/contest/123/problem/A1").unwrap();
        assert_eq!(p.submit_url, "https://codeforces.com/group/xyz/contest/123/submit");
        assert_eq!(p.problem_code, "A1");
    }

    #[test]
    fn detects_codeforces_problemset() {
        let p = detect_platform("https://codeforces.com/problemset/problem/2266/E").unwrap();
        assert_eq!(p.submit_url, "https://codeforces.com/problemset/submit");
        assert_eq!(p.problem_code, "2266E");
    }

    #[test]
    fn rejects_unsupported_platforms() {
        assert!(detect_platform("https://example.com/problem/1").is_none());
        assert!(detect_platform("https://atcoder.jp/contests/abc452/tasks/abc452_c").is_none());
    }

    #[test]
    fn interrupted_submissions_classify_as_failed() {
        assert_eq!(classify("INTERRUPTED: Safari tab was closed"), Outcome::Failed);
        assert_eq!(
            classify("TIMED OUT: the judge did not answer (last state: WAIT)"),
            Outcome::Failed
        );
    }

    #[test]
    fn derives_tracking_urls() {
        let (prefix, results, mark) = tracking("https://codeforces.com/contest/2266/submit");
        assert_eq!(prefix, "https://codeforces.com/contest/2266/");
        assert_eq!(results, "https://codeforces.com/contest/2266/my");
        assert_eq!(mark, "/my");

        let (prefix, results, mark) = tracking("https://codeforces.com/problemset/submit");
        assert_eq!(prefix, "https://codeforces.com/problemset/");
        assert_eq!(results, "https://codeforces.com/problemset/status?my=on");
        assert_eq!(mark, "/status");
    }

    #[test]
    fn strips_url_and_test_blocks() {
        let code = "// URL: https://codeforces.com/contest/1/problem/A\n#include <bits/stdc++.h>\n/* === TEST CASES ===\n1\n=== END TEST CASES === */\nint main(){}";
        let clean = strip_metadata(code);
        assert!(!clean.contains("URL:"));
        assert!(!clean.contains("TEST CASES"));
        assert!(clean.contains("#include"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generated_applescript_compiles() {
        let platform = detect_platform("https://codeforces.com/contest/2266/problem/E").unwrap();
        let fill = temp_file("testfill", ".js");
        let result = temp_file("testresult", ".js");
        fs::write(&fill, "1").unwrap();
        fs::write(&result, "1").unwrap();

        let script = build_applescript(&platform, &fill, &result, "zchtest123");
        assert!(
            script.contains("on findSubmitTab(theTab, wantedTag, contestPrefix, resultsMark)"),
            "tab lookup handler missing"
        );
        assert!(script.contains("#zchtest123"), "unique tab tag missing");
        assert!(
            !script.contains("id of"),
            "Safari tabs have no scriptable id — identity lookup must not come back"
        );
        assert!(script.contains("https://codeforces.com/contest/2266/my"), "results URL missing");
        assert!(script.contains("display notification"), "captcha alert missing");
        let script_path = temp_file("testcompile", ".applescript");
        let out_path = temp_file("testcompile", ".scpt");
        fs::write(&script_path, script).unwrap();

        let compile = Command::new("/usr/bin/osacompile")
            .arg("-o")
            .arg(&out_path)
            .arg(&script_path)
            .output()
            .expect("osacompile should run");

        let _ = fs::remove_file(&fill);
        let _ = fs::remove_file(&result);
        let _ = fs::remove_file(&script_path);
        let _ = fs::remove_file(&out_path);

        assert!(
            compile.status.success(),
            "AppleScript failed to compile:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        );
    }
}
