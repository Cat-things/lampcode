//! Scriptable mock provider on 127.0.0.1:0. Each request pops the next
//! scripted reply and is recorded (method, path, headers, body) for asserts.

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Reply {
    /// Stream these raw chunks with a delay between them.
    Stream { content_type: &'static str, chunks: Vec<String>, delay_ms: u64, headers: Vec<(&'static str, String)> },
    /// A plain response.
    Status { status: u16, headers: Vec<(&'static str, String)>, body: String },
    /// Send headers, then stall forever after the first chunk.
    Stall { chunks: Vec<String> },
}

impl Reply {
    pub fn sse(events: &[&str]) -> Self {
        Reply::Stream {
            content_type: "text/event-stream",
            chunks: events.iter().map(|e| format!("data: {e}\n\n")).collect(),
            delay_ms: 0,
            headers: vec![],
        }
    }
    pub fn json(body: serde_json::Value) -> Self {
        Reply::Status { status: 200, headers: vec![("content-type", "application/json".into())], body: body.to_string() }
    }
}

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

#[derive(Default)]
struct Inner {
    replies: VecDeque<Reply>,
    seen: Vec<Recorded>,
}

#[derive(Clone)]
pub struct MockServer {
    pub addr: SocketAddr,
    inner: Arc<Mutex<Inner>>,
}

impl MockServer {
    pub async fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner::default()));
        let app = Router::new().fallback(handle).with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { addr, inner }
    }

    pub fn base(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.addr.port(), path)
    }

    pub fn push(&self, r: Reply) {
        self.inner.lock().unwrap().replies.push_back(r);
    }

    pub fn seen(&self) -> Vec<Recorded> {
        self.inner.lock().unwrap().seen.clone()
    }
}

async fn handle(State(inner): State<Arc<Mutex<Inner>>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let reply = {
        let mut g = inner.lock().unwrap();
        g.seen.push(Recorded {
            method,
            path: uri.path_and_query().map(|p| p.to_string()).unwrap_or_default(),
            headers,
            body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        });
        g.replies.pop_front()
    };
    let Some(reply) = reply else {
        return Response::builder().status(StatusCode::NOT_IMPLEMENTED).body(Body::from("no scripted reply")).unwrap();
    };
    match reply {
        Reply::Status { status, headers, body } => {
            let mut b = Response::builder().status(status);
            for (k, v) in headers {
                b = b.header(k, v);
            }
            b.body(Body::from(body)).unwrap()
        }
        Reply::Stream { content_type, chunks, delay_ms, headers } => {
            let s = futures_util::stream::unfold(chunks.into_iter(), move |mut it| async move {
                let c = it.next()?;
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Some((Ok::<_, std::io::Error>(Bytes::from(c)), it))
            });
            let mut b = Response::builder().status(200).header("content-type", content_type);
            for (k, v) in headers {
                b = b.header(k, v);
            }
            b.body(Body::from_stream(s)).unwrap()
        }
        Reply::Stall { chunks } => {
            let first = futures_util::stream::iter(chunks.into_iter().map(|c| Ok::<_, std::io::Error>(Bytes::from(c))));
            let never = futures_util::stream::pending::<Result<Bytes, std::io::Error>>();
            use futures_util::StreamExt;
            Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(first.chain(never)))
                .unwrap()
        }
    }
}
