#![cfg(any(target_os = "linux", target_os = "macos"))]

use egui_bench::agent::sandbox;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Some(code) = sandbox::run_helper() {
        std::process::exit(code);
    }
    let mut failed = Vec::new();
    let cases: [(&str, fn()); 9] = [
        ("a command writes inside the workspace and its scratch", writes_inside),
        ("a command cannot write outside the workspace", cannot_write_outside),
        ("a command cannot read what is outside the workspace", cannot_read_outside),
        ("a command cannot follow a link out", cannot_follow_a_link),
        ("the environment is scrubbed", environment_is_scrubbed),
        ("pipes, globs and exit codes work", shell_works),
        ("without a plan the helper refuses to run", refuses_without_a_plan),
        ("a shared folder is readable and writable only as far as shared", shared_folders),
        ("a share does not widen to its neighbours", a_share_stays_where_it_was_put),
    ];
    for (name, case) in cases {
        match std::panic::catch_unwind(case) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.push(name);
            }
        }
    }
    if !failed.is_empty() {
        eprintln!("{} failed: {failed:?}", failed.len());
        std::process::exit(1);
    }
}

struct Rig {
    root: PathBuf,
    workspace: PathBuf,
    scratch: PathBuf,
    outside: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let base = std::env::temp_dir();
        let root = base
            .canonicalize()
            .unwrap_or(base)
            .join(format!("egui-bench-sandbox-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let make = |sub: &str| {
            let dir = root.join(sub);
            fs::create_dir_all(&dir).unwrap();
            dir.canonicalize().unwrap()
        };
        let rig = Rig {
            workspace: make("work"),
            scratch: make("scratch"),
            outside: make("outside"),
            root,
        };
        fs::write(rig.outside.join("secret.txt"), "TOPSECRET").unwrap();
        rig
    }

    fn plan(&self, command: &str, shared: &[(&Path, bool)]) -> String {
        let shared: Vec<_> =
            shared.iter().map(|(p, w)| serde_json::json!({ "path": p, "write": w })).collect();
        serde_json::json!({
            "workspace": self.workspace,
            "scratch": self.scratch,
            "tool_dirs": [],
            "command": command,
            "shared": shared,
        })
        .to_string()
    }

    fn run(&self, command: &str) -> (i32, String) {
        self.run_with(command, &[])
    }

    fn run_with(&self, command: &str, shared: &[(&Path, bool)]) -> (i32, String) {
        let out = Command::new(std::env::current_exe().unwrap())
            .arg(sandbox::FLAG)
            .arg(self.plan(command, shared))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code().unwrap_or(-1), text)
    }

    fn usable(&self) -> bool {
        let (code, text) = self.run("true");
        if code != 0 {
            eprintln!("no sandbox here, skipping: {text}");
        }
        code == 0
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn writes_inside() {
    let rig = Rig::new("inside");
    if !rig.usable() {
        return;
    }
    let (code, out) =
        rig.run("echo hi > out.txt && cat out.txt && echo t > $TMPDIR/t && cat $TMPDIR/t");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "hi\nt\n");
    assert_eq!(fs::read_to_string(rig.workspace.join("out.txt")).unwrap(), "hi\n");
}

fn cannot_write_outside() {
    let rig = Rig::new("write");
    if !rig.usable() {
        return;
    }
    let target = rig.outside.join("new.txt");
    let (_, out) = rig.run(&format!("echo x > {}", target.display()));
    assert!(out.contains("Permission denied"), "{out}");
    assert!(!target.exists());
    let (_, out) = rig.run("echo x > /etc/egui-bench-sandbox-test");
    assert!(!Path::new("/etc/egui-bench-sandbox-test").exists(), "{out}");
}

fn cannot_read_outside() {
    let rig = Rig::new("read");
    if !rig.usable() {
        return;
    }
    let (_, out) = rig.run(&format!("cat {}", rig.outside.join("secret.txt").display()));
    assert!(out.contains("Permission denied"), "{out}");
    assert!(!out.contains("TOPSECRET"));
    if let Some(home) = std::env::var_os("HOME") {
        let (_, out) = rig.run(&format!("ls {}", Path::new(&home).display()));
        assert!(out.contains("Permission denied"), "{out}");
    }
}

fn cannot_follow_a_link() {
    let rig = Rig::new("link");
    if !rig.usable() {
        return;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&rig.outside, rig.workspace.join("door")).unwrap();
    let (_, out) = rig.run("cat door/secret.txt");
    assert!(!out.contains("TOPSECRET"), "{out}");
}

fn environment_is_scrubbed() {
    let rig = Rig::new("env");
    if !rig.usable() {
        return;
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .arg(sandbox::FLAG)
        .arg(rig.plan("env", &[]))
        .env("SECRET_TOKEN", "leaked")
        .env("OPENAI_API_KEY", "sk-leaked")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("leaked"), "{text}");
    assert!(text.contains("TMPDIR="));
    assert!(text.contains(&format!("HOME={}", rig.workspace.display())));
}

fn shell_works() {
    let rig = Rig::new("shell");
    if !rig.usable() {
        return;
    }
    let (code, out) = rig.run("printf 'a\\nb\\n' > l.txt; ls *.txt | wc -l; exit 3");
    assert_eq!(code, 3, "{out}");
    assert_eq!(out.trim(), "1");
}

fn refuses_without_a_plan() {
    let out = Command::new(std::env::current_exe().unwrap()).arg(sandbox::FLAG).output().unwrap();
    assert_eq!(out.status.code(), Some(126));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no plan"));
}

fn shared_folders() {
    let rig = Rig::new("shared");
    if !rig.usable() {
        return;
    }
    let (code, out) = rig.run(&format!("cat {}", rig.outside.join("secret.txt").display()));
    assert!(out.contains("Permission denied") && code != 0 || !out.contains("TOPSECRET"), "{out}");

    let read_only = [(rig.outside.as_path(), false)];
    let (_, out) =
        rig.run_with(&format!("cat {}", rig.outside.join("secret.txt").display()), &read_only);
    assert_eq!(out, "TOPSECRET", "a read only share must be readable: {out}");
    let target = rig.outside.join("new.txt");
    let (_, out) = rig.run_with(&format!("echo x > {}", target.display()), &read_only);
    assert!(out.contains("Permission denied"), "{out}");
    assert!(!target.exists(), "a read only share must not be writable");

    let writable = [(rig.outside.as_path(), true)];
    let (code, out) = rig.run_with(&format!("echo x > {}", target.display()), &writable);
    assert_eq!(code, 0, "{out}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "x\n");
}

fn a_share_stays_where_it_was_put() {
    let rig = Rig::new("neighbour");
    if !rig.usable() {
        return;
    }
    let shared = rig.root.join("shared");
    let neighbour = rig.root.join("neighbour");
    fs::create_dir_all(&shared).unwrap();
    fs::create_dir_all(&neighbour).unwrap();
    fs::write(shared.join("a.txt"), "A").unwrap();
    fs::write(neighbour.join("b.txt"), "B").unwrap();
    let share = [(shared.as_path(), true)];
    let (_, out) = rig.run_with(&format!("cat {}", shared.join("a.txt").display()), &share);
    assert_eq!(out, "A");
    let (_, out) = rig.run_with(&format!("cat {}", neighbour.join("b.txt").display()), &share);
    assert!(out.contains("Permission denied") && !out.contains('B'), "{out}");
    let (_, out) = rig.run_with(&format!("ls {}", rig.root.display()), &share);
    assert!(out.contains("Permission denied"), "{out}");
    let (_, out) =
        rig.run_with(&format!("echo x > {}", rig.root.join("top.txt").display()), &share);
    assert!(out.contains("Permission denied"), "{out}");
}
