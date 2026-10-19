use super::grants::{self, Decision};
use super::sandbox;
use super::skillset;
use super::{Backend, Cx};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: u64 = 120;
const MAX_TIMEOUT: u64 = 900;
const MAX_OUTPUT: usize = 30_000;

pub const NAMES: [&str; 9] = [
    "bash",
    "request_access",
    "load_skill",
    "list_files",
    "read_file",
    "write_file",
    "edit_file",
    "search_files",
    "workspace",
];

fn text(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(String::from)
}

fn number(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn need(args: &Value, key: &str) -> Result<String, String> {
    text(args, key).ok_or_else(|| format!("missing `{key}`"))
}

pub fn run(
    backend: &dyn Backend,
    cx: &Cx,
    name: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    if !NAMES.contains(&name) {
        return backend.call(name, args, cx);
    }
    let toolbox = Toolbox { backend, cx };
    toolbox.run(name, args)
}

struct Toolbox<'a> {
    backend: &'a dyn Backend,
    cx: &'a Cx<'a>,
}

impl Toolbox<'_> {
    fn run(&self, name: &str, args: &Value) -> Option<Result<String, String>> {
        if !NAMES.contains(&name) {
            return None;
        }
        let ws = self.cx.workspace;
        Some(match name {
            "workspace" => Ok(workspace_note(ws)),
            "request_access" => self.request_access(args),
            "load_skill" => {
                let skills = skillset::discover(
                    &skillset::roots(ws.root()),
                    &self.backend.preferred_skills(),
                );
                skillset::load(
                    &skills,
                    text(args, "name").as_deref(),
                    text(args, "file").as_deref(),
                )
            }
            "list_files" => ws.list(text(args, "path").as_deref(), number(args, "depth")),
            "read_file" => need(args, "path")
                .and_then(|p| ws.read(&p, number(args, "offset"), number(args, "limit"))),
            "write_file" => need(args, "path")
                .and_then(|p| ws.write(&p, &text(args, "content").unwrap_or_default())),
            "edit_file" => (|| {
                let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
                ws.edit(
                    &need(args, "path")?,
                    &need(args, "old")?,
                    &text(args, "new").unwrap_or_default(),
                    all,
                )
            })(),
            "search_files" => {
                need(args, "pattern").and_then(|p| ws.search(&p, text(args, "path").as_deref()))
            }
            "bash" => self.bash(args),
            _ => return None,
        })
    }

    fn request_access(&self, args: &Value) -> Result<String, String> {
        let given = need(args, "path")?;
        let write = args.get("write").and_then(Value::as_bool).unwrap_or(false);
        let reason = text(args, "reason").unwrap_or_default();
        if reason.trim().is_empty() {
            return Err("give a reason: the user reads it when deciding".into());
        }
        let ws = self.cx.workspace;
        let real = grants::vet(&given, write, self.backend.config_dir().as_deref(), ws.root())?;
        if real.starts_with(ws.root()) {
            return Ok(format!("{} is inside the workspace already", real.display()));
        }
        if ws.grants().covers(&real, write) {
            return Ok(format!(
                "{} is already shared{}",
                real.display(),
                if write { " for writing" } else { "" }
            ));
        }
        let ring = || self.cx.desk.ring();
        let decision = ws.grants().ask(real.clone(), write, &reason, self.cx.stop, &ring)?;
        match decision {
            Decision::Deny => Err(format!("the user said no to {}", real.display())),
            Decision::Read if write => Ok(format!(
                "the user shared {} read only: you can read it but not change it",
                real.display()
            )),
            Decision::Read => Ok(format!("the user shared {} read only", real.display())),
            Decision::Write => {
                Ok(format!("the user shared {} for reading and writing", real.display()))
            }
        }
    }

    fn bash(&self, args: &Value) -> Result<String, String> {
        let command = need(args, "command")?;
        if command.trim().is_empty() {
            return Err("`command` is empty".into());
        }
        if !sandbox::supported() {
            return Err(format!(
                "bash is unavailable: no sandbox on this machine ({})",
                sandbox::why_not()
            ));
        }
        let tool_dirs = self.backend.tool_dirs(self.cx);
        let scratch = scratch_dir()?;
        let plan = sandbox::Plan {
            workspace: self.cx.workspace.root().to_path_buf(),
            scratch: scratch.clone(),
            tool_dirs,
            command: command.clone(),
            shared: self
                .cx
                .workspace
                .grants()
                .list()
                .into_iter()
                .map(|g| sandbox::Shared { path: g.path, write: g.write })
                .collect(),
        };
        let timeout = args
            .get("timeout_s")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(1, MAX_TIMEOUT);
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let started = Instant::now();
        let out = execute(
            &me,
            &sandbox::helper_args(&plan)?,
            self.cx.workspace.root(),
            None,
            Duration::from_secs(timeout),
            self.cx.stop,
        );
        let _ = std::fs::remove_dir_all(&scratch);
        report(format!("$ {command}\n"), out?, started, timeout)
    }
}

pub fn workspace_note(ws: &super::workspace::Workspace) -> String {
    let mut out = format!("The workspace is {}", ws.root().display());
    let shared = ws.grants().list();
    if shared.is_empty() {
        out.push_str(".\nNo other folder is shared. request_access asks the user to share one.");
        return out;
    }
    out.push_str(".\nThe user has also shared:");
    for g in shared {
        let how = if g.write { "read and write" } else { "read only" };
        out.push_str(&format!("\n- {} ({how})", g.path.display()));
    }
    out
}

pub fn report(
    mut shown: String,
    out: Output,
    started: Instant,
    timeout: u64,
) -> Result<String, String> {
    shown.push_str(&out.text);
    let took = started.elapsed().as_secs_f32();
    let end = match out.status {
        Status::Exited(code) => format!("\n[exit {code} in {took:.1}s]"),
        Status::Signal => format!("\n[killed by a signal after {took:.1}s]"),
        Status::TimedOut => {
            format!("\n[killed after {timeout}s: too slow, pass a larger timeout_s]")
        }
    };
    shown.push_str(&end);
    match out.status {
        Status::Exited(0) => Ok(shown),
        _ => Err(shown),
    }
}

fn scratch_dir() -> Result<PathBuf, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("egui-bench-bash-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    dir.canonicalize().map_err(|e| e.to_string())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Exited(i32),
    Signal,
    TimedOut,
}

pub struct Output {
    pub text: String,
    pub status: Status,
}

pub fn execute(
    program: &Path,
    argv: &[String],
    cwd: &Path,
    stdin: Option<&str>,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<Output, String> {
    let mut cmd = Command::new(program);
    cmd.args(argv)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_console(&mut cmd);
    own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", program.display()))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(input.as_bytes());
        });
    }
    let (done, drained) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut readers = 0;
    let mut drain = |pipe: Option<Box<dyn Read + Send>>| {
        let Some(mut pipe) = pipe else { return };
        readers += 1;
        let done = done.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            let _ = done.send(buf);
        });
    };
    drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                break match s.code() {
                    Some(c) => Status::Exited(c),
                    None => Status::Signal,
                };
            }
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if stop.load(Ordering::Relaxed) {
            stop_group(&mut child);
            return Err("stopped".into());
        }
        if Instant::now() >= deadline {
            stop_group(&mut child);
            break Status::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let mut text = String::new();
    for _ in 0..readers {
        let Ok(bytes) = drained.recv_timeout(Duration::from_millis(500)) else { break };
        text.push_str(&String::from_utf8_lossy(&bytes));
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
    }
    Ok(Output { text: clip(text), status })
}

fn clip(text: String) -> String {
    if text.len() <= MAX_OUTPUT {
        return text;
    }
    let keep = MAX_OUTPUT / 2;
    let mut head = keep;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - keep;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}\n[... {} bytes left out ...]\n{}", &text[..head], tail - head, &text[tail..])
}

#[cfg(unix)]
fn own_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn own_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn stop_group(child: &mut std::process::Child) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn stop_group(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn no_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn no_console(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_command_runs_in_the_workspace_with_stdin_and_reports_its_exit() {
        let stop = AtomicBool::new(false);
        let dir = std::env::temp_dir();
        let got = execute(
            Path::new("/bin/sh"),
            &v(&["-c", "cat; echo err >&2; exit 3"]),
            &dir,
            Some("hello\n"),
            Duration::from_secs(10),
            &stop,
        )
        .unwrap();
        assert_eq!(got.status, Status::Exited(3));
        assert!(got.text.contains("hello") && got.text.contains("err"));
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_command_is_killed() {
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let got = execute(
            Path::new("/bin/sh"),
            &v(&["-c", "sleep 30"]),
            &std::env::temp_dir(),
            None,
            Duration::from_millis(300),
            &stop,
        )
        .unwrap();
        assert_eq!(got.status, Status::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn long_output_keeps_its_head_and_tail() {
        let big = format!("{}{}", "a".repeat(40_000), "z".repeat(40_000));
        let cut = clip(big);
        assert!(cut.len() < 31_000);
        assert!(cut.starts_with('a') && cut.ends_with('z'));
        assert!(cut.contains("bytes left out"));
    }

    struct Stub;

    impl super::super::Backend for Stub {
        fn app(&self) -> &'static str {
            "egui-bench-test"
        }

        fn brief(&self, _: &Path, _: &[skillset::Skill]) -> String {
            String::new()
        }

        fn welcome(&self) -> super::super::Welcome {
            super::super::Welcome {
                kicker: String::new(),
                title: String::new(),
                text: String::new(),
                placeholder: String::new(),
            }
        }
    }

    struct Rig {
        base: PathBuf,
        ws: super::super::workspace::Workspace,
        elsewhere: PathBuf,
        desk: super::super::Desk,
        stop: AtomicBool,
    }

    fn rig(name: &str) -> Rig {
        let base = std::env::temp_dir().join(format!("local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("work")).unwrap();
        std::fs::create_dir_all(base.join("elsewhere")).unwrap();
        std::fs::write(base.join("elsewhere").join("lib.txt"), "library\n").unwrap();
        let (desk, _asks) = super::super::Desk::new(&egui::Context::default());
        Rig {
            ws: super::super::workspace::Workspace::open(&base.join("work")).unwrap(),
            elsewhere: base.join("elsewhere").canonicalize().unwrap(),
            base,
            desk,
            stop: AtomicBool::new(false),
        }
    }

    fn call(rig: &Rig, name: &str, args: Value) -> Result<String, String> {
        let cx = Cx { workspace: &rig.ws, desk: &rig.desk, stop: &rig.stop };
        run(&Stub, &cx, name, &args).unwrap()
    }

    fn answer(rig: &Rig, decision: Decision) -> std::thread::JoinHandle<()> {
        let grants = rig.ws.grants().clone();
        std::thread::spawn(move || {
            let id = loop {
                if let Some(r) = grants.pending().pop() {
                    break r.id;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            grants.decide(id, decision);
        })
    }

    #[test]
    fn access_is_asked_for_and_then_the_file_tools_reach_the_folder() {
        let rig = rig("access");
        let lib = rig.elsewhere.join("lib.txt");
        let lib = lib.to_str().unwrap();
        assert!(
            call(&rig, "read_file", json!({ "path": lib })).unwrap_err().contains("request_access")
        );

        let user = answer(&rig, Decision::Read);
        let got = call(
            &rig,
            "request_access",
            json!({ "path": rig.elsewhere, "reason": "the parts library" }),
        );
        user.join().unwrap();
        assert!(got.unwrap().contains("read only"));
        assert_eq!(call(&rig, "read_file", json!({ "path": lib })).unwrap(), "library\n");
        assert!(call(&rig, "write_file", json!({ "path": lib, "content": "x" })).is_err());
        assert!(call(&rig, "workspace", json!({})).unwrap().contains("(read only)"));
        let _ = std::fs::remove_dir_all(&rig.base);
    }

    #[test]
    fn a_no_is_an_error_and_shares_nothing() {
        let rig = rig("deny");
        let user = answer(&rig, Decision::Deny);
        let got = call(
            &rig,
            "request_access",
            json!({ "path": rig.elsewhere, "reason": "the parts library" }),
        );
        user.join().unwrap();
        assert!(got.unwrap_err().contains("said no"));
        assert!(call(&rig, "workspace", json!({})).unwrap().contains("No other folder"));
        let _ = std::fs::remove_dir_all(&rig.base);
    }

    #[test]
    fn a_request_needs_a_reason_and_refuses_credentials_without_asking() {
        let rig = rig("refuse");
        let got = call(&rig, "request_access", json!({ "path": rig.elsewhere }));
        assert!(got.unwrap_err().contains("reason"));
        assert!(rig.ws.grants().pending().is_empty());
        let got = call(&rig, "request_access", json!({ "path": "/", "reason": "everything" }));
        assert!(got.unwrap_err().contains("too broad"));
        assert!(rig.ws.grants().pending().is_empty());
        let got = call(&rig, "request_access", json!({ "path": "nowhere-at-all", "reason": "x" }));
        assert!(got.is_err());
        let inside = call(&rig, "request_access", json!({ "path": "sub", "reason": "x" }));
        assert!(inside.is_err(), "a folder that does not exist cannot be shared");
        std::fs::create_dir_all(rig.base.join("work").join("sub")).unwrap();
        let inside = call(&rig, "request_access", json!({ "path": "sub", "reason": "x" }));
        assert!(inside.unwrap().contains("inside the workspace already"));
        let _ = std::fs::remove_dir_all(&rig.base);
    }

    #[test]
    fn asking_again_for_what_is_shared_does_not_ask_the_user() {
        let rig = rig("again");
        rig.ws.grants().grant(rig.elsewhere.clone(), true);
        let got = call(
            &rig,
            "request_access",
            json!({ "path": rig.elsewhere, "reason": "again", "write": true }),
        );
        assert!(got.unwrap().contains("already shared"));
        assert!(rig.ws.grants().pending().is_empty());
        let _ = std::fs::remove_dir_all(&rig.base);
    }
}
