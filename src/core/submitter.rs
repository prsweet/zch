use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{Sender, channel},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
const SUBMISSION_TIMEOUT: Duration = Duration::from_secs(300);

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
    PartiallyAccepted,
    Accepted,
    Rejected,
    Failed,
}

/// Judge verdict of the final `submitter::submit` string. Shared by the TUI
/// submission bar and result classification.
///
/// Only a final `RESULT:` is a verdict. Failures mention pretests too
/// ("Wrong answer on pretest 2"), so "pretest" alone means nothing; only
/// "Pretests passed" is the partial (yellow) outcome.
pub fn classify(result: &str) -> Outcome {
    if result.starts_with("REJECTED") {
        return Outcome::Rejected;
    }
    let Some(verdict) = result.strip_prefix("RESULT:") else {
        return Outcome::Failed;
    };
    let verdict = verdict.trim().to_lowercase();
    if verdict.starts_with("pretests passed") {
        Outcome::PartiallyAccepted
    } else if verdict.starts_with("accepted")
        || verdict.starts_with("happy new year")
        || verdict.starts_with("perfect result")
    {
        Outcome::Accepted
    } else {
        Outcome::Rejected
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
    pub language_id: String,
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
                "no problem URL found (receive the problem with Competitive Companion first)"
                    .to_string(),
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
        language_id: config.language_id.clone(),
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
            if (!found) return 'ERROR: problem __PROBLEM_CODE__ is not in this contest\'s problem list';
            probSelect.dispatchEvent(new Event('change', {bubbles: true}));
        }

        // Code injection
        var ta = document.getElementById('sourceCodeTextarea');
        if (ta) ta.value = code;
        try { var ed = document.querySelector('.ace_editor'); if (ed && typeof ace !== 'undefined') ace.edit(ed).setValue(code, -1); } catch(e) {}

        // Language selection
        // Never submit under whatever language happens to be preselected.
        var sel = document.querySelector('select[name="programTypeId"]');
        if (sel) {
            var langFound = false;
            for (var i = 0; i < sel.options.length; i++) {
                if (sel.options[i].value === '__LANG_ID__') { sel.value = '__LANG_ID__'; langFound = true; break; }
            }
            if (!langFound) return 'ERROR: Codeforces offers no language id __LANG_ID__ here (set cf_language_ids in ~/.zch/config.json)';
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

/// Run on the submit page before submitting: the newest submission id on the
/// results page. Anything above it afterwards is a new submission.
const CF_BASELINE_JS: &str = r#"(function() {
    try {
        var x = new XMLHttpRequest();
        x.open('GET', '__RESULTS_PATH__', false);
        x.send(null);
        if (x.status !== 200) return '0';
        var doc = new DOMParser().parseFromString(x.responseText, 'text/html');
        var rows = doc.querySelectorAll('tr[data-submission-id]');
        var max = 0;
        for (var i = 0; i < rows.length; i++) {
            var id = parseInt(rows[i].getAttribute('data-submission-id'), 10) || 0;
            if (id > max) max = id;
        }
        return String(max);
    } catch (e) { return '0'; }
})();"#;

/// One verdict poll, answered as `<submission id>|<state>`. The AppleScript
/// prepends `ZCH_ID` (the id once known) and `ZCH_BASE` (the baseline).
///
/// The results page is fetched with a synchronous XHR and parsed off-screen
/// instead of reloading the tab: no CSS/JS/images per poll, so Codeforces is
/// far less likely to throttle or challenge us. The row is picked by id —
/// never "the top row" — so parallel submissions cannot read each other's
/// verdict, and an old verdict for the same problem is never reported.
const CF_RESULT_JS: &str = r#"(function() {
    var known = (typeof ZCH_ID !== 'undefined' && ZCH_ID) ? String(ZCH_ID) : '';
    try {
        var base = (typeof ZCH_BASE !== 'undefined') ? (parseInt(ZCH_BASE, 10) || 0) : 0;
        var target = '__PROBLEM_CODE__';
        var say = function(state) { return known + '|' + state; };
        var challenged = function(d) {
            var t = d.title || '';
            return t.indexOf('Just a moment') > -1 || t.indexOf('Attention Required') > -1;
        };

        if (challenged(document)) return say('CAPTCHA');
        var url = document.location.href;
        if (url.indexOf('/submit') > -1) {
            var eSub = document.querySelector('.error.for__submittedProblemCode, .error.for__source');
            if (eSub && eSub.textContent.trim() !== '') {
                if (eSub.textContent.toLowerCase().indexOf('captcha') > -1) return say('CAPTCHA');
                return say('REJECTED: ' + eSub.textContent.trim());
            }
            var ws = document.querySelectorAll('.cf-turnstile, #turnstile-wrapper, .g-recaptcha');
            for (var w = 0; w < ws.length; w++) { if (ws[w].innerHTML.trim() !== '') return say('CAPTCHA'); }
            if (document.querySelector('iframe[src*="captcha"], iframe[src*="challenge"], iframe[src*="turnstile"]')) return say('CAPTCHA');
            return say('WAIT');
        }

        var x = new XMLHttpRequest();
        x.open('GET', '__RESULTS_PATH__', false);
        x.send(null);
        if (x.status !== 200) return say('WAIT: results page answered ' + x.status);
        var doc = new DOMParser().parseFromString(x.responseText, 'text/html');
        if (challenged(doc)) return say('CAPTCHA_FETCH');

        var codesOf = function(row) {
            var out = [];
            var links = row.querySelectorAll('a[href*="/problem/"]');
            for (var i = 0; i < links.length; i++) {
                var h = links[i].getAttribute('href') || '';
                var m = h.match(/\/(?:contest|gym)\/(\d+)\/problem\/([A-Za-z0-9]+)/) ||
                        h.match(/\/problemset\/problem\/(\d+)\/([A-Za-z0-9]+)/);
                if (m) { out.push(m[2].toUpperCase()); out.push((m[1] + m[2]).toUpperCase()); }
            }
            return out;
        };
        var isOurs = function(row) {
            var codes = codesOf(row);
            return codes.length === 0 || codes.indexOf(target) > -1;
        };

        var rows = doc.querySelectorAll('tr[data-submission-id]');
        var row = null;
        var bestId = 0;
        for (var r = 0; r < rows.length; r++) {
            var id = parseInt(rows[r].getAttribute('data-submission-id'), 10) || 0;
            if (known) {
                if (String(id) === known) { row = rows[r]; break; }
            } else if (isOurs(rows[r])) {
                // With a baseline: the oldest new submission (ours came first).
                // Without one: the newest submission for this problem.
                if (base > 0) {
                    if (id > base && (row === null || id < bestId)) { row = rows[r]; bestId = id; }
                } else if (row === null || id > bestId) { row = rows[r]; bestId = id; }
            }
        }
        if (!row) return say('RELOAD: Waiting for submission');

        known = row.getAttribute('data-submission-id') || known;
        var cell = row.querySelector('td.status-verdict-cell') || row.querySelector('.submissionVerdictWrapper');
        var vt = cell ? cell.textContent.replace(/\s+/g, ' ').trim() : '';
        var waiting = cell && cell.getAttribute('waiting') === 'true';
        if (!vt || waiting || /running|queue|judging|testing|pending/i.test(vt)) {
            return say('RELOAD: ' + (vt || 'In queue'));
        }
        return say('RESULT: ' + vt);
    } catch (e) { return known + '|WAIT'; }
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
    set baselineJS to read POSIX file "__BASELINE_JS_PATH__"
    set baseId to "0"
    try
        set baseId to (do JavaScript baselineJS in submitTab) as text
    end try
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
    log "submitted · waiting for the verdict"
    set resultJS to read POSIX file "__RESULT_JS_PATH__"
    set subId to ""
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
            set rawInfo to "|WAIT"
            try
                set rawInfo to (do JavaScript ("var ZCH_ID='" & subId & "'; var ZCH_BASE='" & baseId & "';" & resultJS) in submitTab) as text
            end try
            -- The script answers "<submission id>|<state>"; keep the id.
            set barPos to offset of "|" in rawInfo
            if barPos is 0 then
                set resultInfo to rawInfo
            else
                if barPos > 1 then set subId to text 1 thru (barPos - 1) of rawInfo
                if barPos < (length of rawInfo) then
                    set resultInfo to text (barPos + 1) thru -1 of rawInfo
                else
                    set resultInfo to "WAIT"
                end if
            end if

            if resultInfo is "CAPTCHA" or resultInfo is "CAPTCHA_FETCH" then
                if resCaptchaAlerted is false then
                    set resCaptchaAlerted to true
                    -- The challenge came from the background fetch: show it in the tab.
                    if resultInfo is "CAPTCHA_FETCH" then set URL of submitTab to "__RESULTS_URL__"
                    my alertCaptcha(submitTab)
                end if
                log "CAPTCHA: Waiting for you to solve it..."
            else
                log resultInfo
                set resCaptchaAlerted to false
            end if

            if resultInfo starts with "RESULT:" or resultInfo starts with "REJECTED:" then exit repeat
            delay 2
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
    std::env::temp_dir().join(format!(
        "cphelper_{prefix}_{}_{nanos}{suffix}",
        std::process::id()
    ))
}

/// One AppleScript log line as plain progress text. Glyphs and colours are
/// the UI's business: this only says what is happening.
fn status_for_line(line: &str) -> Option<String> {
    if line.starts_with("CAPTCHA") {
        Some("captcha".to_string())
    } else if let Some(rest) = line.strip_prefix("RELOAD: ") {
        Some(rest.trim().to_string())
    } else if line == "WAIT" {
        Some("waiting for Codeforces".to_string())
    } else if let Some(rest) = line.strip_prefix("WAIT:") {
        Some(rest.trim().to_string())
    } else if line.is_empty() || line.contains("RESULT") || line.contains("REJECTED") {
        None
    } else {
        Some(line.to_string())
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
        && let Some(end_rel) = out[start..].find("=== END TEST CASES === */")
    {
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
        .find_map(|l| {
            l.strip_prefix("// URL: ")
                .or_else(|| l.strip_prefix("# URL: "))
        })
        .map(|s| s.trim().to_string())
}

fn safari_installed() -> bool {
    Path::new("/Applications/Safari.app").exists()
        || Path::new("/System/Applications/Safari.app").exists()
        || Path::new("/System/Cryptexes/App/System/Applications/Safari.app").exists()
}

/// Run a full submission, calling `notify` with human-readable progress lines.
/// Used by the TUI submitter worker.
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

    let lang_id = js_literal_safe(&config.language_id);
    let problem_code = js_literal_safe(&platform.problem_code.to_uppercase());

    let clean_code = strip_metadata(&code);
    let code_b64 = BASE64.encode(clean_code.as_bytes());

    let fill_js = CF_FILL_JS
        .replace("__CODE_B64__", &code_b64)
        .replace("__LANG_ID__", &lang_id)
        .replace("__PROBLEM_CODE__", &problem_code);
    let (_, results_url, _) = tracking(&platform.submit_url);
    let results_path = results_url
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string()))
        .unwrap_or_else(|| "/".to_string());
    let baseline_js = CF_BASELINE_JS.replace("__RESULTS_PATH__", &js_literal_safe(&results_path));
    let result_js = CF_RESULT_JS
        .replace("__RESULTS_PATH__", &js_literal_safe(&results_path))
        .replace("__PROBLEM_CODE__", &problem_code);

    notify(format!(
        "submitting {} as {}",
        info.file_name, info.language
    ));

    let tag = format!(
        "zch{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let scripts = Scripts {
        fill: &fill_js,
        baseline: &baseline_js,
        result: &result_js,
    };
    run_applescript(&platform, scripts, &tag, notify)
}

/// Values pasted into a single-quoted JS string: anything but plain
/// URL/identifier characters is dropped, so a crafted URL cannot break out.
fn js_literal_safe(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "/?=&._-".contains(*c))
        .collect()
}

/// The JavaScript files the AppleScript reads.
struct Scripts<'a> {
    fill: &'a str,
    baseline: &'a str,
    result: &'a str,
}

/// Where the AppleScript finds each JavaScript file.
struct ScriptPaths<'a> {
    fill: &'a Path,
    baseline: &'a Path,
    result: &'a Path,
}

fn build_applescript(platform: &Platform, paths: &ScriptPaths, tag: &str) -> String {
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
        .replace("__FILL_JS_PATH__", &paths.fill.to_string_lossy())
        .replace("__BASELINE_JS_PATH__", &paths.baseline.to_string_lossy())
        .replace("__RESULT_JS_PATH__", &paths.result.to_string_lossy())
}

fn run_applescript(
    platform: &Platform,
    scripts: Scripts,
    tag: &str,
    notify: impl Fn(String) + Send + Clone + 'static,
) -> Result<String, AppError> {
    let fill_path = temp_file("fill", ".js");
    let baseline_path = temp_file("baseline", ".js");
    let result_path = temp_file("result", ".js");
    let script_path = temp_file("run", ".applescript");

    let written = fs::write(&fill_path, scripts.fill)
        .and_then(|_| fs::write(&baseline_path, scripts.baseline))
        .and_then(|_| fs::write(&result_path, scripts.result));
    if let Err(e) = written {
        for path in [&fill_path, &baseline_path, &result_path] {
            let _ = fs::remove_file(path);
        }
        return Err(AppError::Io(e.to_string()));
    }

    let paths = ScriptPaths {
        fill: &fill_path,
        baseline: &baseline_path,
        result: &result_path,
    };
    let script = build_applescript(platform, &paths, tag);
    fs::write(&script_path, script).map_err(|e| AppError::Io(e.to_string()))?;

    let result = (|| -> Result<String, AppError> {
        let mut child = Command::new("/usr/bin/osascript")
            .arg(&script_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Io(format!("failed to run osascript: {e}")))?;

        // Drain both pipes on their own threads: a full stderr pipe would block
        // osascript and the timeout below would never fire.
        let stderr = child.stderr.take().expect("stderr piped");
        let notify_thread = notify.clone();
        let stderr_reader = thread::spawn(move || {
            let mut buffer = String::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                buffer.push_str(&line);
                buffer.push('\n');
                if let Some(msg) = status_for_line(line.trim()) {
                    notify_thread(msg);
                }
            }
            buffer
        });
        let stdout = child.stdout.take().expect("stdout piped");
        let stdout_reader = thread::spawn(move || {
            let mut buffer = String::new();
            let _ = BufReader::new(stdout).read_to_string(&mut buffer);
            buffer
        });

        let started = Instant::now();
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(Some(status)),
                Ok(None) => {
                    if started.elapsed() >= SUBMISSION_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Ok(None);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(e);
                }
            }
        };

        let stderr_text = stderr_reader.join().unwrap_or_default();
        let stdout_text = stdout_reader.join().unwrap_or_default();

        match outcome {
            Err(e) => Err(AppError::Io(format!("failed to watch osascript: {e}"))),
            Ok(None) => Err(AppError::Io(format!(
                "submission timed out after {}s — the judge never answered",
                SUBMISSION_TIMEOUT.as_secs()
            ))),
            Ok(Some(status)) if !status.success() => {
                let detail = if stderr_text.trim().is_empty() {
                    stdout_text.trim()
                } else {
                    stderr_text.trim()
                };
                let detail = detail
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
                    .trim();
                Err(AppError::Io(if detail.is_empty() {
                    format!("osascript exited with {status}")
                } else {
                    format!("osascript exited with {status}: {detail}")
                }))
            }
            Ok(Some(_)) => Ok(stdout_text.trim().to_string()),
        }
    })();

    let _ = fs::remove_file(&fill_path);
    let _ = fs::remove_file(&baseline_path);
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
        assert_eq!(
            p.submit_url,
            "https://codeforces.com/group/xyz/contest/123/submit"
        );
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
        assert_eq!(
            classify("INTERRUPTED: Safari tab was closed"),
            Outcome::Failed
        );
        assert_eq!(
            classify("TIMED OUT: the judge did not answer (last state: WAIT)"),
            Outcome::Failed
        );
    }

    #[test]
    fn pretest_failures_are_rejections() {
        assert_eq!(
            classify("RESULT: Wrong answer on pretest 2"),
            Outcome::Rejected
        );
        assert_eq!(
            classify("RESULT: Time limit exceeded on pretest 1"),
            Outcome::Rejected
        );
        assert_eq!(
            classify("RESULT: Pretests passed"),
            Outcome::PartiallyAccepted
        );
        assert_eq!(classify("RESULT: Accepted"), Outcome::Accepted);
        assert_eq!(classify("RESULT: Happy New Year!"), Outcome::Accepted);
        assert_eq!(classify("RESULT: Compilation error"), Outcome::Rejected);
        assert_eq!(
            classify("REJECTED: You have submitted exactly the same code before"),
            Outcome::Rejected
        );
        assert_eq!(
            classify("ERROR: Codeforces offers no language id 70 here"),
            Outcome::Failed
        );
    }

    #[test]
    fn values_pasted_into_javascript_cannot_escape_the_string() {
        assert_eq!(js_literal_safe("A1"), "A1");
        assert_eq!(js_literal_safe("/contest/1/my"), "/contest/1/my");
        assert_eq!(
            js_literal_safe("/problemset/status?my=on"),
            "/problemset/status?my=on"
        );
        assert_eq!(js_literal_safe("A');alert(1);//"), "Aalert1//");
    }

    /// Syntax-check the injected JavaScript when Node is available.
    #[test]
    fn injected_javascript_parses() {
        if Command::new("node").arg("--version").output().is_err() {
            return;
        }
        for (name, source) in [
            ("fill", CF_FILL_JS),
            ("baseline", CF_BASELINE_JS),
            ("result", CF_RESULT_JS),
        ] {
            assert!(
                source.is_ascii(),
                "{name}: AppleScript reads the file as MacRoman"
            );
            let path = temp_file(&format!("syntax_{name}"), ".js");
            fs::write(&path, source).unwrap();
            let out = Command::new("node")
                .arg("--check")
                .arg(&path)
                .output()
                .unwrap();
            let _ = fs::remove_file(&path);
            assert!(
                out.status.success(),
                "{name} does not parse:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
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
        let baseline = temp_file("testbaseline", ".js");
        let result = temp_file("testresult", ".js");
        fs::write(&fill, "1").unwrap();
        fs::write(&baseline, "1").unwrap();
        fs::write(&result, "1").unwrap();

        let paths = ScriptPaths {
            fill: &fill,
            baseline: &baseline,
            result: &result,
        };
        let script = build_applescript(&platform, &paths, "zchtest123");
        assert!(
            !script.contains("__"),
            "every placeholder must be filled:\n{script}"
        );
        assert!(
            !script.contains("window.location.reload"),
            "verdicts are polled in the background, never by reloading the tab"
        );
        assert!(
            script.contains("on findSubmitTab(theTab, wantedTag, contestPrefix, resultsMark)"),
            "tab lookup handler missing"
        );
        assert!(script.contains("#zchtest123"), "unique tab tag missing");
        assert!(
            !script.contains("id of"),
            "Safari tabs have no scriptable id — identity lookup must not come back"
        );
        assert!(
            script.contains("https://codeforces.com/contest/2266/my"),
            "results URL missing"
        );
        assert!(
            script.contains("display notification"),
            "captcha alert missing"
        );
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
        let _ = fs::remove_file(&baseline);
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
