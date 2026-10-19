use super::config::Config;
use super::grants::{self, Grants};
use super::workspace::Workspace;
use super::{Backend, Cx, Desk, local, skillset, tools};
use serde_json::{Value, json};
use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

const PATIENCE: Duration = Duration::from_secs(600);

pub enum Turn {
    You(String),
    Said(String),
    Did { name: String, args: String, answer: Option<Result<String, String>> },
    Fault(String),
}

enum Update {
    Delta(String),
    Said,
    Calling { name: String, args: String },
    Answered(Result<String, String>),
    Fault(String),
    Done,
}

pub struct Chat {
    pub backend: Arc<dyn Backend>,
    pub grants: Grants,
    pub config: Config,
    pub turns: Vec<Turn>,
    pub draft: String,
    history: Vec<Value>,
    remembered: usize,
    updates: Option<Receiver<Update>>,
    stop: Arc<AtomicBool>,
    speaking: bool,
}

impl Chat {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            config: Config::load(backend.config_dir().as_deref()),
            grants: Grants::default(),
            backend,
            turns: Vec::new(),
            draft: String::new(),
            history: Vec::new(),
            remembered: 0,
            updates: None,
            stop: Arc::new(AtomicBool::new(false)),
            speaking: false,
        }
    }

    pub fn workspace_path(&self) -> std::path::PathBuf {
        self.config.workspace_path(self.backend.app())
    }

    pub fn busy(&self) -> bool {
        self.updates.is_some()
    }

    pub fn clear(&mut self) {
        for grant in self.grants.list() {
            self.grants.revoke(&grant.path);
        }
        self.turns.clear();
        self.history.clear();
        self.remembered = 0;
    }

    pub fn interrupt(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn ask(&mut self, text: &str, desk: Desk) {
        let text = text.trim();
        if text.is_empty() || self.busy() {
            return;
        }
        self.turns.push(Turn::You(text.to_string()));
        if let Some(fault) = self.config.fault() {
            self.turns.push(Turn::Fault(format!("{fault}: set one under Model")));
            self.remembered = self.turns.len();
            return;
        }
        self.history.push(json!({ "role": "user", "content": text }));
        let opened = Workspace::open(&self.workspace_path()).and_then(|w| {
            grants::vet(
                &w.root().display().to_string(),
                true,
                self.backend.config_dir().as_deref(),
                w.root(),
            )
            .map(|_| w)
        });
        let workspace = match opened {
            Ok(w) => w.with_grants(self.grants.clone()),
            Err(e) => {
                self.turns.push(Turn::Fault(format!("the workspace: {e}")));
                self.remembered = self.turns.len();
                self.history.pop();
                return;
            }
        };
        let (out, updates) = channel();
        self.updates = Some(updates);
        self.speaking = false;
        self.stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            backend: self.backend.clone(),
            config: self.config.clone(),
            workspace,
            history: self.history.clone(),
            desk,
            out,
            stop: self.stop.clone(),
        };
        std::thread::spawn(move || worker.run());
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.updates.as_ref() else { return };
        let mut done = false;
        for u in rx.try_iter().collect::<Vec<_>>() {
            match u {
                Update::Delta(text) => {
                    if self.speaking
                        && let Some(Turn::Said(s)) = self.turns.last_mut()
                    {
                        s.push_str(&text);
                        continue;
                    }
                    let text = text.trim_start();
                    if text.is_empty() {
                        continue;
                    }
                    self.turns.push(Turn::Said(text.to_string()));
                    self.speaking = true;
                }
                Update::Said => {
                    self.speaking = false;
                    if let Some(Turn::Said(s)) = self.turns.last_mut() {
                        s.truncate(s.trim_end().len());
                    }
                }
                Update::Calling { name, args } => {
                    self.speaking = false;
                    self.turns.push(Turn::Did { name, args, answer: None });
                }
                Update::Answered(r) => {
                    if let Some(Turn::Did { answer, .. }) = self
                        .turns
                        .iter_mut()
                        .rev()
                        .find(|t| matches!(t, Turn::Did { answer: None, .. }))
                    {
                        *answer = Some(r);
                    }
                }
                Update::Fault(e) => {
                    self.speaking = false;
                    self.turns.push(Turn::Fault(e));
                }
                Update::Done => done = true,
            }
        }
        if done {
            self.updates = None;
            self.speaking = false;
            self.remember();
        }
    }

    fn remember(&mut self) {
        for t in &self.turns[self.remembered..] {
            let content = match t {
                Turn::Said(s) => s.clone(),
                Turn::Did { name, args, answer: Some(Ok(_)) } => {
                    format!("called {name}({args}) and read the answer")
                }
                Turn::Did { name, args, answer: Some(Err(e)) } => {
                    format!("called {name}({args}): {e}")
                }
                _ => continue,
            };
            self.history.push(json!({ "role": "assistant", "content": content }));
        }
        self.remembered = self.turns.len();
    }
}

struct Worker {
    backend: Arc<dyn Backend>,
    config: Config,
    workspace: Workspace,
    history: Vec<Value>,
    desk: Desk,
    out: Sender<Update>,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
struct Completion {
    text: String,
    calls: Vec<Call>,
}

#[derive(Default, Clone)]
struct Call {
    id: String,
    name: String,
    args: String,
}

impl Worker {
    fn say(&self, u: Update) {
        let _ = self.out.send(u);
        self.desk.ring();
    }

    fn run(mut self) {
        if let Err(e) = self.exchange() {
            self.say(Update::Fault(e));
        }
        self.say(Update::Done);
    }

    fn exchange(&mut self) -> Result<(), String> {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(PATIENCE))
            .timeout_recv_body(Some(PATIENCE))
            .http_status_as_error(false)
            .user_agent(self.backend.user_agent())
            .build()
            .new_agent();
        let tools = tools::list(self.backend.as_ref());
        let steps = self.config.steps.max(1);
        for _ in 0..steps {
            if self.stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            let reply = self.complete(&agent, &tools)?;
            let mut message = json!({ "role": "assistant", "content": reply.text });
            if !reply.calls.is_empty() {
                message["tool_calls"] = Value::Array(
                    reply
                        .calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id,
                                "type": "function",
                                "function": { "name": c.name, "arguments": c.args },
                            })
                        })
                        .collect(),
                );
            }
            self.history.push(message);
            if reply.calls.is_empty() {
                self.say(Update::Said);
                return Ok(());
            }
            for call in reply.calls {
                let answer = self.run_tool(&call);
                let content = match &answer {
                    Ok(v) => v.clone(),
                    Err(e) => json!({ "error": e }).to_string(),
                };
                self.history.push(json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": content,
                }));
                self.say(Update::Answered(answer));
            }
        }
        Err(format!("stopped after {steps} tool calls"))
    }

    fn run_tool(&self, call: &Call) -> Result<String, String> {
        self.say(Update::Calling { name: call.name.clone(), args: call.args.clone() });
        let args: Value = if call.args.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&call.args).map_err(|e| format!("bad arguments: {e}"))?
        };
        let cx = Cx { workspace: &self.workspace, desk: &self.desk, stop: &self.stop };
        local::run(self.backend.as_ref(), &cx, &call.name, &args)
            .unwrap_or_else(|| Err(format!("no tool called {}", call.name)))
    }

    fn complete(&self, agent: &ureq::Agent, tools: &Value) -> Result<Completion, String> {
        let mut messages = vec![json!({ "role": "system", "content": self.brief() })];
        messages.extend(self.history.iter().cloned());
        let body = json!({
            "model": self.config.model,
            "messages": messages,
            "tools": tools,
            "stream": true,
        });
        let mut req = agent.post(self.config.endpoint()).header("content-type", "application/json");
        if !self.config.key.trim().is_empty() {
            req = req.header("authorization", format!("Bearer {}", self.config.key.trim()));
        }
        let resp = req.send(body.to_string()).map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.into_body().read_to_string().unwrap_or_default();
            return Err(format!("{status}: {}", text.trim()));
        }
        self.read_stream(std::io::BufReader::new(resp.into_body().into_reader()))
    }

    fn read_stream(&self, reader: impl BufRead) -> Result<Completion, String> {
        let mut reply = Completion::default();
        for line in reader.lines() {
            if self.stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            let line = line.map_err(|e| e.to_string())?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                self.take_delta(&v, &mut reply);
            }
        }
        Ok(reply)
    }

    fn take_delta(&self, v: &Value, reply: &mut Completion) {
        let Some(delta) = v.pointer("/choices/0/delta") else { return };
        if let Some(text) = delta.get("content").and_then(Value::as_str)
            && !text.is_empty()
        {
            reply.text.push_str(text);
            self.say(Update::Delta(text.to_string()));
        }
        let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else { return };
        for c in calls {
            let i = c.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            while reply.calls.len() <= i {
                reply.calls.push(Call::default());
            }
            let slot = &mut reply.calls[i];
            if let Some(id) = c.get("id").and_then(Value::as_str) {
                slot.id = id.to_string();
            }
            if let Some(f) = c.get("function") {
                if let Some(n) = f.get("name").and_then(Value::as_str) {
                    slot.name.push_str(n);
                }
                if let Some(a) = f.get("arguments").and_then(Value::as_str) {
                    slot.args.push_str(a);
                }
            }
        }
    }

    fn brief(&self) -> String {
        let skills = skillset::discover(
            &skillset::roots(self.workspace.root()),
            &self.backend.preferred_skills(),
        );
        let base = self.backend.brief(self.workspace.root(), &skills);
        match self.config.brief.trim() {
            "" => base,
            extra => format!("{base}\n\n{extra}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::toolkit;
    use super::*;
    use std::io::{Read, Write};

    pub struct Test(pub toolkit::Toolkit);

    impl Test {
        pub fn new() -> Self {
            Self(toolkit::Toolkit::new().with(toolkit::Tool::host(
                "list_things",
                "List the things the host has, with their state.",
                tools::object(json!({}), &[]),
            )))
        }
    }

    impl Backend for Test {
        fn app(&self) -> &'static str {
            "egui-bench-test"
        }

        fn brief(&self, workspace: &std::path::Path, skills: &[skillset::Skill]) -> String {
            format!("test brief {} {}", workspace.display(), skills.len())
        }

        fn welcome(&self) -> super::super::Welcome {
            super::super::Welcome {
                kicker: String::new(),
                title: String::new(),
                text: String::new(),
                placeholder: String::new(),
            }
        }

        fn toolkit(&self) -> &toolkit::Toolkit {
            &self.0
        }
    }

    fn chat() -> Chat {
        Chat::new(Arc::new(Test::new()))
    }

    fn worker(
        url: String,
    ) -> (Worker, Receiver<Update>, std::sync::mpsc::Receiver<super::super::Ask>) {
        let (out, updates) = channel();
        let (desk, asks) = Desk::new(&egui::Context::default());
        let dir = std::env::temp_dir().join(format!("bench-chat-ws-{}", std::process::id()));
        let w = Worker {
            backend: Arc::new(Test::new()),
            config: Config { url, model: "stub".into(), ..Config::default() },
            workspace: Workspace::open(&dir).unwrap(),
            history: vec![json!({ "role": "user", "content": "what things are there" })],
            desk,
            out,
            stop: Arc::new(AtomicBool::new(false)),
        };
        (w, updates, asks)
    }

    fn stub_server(replies: Vec<Vec<Value>>) -> (String, Receiver<Value>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
        let (seen, asked) = channel();
        std::thread::spawn(move || {
            let mut left = replies.into_iter();
            if let Some(Ok(mut s)) = listener.incoming().next() {
                loop {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match s.read(&mut byte) {
                            Ok(1) => head.push(byte[0]),
                            _ => return,
                        }
                    }
                    let text = String::from_utf8_lossy(&head).to_string();
                    let len = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0u8; len];
                    if s.read_exact(&mut body).is_err() {
                        return;
                    }
                    let _ = seen.send(serde_json::from_slice(&body).unwrap_or(Value::Null));
                    let Some(events) = left.next() else { return };
                    let mut sse = String::new();
                    for e in events {
                        sse.push_str(&format!("data: {e}\n\n"));
                    }
                    sse.push_str("data: [DONE]\n\n");
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
                        sse.len()
                    );
                    if s.write_all(head.as_bytes()).is_err() || s.write_all(sse.as_bytes()).is_err()
                    {
                        return;
                    }
                    let _ = s.flush();
                }
            }
        });
        (url, asked)
    }

    #[test]
    fn a_streamed_tool_call_is_reassembled() {
        let (w, _updates, _asks) = worker(String::new());
        let mut reply = Completion::default();
        let chunks = [
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"re"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"move","arguments":"{\"thing\":"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a\"}"}}]}}]}),
            json!({"choices":[{"delta":{"content":"moving"}}]}),
        ];
        for c in chunks {
            w.take_delta(&c, &mut reply);
        }
        assert_eq!(reply.text, "moving");
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(reply.calls[0].name, "remove");
        assert_eq!(reply.calls[0].args, "{\"thing\":\"a\"}");
    }

    #[test]
    fn two_calls_in_one_reply_keep_their_own_arguments() {
        let (w, _updates, _asks) = worker(String::new());
        let mut reply = Completion::default();
        w.take_delta(
            &json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"a","function":{"name":"list_things","arguments":"{}"}},
                {"index":1,"id":"b","function":{"name":"remove","arguments":"{\"thing\":\"a\"}"}}
            ]}}]}),
            &mut reply,
        );
        assert_eq!(reply.calls.len(), 2);
        assert_eq!(reply.calls[0].name, "list_things");
        assert_eq!(reply.calls[1].args, "{\"thing\":\"a\"}");
    }

    #[test]
    fn blank_lines_round_a_reply_are_not_drawn() {
        let (tx, rx) = channel();
        let mut chat = Chat { updates: Some(rx), ..chat() };
        for d in ["\n\n", "Done", " it.", "\n\n"] {
            let _ = tx.send(Update::Delta(d.to_string()));
        }
        let _ = tx.send(Update::Said);
        chat.poll();
        assert_eq!(chat.turns.iter().filter(|t| matches!(t, Turn::Said(_))).count(), 1);
        let Some(Turn::Said(said)) = chat.turns.last() else { panic!("no reply") };
        assert_eq!(said, "Done it.");
    }

    #[test]
    fn what_was_remembered_is_not_remembered_again() {
        let (tx, rx) = channel();
        let mut chat = Chat { updates: Some(rx), ..chat() };
        chat.history.push(json!({ "role": "user", "content": "one" }));
        chat.turns.push(Turn::You("one".into()));
        let _ = tx.send(Update::Delta("first".into()));
        let _ = tx.send(Update::Said);
        let _ = tx.send(Update::Done);
        chat.poll();
        let (tx, rx) = channel();
        chat.updates = Some(rx);
        chat.history.push(json!({ "role": "user", "content": "two" }));
        chat.turns.push(Turn::You("two".into()));
        let _ = tx.send(Update::Delta("second".into()));
        let _ = tx.send(Update::Said);
        let _ = tx.send(Update::Done);
        chat.poll();
        let said: Vec<&str> = chat.history.iter().filter_map(|m| m["content"].as_str()).collect();
        assert_eq!(said, ["one", "first", "two", "second"]);
    }

    #[test]
    fn a_tool_call_reaches_the_window_and_its_answer_goes_back() {
        let (url, asked) = stub_server(vec![
            vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"c1","function":{"name":"list_things","arguments":"{}"}}
            ]}}]})],
            vec![
                json!({"choices":[{"delta":{"content":"two things "}}]}),
                json!({"choices":[{"delta":{"content":"found"}}]}),
            ],
        ]);
        let (w, updates, asks) = worker(url);
        let window = std::thread::spawn(move || {
            let ask = asks.recv().expect("one tool call");
            assert!(ask.name == "list_things");
            let _ = ask.reply.send(Ok(json!({ "things": [{ "name": "a" }] })));
        });
        w.run();
        window.join().expect("the window answered");

        let mut said = String::new();
        let mut called = Vec::new();
        let mut faults = Vec::new();
        for u in updates.try_iter() {
            match u {
                Update::Delta(t) => said.push_str(&t),
                Update::Calling { name, .. } => called.push(name),
                Update::Fault(e) => faults.push(e),
                _ => {}
            }
        }
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(called, ["list_things"]);
        assert_eq!(said, "two things found");

        let first = asked.recv().expect("a first request");
        assert_eq!(first["stream"], true);
        assert_eq!(
            first["tools"].as_array().map(Vec::len),
            Some(9 + usize::from(super::super::sandbox::supported()))
        );
        assert_eq!(first["messages"][0]["role"], "system");
        let second = asked.recv().expect("a second request");
        let msgs = second["messages"].as_array().expect("messages");
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2]["tool_calls"][0]["function"]["name"], "list_things");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "c1");
        assert_eq!(msgs[3]["content"], "{\"things\":[{\"name\":\"a\"}]}");
    }

    #[test]
    fn a_server_that_refuses_is_reported() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
        std::thread::spawn(move || {
            if let Some(Ok(mut s)) = listener.incoming().next() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let body = "{\"error\":\"no such model\"}";
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        let (w, updates, _asks) = worker(url);
        w.run();
        let faults: Vec<String> = updates
            .try_iter()
            .filter_map(|u| match u {
                Update::Fault(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(faults.len(), 1);
        assert!(faults[0].contains("404"), "{}", faults[0]);
        assert!(faults[0].contains("no such model"), "{}", faults[0]);
    }

    #[test]
    fn a_workspace_that_holds_credentials_is_refused() {
        let mut c = chat();
        c.config.model = "m".into();
        c.config.workspace = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
        if c.config.workspace.is_empty() {
            return;
        }
        let (desk, _asks) = Desk::new(&egui::Context::default());
        c.ask("hello", desk);
        assert!(!c.busy(), "nothing was started");
        let Some(Turn::Fault(why)) = c.turns.last() else {
            panic!("no fault: {:?}", c.turns.len())
        };
        assert!(why.contains("workspace") && why.contains("home folder"), "{why}");
    }

    #[test]
    fn a_chat_with_no_model_says_so() {
        let mut chat = Chat { config: Config::default(), ..chat() };
        chat.config.model.clear();
        let (desk, _asks) = Desk::new(&egui::Context::default());
        chat.ask("what things are there", desk);
        assert_eq!(chat.turns.len(), 2);
        assert!(matches!(chat.turns[1], Turn::Fault(_)));
        assert!(!chat.busy());
    }
}
