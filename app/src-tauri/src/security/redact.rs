//! Secret redaction. Every log line, every error shown in the UI and every
//! exported session goes through [`redact`].

use regex::{Captures, Regex};
use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::LazyLock;

/// Which part of a match gets replaced.
enum Target {
    /// The whole match is the secret.
    Whole,
    /// Keep everything before capture group N, replace group N.
    Group(usize),
}

struct Rule {
    kind: &'static str,
    re: Regex,
    target: Target,
}

fn rule(kind: &'static str, pattern: &str, target: Target) -> Rule {
    Rule {
        kind,
        re: Regex::new(pattern).expect("redaction pattern must compile"),
        target,
    }
}

/// Ordered: more specific formats run before generic ones, so `sk-ant-` is
/// labelled anthropic (not openai) and a JWT in a Bearer header is labelled jwt.
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    use Target::*;
    vec![
        rule(
            "private-key",
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            Whole,
        ),
        rule(
            "jwt",
            r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
            Whole,
        ),
        rule("anthropic", r"\bsk-ant-[A-Za-z0-9_-]{20,}", Whole),
        rule("openai", r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}", Whole),
        rule("groq", r"\bgsk_[A-Za-z0-9]{20,}", Whole),
        rule("nvidia", r"\bnvapi-[A-Za-z0-9_-]{20,}", Whole),
        rule("google", r"\bAIza[0-9A-Za-z_-]{35}", Whole),
        rule(
            "github",
            r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})",
            Whole,
        ),
        rule("xai", r"\bxai-[A-Za-z0-9]{20,}", Whole),
        rule("aws", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", Whole),
        rule("bearer", r"(?i)\b(bearer\s+)([A-Za-z0-9._~+/-]{16,}=*)", Group(2)),
        // Generic: 32+ chars of hex/base64-ish right after a key/token/secret word.
        rule(
            "secret",
            r#"(?i)\b([a-z0-9_.-]*(?:key|token|secret|passw(?:or)?d|auth)[a-z0-9_.-]*["']?\s*[:=]\s*["']?)([A-Za-z0-9+/_=-]{32,})"#,
            Group(2),
        ),
    ]
});

/// Replace anything that looks like a credential with `[REDACTED:<kind>]`.
/// Returns the input unchanged (borrowed) when nothing matches.
pub fn redact(s: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(s);
    for r in RULES.iter() {
        if !r.re.is_match(&out) {
            continue;
        }
        let replaced = match r.target {
            Target::Whole => r.re.replace_all(&out, format!("[REDACTED:{}]", r.kind).as_str()),
            Target::Group(g) => r.re.replace_all(&out, |c: &Captures| {
                let whole = c.get(0).unwrap();
                let secret = c.get(g).unwrap();
                format!(
                    "{}[REDACTED:{}]{}",
                    &whole.as_str()[..secret.start() - whole.start()],
                    r.kind,
                    &whole.as_str()[secret.end() - whole.start()..]
                )
            }),
        }
        .into_owned();
        out = Cow::Owned(replaced);
    }
    out
}

/// `io::Write` adapter that redacts everything written through it. Used as the
/// writer for the tracing subscriber so no log line can carry a key.
pub struct RedactingWriter<W: Write> {
    inner: W,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.inner.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// `MakeWriter` for tracing-subscriber: wraps every writer it hands out.
pub struct RedactingMakeWriter<M>(pub M);

impl<'a, M> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: tracing_subscriber::fmt::MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.0.make_writer())
    }
}

#[cfg(test)]
mod tests {
    use super::redact;

    // Fake keys are assembled at runtime so no key-shaped literal ever lands in
    // the source tree (keeps secret scanners and our own canary scan quiet).
    fn fake(prefix: &str, body_char: char, len: usize) -> String {
        let mut s = String::from(prefix);
        s.extend(std::iter::repeat_n(body_char, len / 2));
        s.extend("aB3xZ9qL7mN2pR5tV8wK4yH6".chars().cycle().take(len - len / 2));
        s
    }

    fn assert_redacted(input: &str, kind: &str) {
        let out = redact(input);
        assert!(
            out.contains(&format!("[REDACTED:{kind}]")),
            "expected [REDACTED:{kind}] in {out:?}"
        );
    }

    fn assert_gone(input: &str, secret: &str) {
        let out = redact(input);
        assert!(!out.contains(secret), "secret survived redaction: {out:?}");
    }

    #[test]
    fn openai_project_key() {
        let k = fake(&["sk-", "proj-"].concat(), 'A', 48);
        let line = format!("calling openai with {k}");
        assert_redacted(&line, "openai");
        assert_gone(&line, &k);
    }

    #[test]
    fn anthropic_key_is_not_misread_as_openai() {
        let k = fake(&["sk-", "ant-", "api03-"].concat(), 'B', 90);
        let line = format!("x-api-key: {k}");
        assert_redacted(&line, "anthropic");
        assert_gone(&line, &k);
    }

    #[test]
    fn groq_key() {
        let k = fake(&["gsk", "_"].concat(), 'C', 52);
        assert_redacted(&k, "groq");
    }

    #[test]
    fn nvidia_key() {
        let k = fake(&["nvapi", "-"].concat(), 'D', 64);
        assert_redacted(&format!("NVIDIA_API_KEY={k}"), "nvidia");
    }

    #[test]
    fn google_key() {
        let k = fake(&["AI", "za"].concat(), 'E', 35);
        assert_redacted(&format!("?key={k}&alt=sse"), "google");
    }

    #[test]
    fn github_token() {
        let k = fake(&["gh", "p_"].concat(), 'F', 36);
        assert_redacted(&k, "github");
    }

    #[test]
    fn xai_key() {
        let k = fake(&["xai", "-"].concat(), 'G', 80);
        assert_redacted(&k, "xai");
    }

    #[test]
    fn aws_access_key_id() {
        let k = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
        assert_redacted(&format!("aws_access_key_id = {k}"), "aws");
    }

    #[test]
    fn jwt() {
        let jwt = [
            "ey",
            "JhbGciOiJIUzI1NiJ9",
            ".",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            ".",
            "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        ]
        .concat();
        assert_redacted(&format!("Authorization: Bearer {jwt}"), "jwt");
        assert_gone(&jwt, "dozjgNryP4J3jVmNHl0w5N");
    }

    #[test]
    fn bearer_header_with_opaque_token() {
        let t = fake("", 'H', 40);
        let line = format!("Authorization: Bearer {t}");
        assert_redacted(&line, "bearer");
        assert_gone(&line, &t);
    }

    #[test]
    fn generic_hex_next_to_key_word() {
        let hex = "0123456789abcdef".repeat(3);
        let line = format!("api_key={hex}");
        assert_redacted(&line, "secret");
        assert_gone(&line, &hex);
    }

    #[test]
    fn generic_base64_in_json_token_field() {
        let b64 = fake("", 'Q', 44);
        let line = format!(r#"{{"token": "{b64}"}}"#);
        assert_redacted(&line, "secret");
        assert_gone(&line, &b64);
    }

    #[test]
    fn private_key_block() {
        let block = [
            "-----BEGIN ",
            "OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n",
            "-----END ",
            "OPENSSH PRIVATE KEY-----",
        ]
        .concat();
        assert_redacted(&block, "private-key");
        assert_gone(&block, "b3BlbnNzaC1rZXktdjEAAAAA");
    }

    #[test]
    fn multiple_secrets_in_one_line() {
        let a = fake(&["gsk", "_"].concat(), 'J', 52);
        let b = fake(&["nvapi", "-"].concat(), 'K', 64);
        let line = format!("{a} then {b}");
        let out = redact(&line);
        assert!(!out.contains(&a) && !out.contains(&b), "{out:?}");
    }

    #[test]
    fn plain_prose_is_untouched_and_borrowed() {
        let s = "The agent read 3 files and ran `cargo test` in 1.2s.";
        let out = redact(s);
        assert_eq!(out, s);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn bare_git_sha_is_untouched() {
        let s = "checked out 9fceb02d0ae598e95dc970b74767f19372d61af8 on main";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn uuid_is_untouched() {
        let s = "session 550e8400-e29b-41d4-a716-446655440000 started";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn tracing_log_lines_are_redacted() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(super::RedactingMakeWriter(move || sink.clone()))
            .finish();
        let k = fake(&["gsk", "_"].concat(), 'L', 52);
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(key = %k, "provider rejected request");
        });
        let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(logged.contains("provider rejected request"), "{logged:?}");
        assert!(!logged.contains(&k), "key leaked into log: {logged:?}");
    }

    #[test]
    fn short_sk_word_is_untouched() {
        let s = "the sk-learn docs and task-123";
        assert_eq!(redact(s), s);
    }
}
