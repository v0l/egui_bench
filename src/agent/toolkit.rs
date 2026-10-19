use super::Cx;
use super::tools::{Spec, object};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

pub type Run = Arc<dyn Fn(&Value, &Cx) -> Result<String, String> + Send + Sync>;
pub type Summary = Arc<dyn Fn(&Value) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub struct Tool {
    pub spec: Spec,
    run: Run,
    summary: Option<Summary>,
}

impl Tool {
    pub fn new(
        name: &str,
        about: &str,
        schema: Value,
        run: impl Fn(&Value, &Cx) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self { spec: Spec::new(name, about, schema), run: Arc::new(run), summary: None }
    }

    pub fn plain(
        name: &str,
        about: &str,
        run: impl Fn(&Value, &Cx) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self::new(name, about, object(serde_json::json!({}), &[]), run)
    }

    pub fn host(name: &str, about: &str, schema: Value) -> Self {
        Self::host_shaped(name, about, schema, |answer| answer)
    }

    pub fn host_shaped(
        name: &str,
        about: &str,
        schema: Value,
        shape: impl Fn(Value) -> Value + Send + Sync + 'static,
    ) -> Self {
        let call = name.to_string();
        Self::new(name, about, schema, move |args, cx| {
            cx.ask(&call, args.clone()).map(|v| shape(v).to_string())
        })
    }

    pub fn summarised_by(
        mut self,
        summary: impl Fn(&Value) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.summary = Some(Arc::new(summary));
        self
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }
}

pub fn empty() -> &'static Toolkit {
    static EMPTY: std::sync::OnceLock<Toolkit> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Toolkit::default)
}

#[derive(Clone, Default)]
pub struct Toolkit {
    tools: Vec<Tool>,
}

impl Toolkit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn push(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    pub fn extend(&mut self, tools: impl IntoIterator<Item = Tool>) {
        self.tools.extend(tools);
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.iter().map(|t| t.name())
    }

    pub fn specs(&self) -> Vec<Spec> {
        self.tools.iter().map(|t| t.spec.clone()).collect()
    }

    pub fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name() == name)
    }

    pub fn call(&self, name: &str, args: &Value, cx: &Cx) -> Option<Result<String, String>> {
        self.find(name).map(|t| (t.run)(args, cx))
    }

    pub fn summary(&self, name: &str, args: &Value) -> Option<String> {
        self.find(name).and_then(|t| t.summary.as_ref()).and_then(|s| s(args))
    }

    pub fn problems(&self, reserved: &[&str]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for tool in &self.tools {
            let name = tool.name();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.push(format!("{name:?} is not a name a model can call"));
            }
            if reserved.contains(&name) {
                out.push(format!("{name} shadows a built-in tool"));
            }
            if !seen.insert(name.to_string()) {
                out.push(format!("{name} is registered twice"));
            }
            if tool.spec.about.trim().len() < 10 {
                out.push(format!("{name} has no description a model can choose it by"));
            }
            if tool.spec.schema.get("type").and_then(Value::as_str) != Some("object") {
                out.push(format!("{name} has no object schema"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::string;
    use crate::agent::workspace::Workspace;
    use crate::agent::{Desk, local};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    fn echo() -> Tool {
        Tool::new(
            "echo",
            "Say back what was said, in capitals.",
            object(json!({ "text": string("What to say") }), &["text"]),
            |args, _| Ok(args["text"].as_str().unwrap_or_default().to_uppercase()),
        )
        .summarised_by(|args| args["text"].as_str().map(String::from))
    }

    fn cx_parts() -> (Workspace, Desk, AtomicBool) {
        let dir = std::env::temp_dir().join(format!("toolkit-{}", std::process::id()));
        let (desk, _asks) = Desk::new(&egui::Context::default());
        (Workspace::open(&dir).unwrap(), desk, AtomicBool::new(false))
    }

    #[test]
    fn a_registered_tool_is_called_by_name_and_summarised_by_its_own_rule() {
        let kit = Toolkit::new().with(echo()).with(Tool::plain(
            "ping",
            "Answer that the host is there.",
            |_, _| Ok("pong".into()),
        ));
        let (ws, desk, stop) = cx_parts();
        let cx = Cx { workspace: &ws, desk: &desk, stop: &stop };
        assert_eq!(kit.call("echo", &json!({ "text": "hi" }), &cx), Some(Ok("HI".to_string())));
        assert_eq!(kit.call("ping", &json!({}), &cx), Some(Ok("pong".to_string())));
        assert_eq!(kit.call("nope", &json!({}), &cx), None);
        assert_eq!(kit.summary("echo", &json!({ "text": "hi" })).as_deref(), Some("hi"));
        assert_eq!(kit.summary("ping", &json!({})), None);
        assert_eq!(kit.len(), 2);
        assert_eq!(kit.names().collect::<Vec<_>>(), ["echo", "ping"]);
        assert_eq!(kit.specs()[0].schema["type"], "object");
    }

    #[test]
    fn a_host_tool_forwards_its_name_and_arguments_to_the_window() {
        let kit = Toolkit::new().with(Tool::host(
            "tune",
            "Point the receiver at a frequency in MHz.",
            object(json!({ "mhz": json!({ "type": "number" }) }), &["mhz"]),
        ));
        let dir = std::env::temp_dir().join(format!("toolkit-host-{}", std::process::id()));
        let (desk, asks) = Desk::new(&egui::Context::default());
        let ws = Workspace::open(&dir).unwrap();
        let stop = AtomicBool::new(false);
        let window = std::thread::spawn(move || {
            let ask = asks.recv().expect("a tool call");
            assert_eq!(ask.name, "tune");
            assert_eq!(ask.args, json!({ "mhz": 145.5 }));
            let _ = ask.reply.send(Ok(json!({ "center_hz": 145_500_000.0 })));
        });
        let cx = Cx { workspace: &ws, desk: &desk, stop: &stop };
        let got = kit.call("tune", &json!({ "mhz": 145.5 }), &cx).unwrap();
        window.join().unwrap();
        assert_eq!(got.unwrap(), "{\"center_hz\":145500000.0}");
    }

    #[test]
    fn a_host_answer_can_be_reshaped_before_the_model_reads_it() {
        let kit = Toolkit::new().with(Tool::host_shaped(
            "screenshot",
            "A PNG of the interface as it is now.",
            object(json!({}), &[]),
            |mut answer| {
                if let Some(o) = answer.as_object_mut()
                    && o.remove("png_base64").is_some()
                {
                    o.insert("note".into(), json!("the picture is in the window"));
                }
                answer
            },
        ));
        let dir = std::env::temp_dir().join(format!("toolkit-shape-{}", std::process::id()));
        let (desk, asks) = Desk::new(&egui::Context::default());
        let ws = Workspace::open(&dir).unwrap();
        let stop = AtomicBool::new(false);
        let window = std::thread::spawn(move || {
            let ask = asks.recv().unwrap();
            let _ = ask.reply.send(Ok(json!({ "png_base64": "AAAA", "width": 10 })));
        });
        let cx = Cx { workspace: &ws, desk: &desk, stop: &stop };
        let got = kit.call("screenshot", &json!({}), &cx).unwrap().unwrap();
        window.join().unwrap();
        assert!(!got.contains("AAAA"), "{got}");
        assert!(got.contains("the picture is in the window") && got.contains("\"width\":10"));
    }

    #[test]
    fn a_toolkit_that_would_confuse_a_model_is_reported() {
        let ok = Toolkit::new().with(echo());
        assert!(ok.problems(&[]).is_empty());

        let bad = Toolkit::new()
            .with(echo())
            .with(echo())
            .with(Tool::plain("read_file", "Shadows a built-in.", |_, _| Ok(String::new())))
            .with(Tool::plain("has space", "Cannot be called by name.", |_, _| Ok(String::new())))
            .with(Tool::plain("terse", "x", |_, _| Ok(String::new())))
            .with(Tool::new("flat", "Has an array schema.", json!({ "type": "array" }), |_, _| {
                Ok(String::new())
            }));
        let problems = bad.problems(&local::NAMES);
        for want in [
            "registered twice",
            "shadows a built-in",
            "not a name",
            "no description",
            "no object schema",
        ] {
            assert!(problems.iter().any(|p| p.contains(want)), "{want} missing in {problems:?}");
        }
    }
}
