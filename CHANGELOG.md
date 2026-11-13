# Changelog

## Unreleased

- The UI-free agent (tools, workspace, grants, MCP client, language servers, sandbox, web, skills) moves to the `agent-core` crate in v0l/agentbench. `egui_bench::agent` re-exports it, so paths such as `egui_bench::agent::tools` and `egui_bench::agent::sandbox::run_helper` still work. `Desk::new` takes a repaint callback; `egui_bench::agent::desk(&ctx)` builds one for an egui context. The sandbox integration test now lives in `agent-core`.

## 0.9.7

- `chat` draws turns like agentbench: railed user blocks, plain markdown replies, folded thinking, copy and regenerate links, a live spinner, `header`, `fault`, `ask_box_with` and `transcript_with`.
- Add `rail_block` to the prelude.

## 0.9.6

- Add the `chat` feature: transcript bubbles, ask box, conversation list and context bar for a plain model chat, with no tokio or reqwest.
