use crate::meter::progress;
use crate::panel::status;
use egui::Ui;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const AGENT: &str = concat!("egui_bench/", env!("CARGO_PKG_VERSION"));
const PROBE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub page: String,
    pub published: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub bytes: u64,
    pub digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Unchecked,
    Checking,
    Current(Release),
    Newer(Release),
    Failed(String),
}

impl State {
    pub fn release(&self) -> Option<&Release> {
        match self {
            State::Current(r) | State::Newer(r) => Some(r),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Install {
    #[default]
    Idle,
    Fetching {
        got: u64,
        total: u64,
    },
    Unpacking,
    Done(String),
    Failed(String),
}

impl Install {
    pub fn busy(&self) -> bool {
        matches!(self, Install::Fetching { .. } | Install::Unpacking)
    }
}

#[derive(Clone)]
pub struct Updater {
    inner: Arc<Inner>,
}

struct Inner {
    repo: String,
    cache: Option<PathBuf>,
    current: Mutex<Option<String>>,
    state: Mutex<State>,
    install: Mutex<Install>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Updater {
    pub fn new(repo: impl Into<String>, current: Option<String>) -> Updater {
        Updater::build(repo.into(), current, None)
    }

    pub fn cached(repo: impl Into<String>, current: Option<String>, dir: PathBuf) -> Updater {
        Updater::build(repo.into(), current, Some(dir))
    }

    fn build(repo: String, current: Option<String>, cache: Option<PathBuf>) -> Updater {
        Updater {
            inner: Arc::new(Inner {
                repo,
                cache,
                current: Mutex::new(current),
                state: Mutex::new(State::Unchecked),
                install: Mutex::new(Install::Idle),
            }),
        }
    }

    pub fn repo(&self) -> &str {
        &self.inner.repo
    }

    pub fn page(&self) -> String {
        format!("https://github.com/{}/releases", self.inner.repo)
    }

    pub fn current(&self) -> Option<String> {
        lock(&self.inner.current).clone()
    }

    pub fn set_current(&self, version: Option<String>) {
        *lock(&self.inner.current) = version.clone();
        let mut state = lock(&self.inner.state);
        if let Some(r) = state.release().cloned() {
            *state = judge(r, version.as_deref());
        }
    }

    pub fn state(&self) -> State {
        lock(&self.inner.state).clone()
    }

    pub fn install_state(&self) -> Install {
        lock(&self.inner.install).clone()
    }

    pub fn busy(&self) -> bool {
        matches!(self.state(), State::Checking) || self.install_state().busy()
    }

    pub fn check(&self, ctx: &egui::Context) {
        {
            let mut state = lock(&self.inner.state);
            if matches!(*state, State::Checking) {
                return;
            }
            *state = State::Checking;
        }
        let me = self.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = latest(&me.inner.repo, me.inner.cache.as_deref());
            let current = me.current();
            *lock(&me.inner.state) = match outcome {
                Ok(r) => judge(r, current.as_deref()),
                Err(e) => State::Failed(e),
            };
            ctx.request_repaint();
        });
    }

    pub fn install<F>(&self, ctx: &egui::Context, release: &Release, asset: &Asset, place: F)
    where
        F: FnOnce(&Path) -> Result<(), String> + Send + 'static,
    {
        {
            let mut install = lock(&self.inner.install);
            if install.busy() {
                return;
            }
            *install = Install::Fetching { got: 0, total: asset.bytes };
        }
        let me = self.clone();
        let ctx = ctx.clone();
        let version = release.version.clone();
        let asset = asset.clone();
        std::thread::spawn(move || {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let staging = std::env::temp_dir().join(format!(
                "egui_bench-update-{}-{n}-{}",
                std::process::id(),
                asset.name
            ));
            let outcome = (|| {
                let file = download(&asset, &staging, |got, total| {
                    *lock(&me.inner.install) = Install::Fetching { got, total };
                    ctx.request_repaint();
                })?;
                *lock(&me.inner.install) = Install::Unpacking;
                ctx.request_repaint();
                place(&file)
            })();
            let _ = fs::remove_dir_all(&staging);
            *lock(&me.inner.install) = match outcome {
                Ok(()) => {
                    me.set_current(Some(version.clone()));
                    Install::Done(format!("installed {version}"))
                }
                Err(e) => Install::Failed(e),
            };
            ctx.request_repaint();
        });
    }

    pub fn status(&self, ui: &mut Ui) {
        match self.install_state() {
            Install::Fetching { got, total } => {
                let said = if total > 0 {
                    format!("{} of {}", megabytes(got), megabytes(total))
                } else {
                    megabytes(got)
                };
                let total = (total > 0).then_some(total as f32);
                progress(ui, "download", got as f32, total, &said);
                return;
            }
            Install::Unpacking => {
                progress(ui, "install", 0.0, None, "unpacking");
                return;
            }
            Install::Done(msg) => return status(ui, true, &msg),
            Install::Failed(e) => return status(ui, false, &e),
            Install::Idle => {}
        }
        match self.state() {
            State::Unchecked => {}
            State::Checking => status(ui, true, "asking GitHub for the latest release"),
            State::Current(r) => status(ui, true, &format!("up to date, latest is {}", r.version)),
            State::Newer(r) => status(ui, true, &format!("{} is available", r.version)),
            State::Failed(e) => status(ui, false, &e),
        }
    }
}

fn judge(release: Release, current: Option<&str>) -> State {
    match current {
        Some(c) if !is_newer(&release.version, c) => State::Current(release),
        _ => State::Newer(release),
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent(AGENT)
        .build()
        .new_agent()
}

pub fn latest(repo: &str, cache: Option<&Path>) -> Result<Release, String> {
    latest_from(&format!("https://api.github.com/repos/{repo}/releases/latest"), repo, cache)
}

fn latest_from(url: &str, repo: &str, cache: Option<&Path>) -> Result<Release, String> {
    let file = cache.map(|dir| dir.join(format!("{}.json", repo.replace('/', "-"))));
    let saved = file.as_ref().and_then(|f| fs::read_to_string(f).ok());
    let saved = saved.as_deref().and_then(|s| s.split_once('\n'));
    let mut req = agent()
        .get(url)
        .config()
        .http_status_as_error(false)
        .build()
        .header("Accept", "application/vnd.github+json");
    if let Some((etag, _)) = saved {
        req = req.header("If-None-Match", etag);
    }
    if let Some(token) = ["GITHUB_TOKEN", "GH_TOKEN"].iter().find_map(|k| std::env::var(k).ok()) {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let mut res = req.call().map_err(|e| format!("{repo}: {e}"))?;
    let status = res.status().as_u16();
    let header =
        |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let etag = header("etag");
    let limited = header("x-ratelimit-remaining").as_deref() == Some("0");
    let reset = header("x-ratelimit-reset").and_then(|v| v.parse::<u64>().ok());
    match status {
        200 => {
            let body = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
            let release = parse(&body)?;
            if let (Some(f), Some(etag)) = (&file, etag) {
                if let Some(dir) = f.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let _ = fs::write(f, format!("{etag}\n{body}"));
            }
            Ok(release)
        }
        304 => parse(saved.map(|(_, body)| body).unwrap_or_default()),
        403 | 429 if limited || status == 429 => match saved {
            Some((_, body)) => parse(body),
            None => Err(rate_limited(reset)),
        },
        404 => Err(format!("{repo} has no release yet")),
        _ => Err(format!("{repo}: GitHub answered {status}")),
    }
}

fn rate_limited(reset: Option<u64>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match reset.map(|r| r.saturating_sub(now).div_ceil(60)) {
        Some(m) if m > 0 => {
            format!("GitHub's hourly limit is used up, checks work again in {m} min")
        }
        _ => "GitHub's hourly limit is used up, try again shortly".to_string(),
    }
}

pub fn parse(body: &str) -> Result<Release, String> {
    let j: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    let tag = text(&j["tag_name"]);
    let version = tag.trim_start_matches('v').to_string();
    if version.is_empty() {
        return Err("the release has no tag".into());
    }
    let assets = j["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(Asset {
                name: a["name"].as_str()?.to_string(),
                url: a["browser_download_url"].as_str()?.to_string(),
                bytes: a["size"].as_u64().unwrap_or(0),
                digest: a["digest"].as_str().map(str::to_string),
            })
        })
        .collect();
    Ok(Release {
        version,
        tag,
        page: text(&j["html_url"]),
        published: text(&j["published_at"]),
        assets,
    })
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some((l, lpre)), Some((c, cpre))) = (split(latest), split(current)) else {
        return false;
    };
    for i in 0..l.len().max(c.len()) {
        let a = l.get(i).copied().unwrap_or(0);
        let b = c.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    cpre.is_some() && lpre.is_none()
}

fn split(v: &str) -> Option<(Vec<u64>, Option<&str>)> {
    let v = v.trim().trim_start_matches('v');
    let (nums, pre) = match v.split_once(['-', '+']) {
        Some((n, p)) => (n, Some(p)),
        None => (v, None),
    };
    let nums = nums.split('.').map(|p| p.trim().parse::<u64>().ok()).collect::<Option<Vec<_>>>()?;
    Some((nums, pre))
}

pub fn version_in(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    line.split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| split(w).is_some())
        .map(str::to_string)
}

pub fn probe_version(exe: &Path) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    no_console(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < PROBE_LIMIT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    version_in(&out)
}

pub fn download(
    asset: &Asset,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(&asset.name);
    let res = agent().get(&asset.url).call().map_err(|e| format!("{}: {e}", asset.name))?;
    let mut reader = res.into_body().into_with_config().limit(u64::MAX).reader();
    let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{}: {e}", asset.name))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hash.update(&buf[..n]);
        got += n as u64;
        progress(got, asset.bytes);
    }
    file.flush().map_err(|e| e.to_string())?;
    if asset.bytes > 0 && got != asset.bytes {
        return Err(format!("{}: {got} bytes arrived of {} expected", asset.name, asset.bytes));
    }
    if let Some(want) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let have: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !have.eq_ignore_ascii_case(want) {
            return Err(format!("{} does not match its published sha256", asset.name));
        }
    }
    Ok(path)
}

pub fn unpack(file: &Path, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
    let open = || fs::File::open(file).map_err(|e| e.to_string());
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let gz = flate2::read::GzDecoder::new(open()?);
        tar::Archive::new(gz).unpack(into).map_err(|e| format!("{name}: {e}"))
    } else if name.ends_with(".zip") {
        zip::ZipArchive::new(open()?)
            .and_then(|mut z| z.extract(into))
            .map_err(|e| format!("{name}: {e}"))
    } else if name.ends_with(".dmg") {
        unpack_dmg(file, into)
    } else {
        let to = into.join(file.file_name().ok_or("no file name")?);
        fs::copy(file, &to).map_err(|e| e.to_string())?;
        executable(&to)
    }
}

fn unpack_dmg(dmg: &Path, into: &Path) -> Result<(), String> {
    let mount = into.join(".mount");
    fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run(Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mount)
        .arg(dmg))?;
    let copied = (|| {
        let apps: Vec<PathBuf> = fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .collect();
        if apps.is_empty() {
            return Err(format!("no .app in {}", dmg.display()));
        }
        for app in apps {
            let name = app.file_name().ok_or("unnamed .app")?;
            run(Command::new("ditto").arg(&app).arg(into.join(name)))?;
        }
        Ok(())
    })();
    let detached = run(Command::new("hdiutil").args(["detach", "-force"]).arg(&mount));
    let _ = fs::remove_dir(&mount);
    copied.and(detached)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    let hit = entries.iter().find(|p| p.is_file() && p.file_name().is_some_and(|n| n == name));
    hit.cloned().or_else(|| entries.iter().filter(|p| p.is_dir()).find_map(|d| find_file(d, name)))
}

pub fn replace(new: &Path, dest: &Path) -> Result<(), String> {
    let parent = dest.parent().ok_or("nowhere to install to")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = dest.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
    let staged = parent.join(format!(".{name}.new"));
    let old = parent.join(format!("{name}.old"));
    remove(&staged);
    if fs::rename(new, &staged).is_err() {
        copy_all(new, &staged)?;
    }
    if fs::symlink_metadata(dest).is_ok() {
        remove(&old);
        fs::rename(dest, &old).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    if let Err(e) = fs::rename(&staged, dest) {
        let _ = fs::rename(&old, dest);
        return Err(format!("{}: {e}", dest.display()));
    }
    remove(&old);
    Ok(())
}

pub fn running_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(strip_replaced(&fs::canonicalize(&exe).unwrap_or(exe)))
}

pub fn strip_replaced(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let text = text.strip_suffix(" (deleted)").unwrap_or(&text);
    let text = text.strip_suffix(".old").unwrap_or(text);
    PathBuf::from(text)
}

pub fn replace_self(new: &Path) -> Result<(), String> {
    replace(new, &running_exe()?)
}

pub fn relaunch() -> Result<(), String> {
    let exe = running_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn sweep(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.ends_with(".old") || (name.starts_with('.') && name.ends_with(".new")) {
            remove(&path);
        }
    }
}

fn remove(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = fs::remove_file(path);
        }
        Err(_) => {}
    }
}

fn copy_all(from: &Path, to: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(from).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = fs::read_link(from).map_err(|e| e.to_string())?;
            return std::os::unix::fs::symlink(target, to).map_err(|e| e.to_string());
        }
    }
    if meta.is_dir() {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    fs::copy(from, to).map(|_| ()).map_err(|e| format!("{}: {e}", from.display()))
}

fn executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(())
}

#[cfg(windows)]
fn no_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_console(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("egui_bench_update_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_later_version_is_newer() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.0"));
    }

    #[test]
    fn the_same_version_is_not_an_upgrade() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn a_pre_release_sorts_below_the_version_it_names() {
        assert!(!is_newer("0.2.0-rc1", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-rc1"));
        assert!(is_newer("0.2.0-rc1", "0.1.0"));
    }

    #[test]
    fn nonsense_is_not_an_upgrade() {
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[test]
    fn an_unknown_or_older_install_is_offered_the_release() {
        let r = parse(r#"{"tag_name":"v0.3.1"}"#).unwrap();
        assert!(matches!(judge(r.clone(), None), State::Newer(_)));
        assert!(matches!(judge(r.clone(), Some("0.3.0")), State::Newer(_)));
        assert!(matches!(judge(r, Some("0.3.1")), State::Current(_)));
    }

    #[test]
    fn a_release_reads_its_tag_page_and_assets() {
        let body = r#"{
            "tag_name": "v0.3.1",
            "html_url": "https://github.com/v0l/gcad/releases/tag/v0.3.1",
            "published_at": "2026-05-01T10:00:00Z",
            "assets": [{
                "name": "gcad-x86_64-unknown-linux-gnu.tar.gz",
                "browser_download_url": "https://example.invalid/gcad.tar.gz",
                "size": 1234,
                "digest": "sha256:abcd"
            }]
        }"#;
        let r = parse(body).unwrap();
        assert_eq!(r.version, "0.3.1");
        assert_eq!(r.tag, "v0.3.1");
        let a = r.asset("gcad-x86_64-unknown-linux-gnu.tar.gz").unwrap();
        assert_eq!(a.bytes, 1234);
        assert_eq!(a.digest.as_deref(), Some("sha256:abcd"));
    }

    #[test]
    fn a_body_that_is_not_a_release_is_an_error() {
        assert!(parse(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn the_version_is_read_from_a_version_line() {
        assert_eq!(version_in("gcad 0.3.1\n").as_deref(), Some("0.3.1"));
        assert_eq!(version_in("agentee 0.3.0 e71c1f4-dirty").as_deref(), Some("0.3.0"));
        assert_eq!(version_in("tool v1.2.0-rc1").as_deref(), Some("1.2.0-rc1"));
        assert_eq!(version_in("error: unexpected argument"), None);
    }

    #[test]
    fn replace_swaps_a_file_and_leaves_nothing_behind() {
        let dir = scratch("replace");
        let dest = dir.join("tool");
        fs::write(&dest, "old").unwrap();
        let new = dir.join("download");
        fs::write(&new, "new").unwrap();
        replace(&new, &dest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
        let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["tool"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replace_swaps_a_whole_folder() {
        let dir = scratch("replace_dir");
        let dest = dir.join("Tool.app");
        fs::create_dir_all(dest.join("Contents")).unwrap();
        fs::write(dest.join("Contents/old"), "").unwrap();
        let new = dir.join("unpacked/Tool.app");
        fs::create_dir_all(new.join("Contents")).unwrap();
        fs::write(new.join("Contents/new"), "").unwrap();
        replace(&new, &dest).unwrap();
        assert!(dest.join("Contents/new").exists());
        assert!(!dest.join("Contents/old").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sweep_clears_what_a_locked_replace_left() {
        let dir = scratch("sweep");
        for f in ["tool.exe", "tool.exe.old", ".tool.exe.new"] {
            fs::write(dir.join(f), "").unwrap();
        }
        sweep(&dir);
        let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["tool.exe"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_running_binary_that_was_replaced_is_still_named_by_its_own_path() {
        let at = |p: &str| strip_replaced(Path::new(p));
        assert_eq!(at("/opt/suite/bench-suite"), PathBuf::from("/opt/suite/bench-suite"));
        assert_eq!(at("/opt/suite/bench-suite.old"), PathBuf::from("/opt/suite/bench-suite"));
        assert_eq!(at("/opt/suite/bench-suite (deleted)"), PathBuf::from("/opt/suite/bench-suite"));
        assert_eq!(
            at("/opt/suite/bench-suite.old (deleted)"),
            PathBuf::from("/opt/suite/bench-suite")
        );
        assert_eq!(at("/opt/suite/tool.old.exe"), PathBuf::from("/opt/suite/tool.old.exe"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_second_update_after_the_first_does_not_look_for_the_old_file() {
        let dir = scratch("twice");
        let app = dir.join("app");
        fs::copy("/bin/sleep", &app).unwrap();
        let mut running = std::process::Command::new(&app).arg("20").spawn().unwrap();
        let seen = || fs::read_link(format!("/proc/{}/exe", running.id())).unwrap();
        assert_eq!(seen(), app.canonicalize().unwrap());

        let next = dir.join("next");
        fs::copy("/bin/sleep", &next).unwrap();
        replace(&next, &app).unwrap();
        assert!(app.exists(), "the new binary is at the original path");
        assert_ne!(seen(), app.canonicalize().unwrap(), "the running one now reports a stale path");
        assert_eq!(strip_replaced(&seen()), app.canonicalize().unwrap());
        let _ = running.kill();
        let _ = running.wait();
        fs::remove_dir_all(&dir).unwrap();
    }

    fn serve(body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        format!("http://{addr}/tool")
    }

    fn sha256(body: &[u8]) -> String {
        let hex: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256:{hex}")
    }

    fn finished(up: &Updater) -> Install {
        let started = Instant::now();
        while up.install_state().busy() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        up.install_state()
    }

    #[test]
    fn an_install_downloads_checks_and_hands_over_the_file() {
        let url = serve(b"new tool");
        let release = parse(r#"{"tag_name":"v0.2.0"}"#).unwrap();
        let asset = Asset { name: "tool".into(), url, bytes: 8, digest: Some(sha256(b"new tool")) };
        let up = Updater::new("v0l/tool", Some("0.1.0".into()));
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = got.clone();
        up.install(&egui::Context::default(), &release, &asset, move |file| {
            *lock(&seen) = fs::read(file).map_err(|e| e.to_string())?;
            Ok(())
        });
        assert_eq!(finished(&up), Install::Done("installed 0.2.0".into()));
        assert_eq!(*lock(&got), b"new tool");
        assert_eq!(up.current().as_deref(), Some("0.2.0"));
    }

    #[test]
    fn a_download_that_does_not_match_its_digest_is_refused() {
        let url = serve(b"tampered");
        let release = parse(r#"{"tag_name":"v0.2.0"}"#).unwrap();
        let asset = Asset { name: "tool".into(), url, bytes: 8, digest: Some(sha256(b"original")) };
        let up = Updater::new("v0l/tool", Some("0.1.0".into()));
        up.install(&egui::Context::default(), &release, &asset, |_| {
            panic!("a file that failed its check was handed over")
        });
        assert!(matches!(finished(&up), Install::Failed(e) if e.contains("sha256")));
        assert_eq!(up.current().as_deref(), Some("0.1.0"));
    }

    fn serve_github(etag: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().flatten().enumerate() {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let got = stream.read(&mut buf).unwrap_or(0);
                let asked = String::from_utf8_lossy(&buf[..got]).to_lowercase();
                let body = r#"{"tag_name":"v1.2.0"}"#;
                let reply = if n >= 2 {
                    "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else if asked.contains(&format!("if-none-match: {etag}")) {
                    "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        format!("http://{addr}/releases/latest")
    }

    #[test]
    fn a_saved_answer_is_reused_when_nothing_changed_or_the_limit_is_hit() {
        let dir = scratch("cache");
        let url = serve_github("\"abc\"");
        let first = latest_from(&url, "v0l/tool", Some(&dir)).unwrap();
        assert_eq!(first.version, "1.2.0");
        let unchanged = latest_from(&url, "v0l/tool", Some(&dir)).unwrap();
        assert_eq!(unchanged.version, "1.2.0");
        let limited = latest_from(&url, "v0l/tool", Some(&dir)).unwrap();
        assert_eq!(limited.version, "1.2.0");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_limit_with_nothing_saved_says_so() {
        let url = serve_github("\"abc\"");
        let _ = latest_from(&url, "v0l/tool", None);
        let _ = latest_from(&url, "v0l/tool", None);
        let e = latest_from(&url, "v0l/tool", None).unwrap_err();
        assert!(e.contains("hourly limit"), "{e}");
    }

    #[test]
    fn a_tarball_unpacks_and_its_binary_is_found() {
        let dir = scratch("unpack");
        let file = dir.join("tool-x86_64.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&file).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(2);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "tool-x86_64/tool", &b"hi"[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let into = dir.join("unpacked");
        unpack(&file, &into).unwrap();
        let found = find_file(&into, "tool").unwrap();
        assert_eq!(fs::read(found).unwrap(), b"hi");
        fs::remove_dir_all(&dir).unwrap();
    }
}
