//! Helpers for text that carries structure inline:
//! - `<think>...</think>` reasoning inside normal content.
//! - The JSON-in-text tool protocol for models without native tool calling:
//!   the model writes `<tool_call>{"name": "...", "arguments": {...}}</tool_call>`.

use super::{ChatEvent, ToolSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Outside(String),
    Inside(String),
    /// The closing tag was seen.
    Closed,
}

/// Splits a stream of text deltas on an open/close tag pair. Text that could
/// be the start of a tag is held back until the next delta decides it.
#[derive(Debug)]
pub struct TagSplitter {
    open: &'static str,
    close: &'static str,
    inside: bool,
    pending: String,
}

impl TagSplitter {
    pub fn new(open: &'static str, close: &'static str) -> Self {
        Self {
            open,
            close,
            inside: false,
            pending: String::new(),
        }
    }

    pub fn push(&mut self, delta: &str) -> Vec<Segment> {
        self.pending.push_str(delta);
        let mut out = Vec::new();
        loop {
            let tag = if self.inside { self.close } else { self.open };
            if let Some(pos) = self.pending.find(tag) {
                let before: String = self.pending[..pos].to_string();
                self.emit(before, &mut out);
                self.pending.drain(..pos + tag.len());
                if self.inside {
                    out.push(Segment::Closed);
                }
                self.inside = !self.inside;
                continue;
            }
            // Keep the longest suffix that is a prefix of the tag.
            let keep = (1..tag.len().min(self.pending.len() + 1))
                .rev()
                .find(|&n| {
                    self.pending.is_char_boundary(self.pending.len() - n)
                        && tag.starts_with(&self.pending[self.pending.len() - n..])
                })
                .unwrap_or(0);
            let cut = self.pending.len() - keep;
            let text: String = self.pending.drain(..cut).collect();
            self.emit(text, &mut out);
            return out;
        }
    }

    pub fn finish(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        let rest = std::mem::take(&mut self.pending);
        self.emit(rest, &mut out);
        out
    }

    pub fn is_inside(&self) -> bool {
        self.inside
    }

    fn emit(&self, text: String, out: &mut Vec<Segment>) {
        if text.is_empty() {
            return;
        }
        out.push(if self.inside {
            Segment::Inside(text)
        } else {
            Segment::Outside(text)
        });
    }
}

/// Turns content deltas into text and reasoning events, pulling `<think>`
/// blocks out of the content.
#[derive(Debug)]
pub struct ThinkExtractor(TagSplitter);

impl Default for ThinkExtractor {
    fn default() -> Self {
        Self(TagSplitter::new("<think>", "</think>"))
    }
}

impl ThinkExtractor {
    pub fn push(&mut self, delta: &str) -> Vec<ChatEvent> {
        to_events(self.0.push(delta))
    }
    pub fn finish(&mut self) -> Vec<ChatEvent> {
        to_events(self.0.finish())
    }
}

fn to_events(segs: Vec<Segment>) -> Vec<ChatEvent> {
    segs.into_iter()
        .filter_map(|s| match s {
            Segment::Outside(t) => Some(ChatEvent::TextDelta(t)),
            Segment::Inside(t) => Some(ChatEvent::ReasoningDelta(t)),
            Segment::Closed => None,
        })
        .collect()
}

pub const TOOL_OPEN: &str = "<tool_call>";
pub const TOOL_CLOSE: &str = "</tool_call>";

/// System prompt addition that teaches the text tool protocol.
pub fn tool_instructions(tools: &[ToolSpec]) -> String {
    let mut s = String::from(
        "You can call tools. To call one, write a block like this on its own, and nothing else after it:\n\
         <tool_call>{\"name\": \"tool_name\", \"arguments\": {\"arg\": \"value\"}}</tool_call>\n\
         The content between the tags must be a single JSON object. You will get the result in the next message. \
         Available tools:\n",
    );
    for t in tools {
        s.push_str(&format!(
            "- {}: {}\n  arguments schema: {}\n",
            t.name, t.description, t.parameters
        ));
    }
    s
}

/// Render a past tool call back into text, for providers using this protocol.
pub fn render_call(name: &str, arguments: &str) -> String {
    let args: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::Value::Object(Default::default()));
    format!(
        "{TOOL_OPEN}{}{TOOL_CLOSE}",
        serde_json::json!({ "name": name, "arguments": args })
    )
}

/// Pulls `<tool_call>` blocks out of streamed text and turns them into tool
/// call events. Text outside the blocks passes through.
#[derive(Debug)]
pub struct TextToolParser {
    split: TagSplitter,
    body: String,
    next_index: u32,
}

impl Default for TextToolParser {
    fn default() -> Self {
        Self {
            split: TagSplitter::new(TOOL_OPEN, TOOL_CLOSE),
            body: String::new(),
            next_index: 0,
        }
    }
}

impl TextToolParser {
    pub fn push(&mut self, delta: &str) -> Vec<ChatEvent> {
        let segs = self.split.push(delta);
        self.handle(segs)
    }

    pub fn finish(&mut self) -> Vec<ChatEvent> {
        let segs = self.split.finish();
        let mut out = self.handle(segs);
        if self.split.is_inside() {
            // Unterminated block: try to parse it anyway, else show it as text.
            let body = std::mem::take(&mut self.body);
            out.extend(self.call_or_text(&body));
        }
        out
    }

    pub fn calls_made(&self) -> u32 {
        self.next_index
    }

    fn handle(&mut self, segs: Vec<Segment>) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        for s in segs {
            match s {
                Segment::Outside(t) => out.push(ChatEvent::TextDelta(t)),
                Segment::Inside(t) => self.body.push_str(&t),
                Segment::Closed => {
                    let body = std::mem::take(&mut self.body);
                    out.extend(self.call_or_text(&body));
                }
            }
        }
        out
    }

    fn call_or_text(&mut self, body: &str) -> Vec<ChatEvent> {
        #[derive(serde::Deserialize)]
        struct Call {
            name: String,
            #[serde(default)]
            arguments: serde_json::Value,
        }
        match serde_json::from_str::<Call>(body.trim()) {
            Ok(c) if !c.name.is_empty() => {
                let index = self.next_index;
                self.next_index += 1;
                let args = if c.arguments.is_null() {
                    serde_json::json!({})
                } else {
                    c.arguments
                };
                vec![ChatEvent::ToolCallDelta {
                    index,
                    id: Some(format!("call_text_{index}")),
                    name: Some(c.name),
                    args_delta: args.to_string(),
                }]
            }
            _ => vec![ChatEvent::TextDelta(format!("{TOOL_OPEN}{body}{TOOL_CLOSE}"))],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(evs: &[ChatEvent]) -> String {
        evs.iter()
            .filter_map(|e| {
                if let ChatEvent::TextDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    fn reasoning(evs: &[ChatEvent]) -> String {
        evs.iter()
            .filter_map(|e| {
                if let ChatEvent::ReasoningDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn think_tags_split_across_deltas() {
        let full = "<think>let me see</think>The answer is 4. a < b";
        for i in 0..=full.len() {
            if !full.is_char_boundary(i) {
                continue;
            }
            let mut x = ThinkExtractor::default();
            let mut evs = x.push(&full[..i]);
            evs.extend(x.push(&full[i..]));
            evs.extend(x.finish());
            assert_eq!(reasoning(&evs), "let me see", "split {i}");
            assert_eq!(text(&evs), "The answer is 4. a < b", "split {i}");
        }
    }

    #[test]
    fn text_tool_protocol_round_trips() {
        let rendered = render_call("read_file", r#"{"path":"src/main.rs"}"#);
        let stream = format!("Let me look.\n{rendered}");
        let mut p = TextToolParser::default();
        let mut evs = Vec::new();
        for ch in stream.chars() {
            evs.extend(p.push(&ch.to_string()));
        }
        evs.extend(p.finish());
        assert_eq!(text(&evs), "Let me look.\n");
        let call = evs.iter().find_map(|e| match e {
            ChatEvent::ToolCallDelta { name, args_delta, .. } => Some((name.clone().unwrap(), args_delta.clone())),
            _ => None,
        });
        let (name, args) = call.expect("tool call parsed");
        assert_eq!(name, "read_file");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&args).unwrap(),
            serde_json::json!({"path": "src/main.rs"})
        );
    }

    #[test]
    fn malformed_tool_block_stays_visible_as_text() {
        let mut p = TextToolParser::default();
        let mut evs = p.push("<tool_call>{oops</tool_call> ok");
        evs.extend(p.finish());
        assert_eq!(text(&evs), "<tool_call>{oops</tool_call> ok");
        assert_eq!(p.calls_made(), 0);
    }

    #[test]
    fn unterminated_tool_block_is_parsed_at_end() {
        let mut p = TextToolParser::default();
        let mut evs = p.push(r#"<tool_call>{"name":"ls","arguments":{}}"#);
        evs.extend(p.finish());
        assert_eq!(p.calls_made(), 1);
        assert_eq!(text(&evs), "");
    }

    #[test]
    fn instructions_list_tools() {
        let t = ToolSpec {
            name: "ls".into(),
            description: "list".into(),
            parameters: serde_json::json!({"type":"object"}),
        };
        let s = tool_instructions(&[t]);
        assert!(s.contains("<tool_call>") && s.contains("- ls: list"));
    }
}
