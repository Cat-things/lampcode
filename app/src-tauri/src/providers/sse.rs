//! Incremental Server-Sent Events and NDJSON parsing. Network chunks can split
//! anywhere (mid-line, mid-UTF-8 character, mid-JSON), so both parsers buffer
//! raw bytes and only emit complete lines.

use bytes::Bytes;
use futures_util::{Stream, StreamExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Upper bound for one buffered line; a provider that never sends a newline
/// can't make us buffer forever.
const MAX_LINE: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
struct LineBuffer {
    buf: Vec<u8>,
}

impl LineBuffer {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        self.buf.extend_from_slice(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
        if self.buf.len() > MAX_LINE {
            return Err("stream line too long".into());
        }
        Ok(lines)
    }

    fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let mut line = std::mem::take(&mut self.buf);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(String::from_utf8_lossy(&line).into_owned())
    }
}

#[derive(Debug, Default)]
pub struct SseParser {
    lines: LineBuffer,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, String> {
        let mut out = Vec::new();
        for line in self.lines.push(chunk)? {
            self.line(&line, &mut out);
        }
        Ok(out)
    }

    /// End of stream: flush a final event that wasn't followed by a blank line.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if let Some(line) = self.lines.finish() {
            self.line(&line, &mut out);
        }
        self.dispatch(&mut out);
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line.starts_with(':') {
            return; // comment / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_string()),
            "event" => self.event = Some(value.to_string()),
            _ => {} // id, retry: not used
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        if self.data.is_empty() {
            self.event = None;
            return;
        }
        out.push(SseEvent {
            event: self.event.take(),
            data: self.data.join("\n"),
        });
        self.data.clear();
    }
}

/// Turn a byte stream into SSE events. Transport errors are passed through.
pub fn sse_events<S, E>(bytes: S) -> impl Stream<Item = Result<SseEvent, String>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display,
{
    framed(bytes, SseParser::default(), |p, c| p.push(c), |p| p.finish())
}

#[derive(Debug, Default)]
pub struct NdjsonParser {
    lines: LineBuffer,
}

impl NdjsonParser {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        Ok(self
            .lines
            .push(chunk)?
            .into_iter()
            .filter(|l| !l.trim().is_empty())
            .collect())
    }
    pub fn finish(&mut self) -> Vec<String> {
        self.lines
            .finish()
            .into_iter()
            .filter(|l| !l.trim().is_empty())
            .collect()
    }
}

pub fn ndjson_lines<S, E>(bytes: S) -> impl Stream<Item = Result<String, String>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display,
{
    framed(bytes, NdjsonParser::default(), |p, c| p.push(c), |p| p.finish())
}

fn framed<S, E, P, T>(
    bytes: S,
    parser: P,
    push: fn(&mut P, &[u8]) -> Result<Vec<T>, String>,
    finish: fn(&mut P) -> Vec<T>,
) -> impl Stream<Item = Result<T, String>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display,
    P: Send + 'static,
    T: Send + 'static,
{
    let bytes = Box::pin(bytes);
    futures_util::stream::unfold(
        (
            bytes,
            parser,
            std::collections::VecDeque::<Result<T, String>>::new(),
            false,
        ),
        move |(mut bytes, mut parser, mut queue, mut ended)| async move {
            loop {
                if let Some(item) = queue.pop_front() {
                    return Some((item, (bytes, parser, queue, ended)));
                }
                if ended {
                    return None;
                }
                match bytes.next().await {
                    Some(Ok(chunk)) => match push(&mut parser, &chunk) {
                        Ok(items) => queue.extend(items.into_iter().map(Ok)),
                        Err(e) => {
                            queue.push_back(Err(e));
                            ended = true;
                        }
                    },
                    Some(Err(e)) => {
                        queue.push_back(Err(e.to_string()));
                        ended = true;
                    }
                    None => {
                        queue.extend(finish(&mut parser).into_iter().map(Ok));
                        ended = true;
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::default();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.push(c).unwrap());
        }
        out.extend(p.finish());
        out
    }

    #[test]
    fn split_chunks_reassemble() {
        let whole = "data: {\"choices\":[{\"delta\":{\"content\":\"héllo\"}}]}\n\ndata: [DONE]\n\n".as_bytes();
        let expected = parse_all(&[whole]);
        assert_eq!(expected.len(), 2);
        // Split at every byte position, including inside the 2-byte 'é'.
        for i in 1..whole.len() {
            assert_eq!(parse_all(&[&whole[..i], &whole[i..]]), expected, "split at {i}");
        }
        let singles: Vec<&[u8]> = whole.chunks(1).collect();
        assert_eq!(parse_all(&singles), expected);
    }

    #[test]
    fn crlf_comments_event_names_and_multiline_data() {
        let evs =
            parse_all(&[b": ping\r\nevent: message_start\r\ndata: {\"a\":1}\r\n\r\ndata: line1\ndata: line2\n\n"]);
        assert_eq!(
            evs[0],
            SseEvent {
                event: Some("message_start".into()),
                data: "{\"a\":1}".into()
            }
        );
        assert_eq!(evs[1].data, "line1\nline2");
        assert_eq!(evs[1].event, None);
    }

    #[test]
    fn eof_without_blank_line_still_flushes() {
        let evs = parse_all(&[b"data: {\"x\":1}"]);
        assert_eq!(
            evs,
            vec![SseEvent {
                event: None,
                data: "{\"x\":1}".into()
            }]
        );
    }

    #[test]
    fn huge_line_is_an_error_not_a_hang() {
        let mut p = SseParser::default();
        let big = vec![b'a'; MAX_LINE + 10];
        assert!(p.push(&big).is_err());
    }

    #[test]
    fn ndjson_splits() {
        let mut p = NdjsonParser::default();
        let mut out = p.push(b"{\"a\":1}\n{\"b\"").unwrap();
        out.extend(p.push(b":2}\n\n{\"c\":3}").unwrap());
        out.extend(p.finish());
        assert_eq!(out, vec!["{\"a\":1}", "{\"b\":2}", "{\"c\":3}"]);
    }

    #[tokio::test]
    async fn stream_adapter_passes_transport_errors() {
        let chunks: Vec<Result<Bytes, String>> =
            vec![Ok(Bytes::from_static(b"data: 1\n\n")), Err("connection reset".into())];
        let out: Vec<_> = sse_events(futures_util::stream::iter(chunks)).collect().await;
        assert_eq!(out[0].as_ref().unwrap().data, "1");
        assert_eq!(out[1].as_ref().unwrap_err(), "connection reset");
        assert_eq!(out.len(), 2);
    }
}
