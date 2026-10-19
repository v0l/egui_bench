pub mod chat;
pub mod config;
pub mod grants;
pub mod local;
pub mod pane;
pub mod sandbox;
pub mod settings;
pub mod skillset;
pub mod toolkit;
pub mod tools;
pub mod workspace;

use serde_json::Value;
use skillset::Skill;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use workspace::Workspace;

pub type Reply = Result<Value, String>;

pub type Poll<H> = Box<dyn FnMut(&mut H) -> Option<Reply>>;

const PATIENCE: Duration = Duration::from_secs(900);

pub struct Ask {
    pub name: String,
    pub args: Value,
    pub reply: Sender<Reply>,
}

#[derive(Clone)]
pub struct Desk {
    jobs: Sender<Ask>,
    ctx: egui::Context,
}

impl Desk {
    pub fn new(ctx: &egui::Context) -> (Self, Receiver<Ask>) {
        let (jobs, asks) = channel();
        (Self { jobs, ctx: ctx.clone() }, asks)
    }

    pub fn ring(&self) {
        self.ctx.request_repaint();
    }

    pub fn ask(&self, name: &str, args: Value, stop: &AtomicBool) -> Reply {
        let (reply, answer) = channel();
        let ask = Ask { name: name.to_string(), args, reply };
        self.jobs.send(ask).map_err(|_| "the window has gone".to_string())?;
        self.ring();
        let started = Instant::now();
        loop {
            match answer.recv_timeout(Duration::from_millis(200)) {
                Ok(r) => return r,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("the window dropped the request".into());
                }
                Err(RecvTimeoutError::Timeout) => {
                    if stop.load(Ordering::Relaxed) {
                        return Err("stopped".into());
                    }
                    if started.elapsed() > PATIENCE {
                        return Err(format!("the window did not answer in {PATIENCE:?}"));
                    }
                }
            }
        }
    }
}

pub struct Cx<'a> {
    pub workspace: &'a Workspace,
    pub desk: &'a Desk,
    pub stop: &'a AtomicBool,
}

impl Cx<'_> {
    pub fn ask(&self, name: &str, args: Value) -> Reply {
        self.desk.ask(name, args, self.stop)
    }
}

pub struct Welcome {
    pub kicker: String,
    pub title: String,
    pub text: String,
    pub placeholder: String,
}

pub trait Backend: Send + Sync + 'static {
    fn app(&self) -> &'static str;

    fn config_dir(&self) -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(self.app()))
    }

    fn user_agent(&self) -> String {
        format!("{}/{}", self.app(), env!("CARGO_PKG_VERSION"))
    }

    fn brief(&self, workspace: &Path, skills: &[Skill]) -> String;

    fn welcome(&self) -> Welcome;

    fn toolkit(&self) -> &toolkit::Toolkit {
        toolkit::empty()
    }

    fn specs(&self) -> Vec<tools::Spec> {
        self.toolkit().specs()
    }

    fn call(&self, name: &str, args: &Value, cx: &Cx) -> Option<Result<String, String>> {
        self.toolkit().call(name, args, cx)
    }

    fn preferred_skills(&self) -> Vec<String> {
        Vec::new()
    }

    fn tool_dirs(&self, _cx: &Cx) -> Vec<PathBuf> {
        Vec::new()
    }

    fn summary(&self, name: &str, args: &Value) -> Option<String> {
        self.toolkit().summary(name, args)
    }
}

pub enum Handled<H> {
    Now(Reply),
    Later(Poll<H>),
}

pub trait Host: Sized {
    fn agent(&mut self) -> &mut Agent<Self>;

    fn call(&mut self, ctx: &egui::Context, name: &str, args: &Value) -> Handled<Self>;
}

pub struct Pending<H> {
    reply: Sender<Reply>,
    poll: Poll<H>,
}

pub struct Agent<H> {
    pub chat: chat::Chat,
    pub desk: Desk,
    pub asks: Receiver<Ask>,
    pub open: bool,
    pub settings: Option<config::Config>,
    pub markdown: crate::markdown::Cache,
    pub pending: Vec<Pending<H>>,
}

impl<H> Agent<H> {
    pub fn new(ctx: &egui::Context, backend: Arc<dyn Backend>, open: bool) -> Self {
        let (desk, asks) = Desk::new(ctx);
        Self {
            chat: chat::Chat::new(backend),
            desk,
            asks,
            open,
            settings: None,
            markdown: crate::markdown::Cache::default(),
            pending: Vec::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        pane::show(ui, self);
    }

    pub fn show_with(&mut self, ui: &mut egui::Ui, extras: &mut pane::Extras) {
        pane::show_with(ui, self, extras);
    }

    pub fn edit_settings(&mut self) {
        self.settings = Some(self.chat.config.clone());
    }

    pub fn set_config(&mut self, config: config::Config) {
        self.chat.config = config;
    }

    pub fn poll(&mut self) {
        self.chat.poll();
    }

    pub fn live(&self) -> bool {
        self.chat.busy() || !self.pending.is_empty()
    }

    pub fn needs_you(&self) -> bool {
        !self.chat.grants.pending().is_empty()
    }
}

pub fn serve<H: Host>(host: &mut H, ctx: &egui::Context) {
    let asks: Vec<Ask> = host.agent().asks.try_iter().collect();
    let any = !asks.is_empty();
    for ask in asks {
        match host.call(ctx, &ask.name, &ask.args) {
            Handled::Now(r) => {
                let _ = ask.reply.send(r);
            }
            Handled::Later(poll) => host.agent().pending.push(Pending { reply: ask.reply, poll }),
        }
    }
    let pending = std::mem::take(&mut host.agent().pending);
    for mut p in pending {
        match (p.poll)(host) {
            Some(r) => {
                let _ = p.reply.send(r);
            }
            None => host.agent().pending.push(p),
        }
    }
    if any || !host.agent().pending.is_empty() {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}
