# Changelog

## Unreleased

## 0.9.8

- `gix` is gated off wasm32, so web builds no longer pull it in.

## 0.9.7

- `chat` draws turns like agentbench: railed user blocks, plain markdown replies, folded thinking, copy and regenerate links, a live spinner, `header`, `fault`, `ask_box_with` and `transcript_with`.
- Add `rail_block` to the prelude.

## 0.9.6

- Add the `chat` feature: transcript bubbles, ask box, conversation list and context bar for a plain model chat, with no tokio or reqwest.
