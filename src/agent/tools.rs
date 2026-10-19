use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Spec {
    pub name: String,
    pub about: String,
    pub schema: Value,
}

impl Spec {
    pub fn new(name: &str, about: &str, schema: Value) -> Self {
        Self { name: name.to_string(), about: about.to_string(), schema }
    }
}

pub fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

pub fn string(about: &str) -> Value {
    json!({ "type": "string", "description": about })
}

pub fn integer(about: &str) -> Value {
    json!({ "type": "integer", "description": about })
}

pub fn builtin() -> Vec<Spec> {
    let path = string("A path inside the workspace, relative to it");
    vec![
        Spec::new(
            "load_skill",
            "Read an agent skill: the manual for a tool, with its file formats and workflow. \
             Load the skill before writing or editing that tool's files. With no name it lists \
             the skills. A skill may have other files, which `file` reads.",
            object(
                json!({
                    "name": string("The skill to load"),
                    "file": string("Another file in the skill, named when you loaded it"),
                }),
                &[],
            ),
        ),
        Spec::new("workspace", "Say which folder the file tools work in.", object(json!({}), &[])),
        Spec::new(
            "request_access",
            "Ask the user to share a folder or file outside the workspace, when the work needs \
             it: a project folder, a downloads folder, a parts library. They see the path and your \
             reason and choose read only, read and write, or no. Ask for the narrowest folder that \
             will do, and ask for read only unless you must change something. Once it is shared \
             the file tools take its absolute path, and bash can read it, and write to it if write \
             was granted, until the user clears the chat or stops sharing. Credential folders and \
             the whole home folder are refused without asking.",
            object(
                json!({
                    "path": string("The folder or file to share, absolute or starting with ~"),
                    "write": json!({ "type": "boolean", "description": "Whether you need to change it, default false" }),
                    "reason": string("One sentence the user reads when deciding"),
                }),
                &["path", "reason"],
            ),
        ),
        Spec::new(
            "list_files",
            "List the files and folders in the workspace, skipping .git and target.",
            object(
                json!({
                    "path": path,
                    "depth": integer("How many levels down, 1 to 6, default 2"),
                }),
                &[],
            ),
        ),
        Spec::new(
            "read_file",
            "Read a text file in the workspace with a line budget. A long file is read in \
             pages: the answer says which offset to ask for next.",
            object(
                json!({
                    "path": path,
                    "offset": integer("The line to start at, from 1"),
                    "limit": integer("How many lines"),
                }),
                &["path"],
            ),
        ),
        Spec::new(
            "write_file",
            "Create a file in the workspace, or replace one whole. Folders are made as needed. \
             To change part of a file use edit_file.",
            object(
                json!({ "path": path, "content": string("The whole file") }),
                &["path", "content"],
            ),
        ),
        Spec::new(
            "edit_file",
            "Replace text in a file in the workspace. `old` must match the file exactly and \
             appear once, unless all is true. Read the file first.",
            object(
                json!({
                    "path": path,
                    "old": string("The exact text to replace, with enough lines round it to be unique"),
                    "new": string("What to put there"),
                    "all": { "type": "boolean", "description": "Replace every occurrence" },
                }),
                &["path", "old", "new"],
            ),
        ),
        Spec::new(
            "search_files",
            "Find lines in the workspace's text files that contain some text, ignoring case.",
            object(
                json!({
                    "pattern": string("The text to look for"),
                    "path": string("A folder or file to search in, default the whole workspace"),
                }),
                &["pattern"],
            ),
        ),
        Spec::new(
            "bash",
            "Run a shell command in the workspace and read what it prints: pipes, globs, \
             redirects, python, git, make, anything installed. It runs in an OS sandbox: it can \
             read the system and the installed tools, and write only inside the workspace and a \
             scratch folder. The rest of the user's files, such as their home folder, are not \
             readable. The network is open. Use the file tools for editing; use this for \
             everything else.",
            object(
                json!({
                    "command": string("The command line, run by bash -c"),
                    "timeout_s": integer("Seconds before it is killed, default 120, at most 900"),
                }),
                &["command"],
            ),
        ),
    ]
}

pub const GUIDE: &str = "\
- Before writing or editing files in a format you were not taught, load its skill with \
load_skill. A skill is the manual: file formats, workflow and the mistakes to avoid. Do not write \
these formats from memory. load_skill with no name lists the skills, and a skill can have other \
files you read with its file argument.\n\
- Work in the workspace with list_files, read_file, search_files, write_file and edit_file. Read a \
file before you edit it. Paths are relative to the workspace. To use a folder outside it, such as a \
project or a library, call request_access with the folder and a reason: the user chooses read only, \
read and write, or no. Ask for the narrowest folder, and for read only unless you must change \
something.\n\
- bash runs a shell command in a sandbox: it reads the system and the installed tools, writes \
only inside the workspace and the folders the user shared for writing, and cannot see the rest of \
the user's files. Use it for pipes, globs, scripts and anything the other tools do not cover. It \
is not offered on a machine with no sandbox.\n\
- Write markdown, and put code and file contents in fenced blocks.";

pub fn offered(backend: &dyn super::Backend) -> Vec<Spec> {
    let mut specs = builtin();
    if !super::sandbox::supported() {
        specs.retain(|s| s.name != "bash");
    }
    specs.extend(backend.specs());
    specs
}

pub fn list(backend: &dyn super::Backend) -> Value {
    Value::Array(
        offered(backend)
            .into_iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.about,
                        "parameters": t.schema,
                    },
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_a_name_a_sentence_and_an_object_schema() {
        for t in builtin() {
            assert!(t.about.len() > 10, "{}", t.name);
            assert_eq!(t.schema["type"], "object", "{}", t.name);
            assert!(
                super::super::local::NAMES.contains(&t.name.as_str()),
                "{} goes nowhere",
                t.name
            );
        }
        for name in super::super::local::NAMES {
            assert!(builtin().iter().any(|t| t.name == name), "{name} is not offered");
        }
    }
}
