//! OAuth 2 sign-in for plugins that talk to a web service (feature `oauth`).
//!
//! Run the sign-in from the plugin's setup program, then read tokens in the plugin:
//!
//! ```ignore
//! let spotify = OAuth::new("https://accounts.spotify.com/api/token", CLIENT_ID);
//! // setup program: browser sign-in with PKCE (no client secret shipped)
//! spotify.authorize_pkce("https://accounts.spotify.com/authorize", "user-read-playback-state", 47811, &[])?;
//! // plugin: a valid access token, refreshed when it is about to expire
//! let token = spotify.access_token()?;
//! ```
//!
//! Tokens live in the OS credential store through [`crate::secrets`] (item `oauth-token.json`): the
//! setup program hands the fresh sign-in to the plugin in a user-only file of that name, and the
//! plugin moves it into the store, as it does with a token file left by older builds.
//! When a refresh is refused, the host is asked to run setup again.

use crate::{log, open_url, secrets, Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long the user has to finish signing in.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const TOKEN_FILE: &str = "oauth-token.json";

/// A token endpoint plus the app's client id (and a client secret only for services that
/// require one even for desktop apps, such as Google).
#[derive(Clone)]
pub struct OAuth {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn random_b64(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| Error::msg(format!("no secure random source: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Percent-encode a query/form component (RFC 3986 unreserved characters stay as they are).
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi * 16 + lo);
                i += 2;
            }
            (b'+', _, _) => out.push(b' '),
            (c, _, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| decode(v)))
}

/// The error code a token endpoint returned: OAuth's `error`, or Twitch's `message`.
fn error_code(body: &Value) -> String {
    body["error"].as_str().or(body["message"].as_str()).unwrap_or("").to_string()
}

fn post_form(url: &str, form: &[(&str, &str)]) -> std::result::Result<Value, (u16, Value)> {
    match ureq::post(url).timeout(Duration::from_secs(20)).send_form(form) {
        Ok(r) => r.into_json().map_err(|e| (0, json!({"error": format!("invalid JSON: {e}")}))),
        Err(ureq::Error::Status(code, r)) => Err((code, r.into_json().unwrap_or(json!({})))),
        Err(e) => Err((0, json!({"error": e.to_string()}))),
    }
}

impl OAuth {
    pub fn new(token_url: &str, client_id: &str) -> Self {
        OAuth { token_url: token_url.into(), client_id: client_id.into(), client_secret: None }
    }

    pub fn with_secret(mut self, secret: &str) -> Self {
        self.client_secret = Some(secret.into()).filter(|s: &String| !s.is_empty());
        self
    }

    /// Sign in through the browser with PKCE and save the token. Listens on
    /// `http://127.0.0.1:{port}/callback`, which must be a registered redirect URI.
    /// `extra` adds authorize-URL parameters (e.g. `("access_type", "offline")` for Google).
    pub fn authorize_pkce(&self, auth_url: &str, scopes: &str, port: u16, extra: &[(&str, &str)]) -> Result<Value> {
        if self.client_id.is_empty() {
            return Err(Error::msg("This build has no OAuth client id configured"));
        }
        let verifier = random_b64(48)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_b64(16)?;
        let redirect = format!("http://127.0.0.1:{port}/callback");
        // Bind before opening the browser, so a busy port fails clearly instead of losing the redirect.
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| Error::msg(format!("Port {port} is busy, so sign-in can't receive its reply ({e}). Close the program using it and try again.")))?;
        let mut params = vec![
            ("client_id", self.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
            ("state", state.as_str()),
            ("scope", scopes),
        ];
        params.extend_from_slice(extra);
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
        open_url(&format!("{auth_url}?{}", query.join("&")))?;

        let code = wait_for_code(listener, &state)?;
        let mut form = vec![("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", redirect.as_str()), ("client_id", self.client_id.as_str()), ("code_verifier", verifier.as_str())];
        if let Some(s) = &self.client_secret {
            form.push(("client_secret", s));
        }
        let token = post_form(&self.token_url, &form).map_err(|(code, body)| Error::msg(format!("Sign-in failed at the token step (HTTP {code}): {}", error_code(&body))))?;
        self.save(&token)
    }

    /// Sign in with the device-code flow (for services without PKCE for desktop apps, e.g. Twitch):
    /// opens the service's activation page and waits until the user approves. `form` is what the
    /// device endpoint needs besides `client_id` (Twitch: `scopes`; Google: `scope`).
    pub fn device_flow(&self, device_url: &str, form: &[(&str, &str)]) -> Result<Value> {
        if self.client_id.is_empty() {
            return Err(Error::msg("This build has no OAuth client id configured"));
        }
        let mut req = vec![("client_id", self.client_id.as_str())];
        req.extend_from_slice(form);
        let dev = post_form(device_url, &req).map_err(|(code, body)| Error::msg(format!("Could not start sign-in (HTTP {code}): {}", error_code(&body))))?;
        let device_code = dev["device_code"].as_str().ok_or_else(|| Error::msg("Sign-in reply had no device code"))?.to_string();
        let page = dev["verification_uri_complete"].as_str().or(dev["verification_uri"].as_str()).or(dev["verification_url"].as_str()).unwrap_or("");
        let user_code = dev["user_code"].as_str().unwrap_or("");
        log(format!("device sign-in: open {page} and enter {user_code}"));
        eprintln!("Sign in at {page} with the code {user_code}");
        open_url(page)?;

        let mut interval = dev["interval"].as_u64().unwrap_or(5).max(1);
        let deadline = now() + dev["expires_in"].as_u64().unwrap_or(SIGN_IN_TIMEOUT.as_secs());
        let mut poll = vec![("client_id", self.client_id.as_str()), ("device_code", device_code.as_str()), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")];
        poll.extend_from_slice(form);
        if let Some(s) = &self.client_secret {
            poll.push(("client_secret", s));
        }
        while now() < deadline {
            std::thread::sleep(Duration::from_secs(interval));
            match post_form(&self.token_url, &poll) {
                Ok(token) => return self.save(&token),
                Err((_, body)) => match error_code(&body).as_str() {
                    "authorization_pending" => {}
                    "slow_down" => interval += 5,
                    "access_denied" => return Err(Error::msg("Sign-in was declined")),
                    other => return Err(Error::msg(format!("Sign-in failed: {other}"))),
                },
            }
        }
        Err(Error::msg("Sign-in timed out — run setup again"))
    }

    /// Store a token reply (adds `expires_at`; keeps the old refresh token when a refresh reply omits it).
    fn save(&self, reply: &Value) -> Result<Value> {
        let mut t = reply.clone();
        if t["access_token"].as_str().is_none() {
            return Err(Error::msg("The token reply had no access token"));
        }
        t["expires_at"] = json!(now() + t["expires_in"].as_u64().unwrap_or(3600));
        if t.get("refresh_token").map_or(true, Value::is_null) {
            if let Some(old) = self.load().and_then(|o| o["refresh_token"].as_str().map(String::from)) {
                t["refresh_token"] = json!(old);
            }
        }
        secrets::set(TOKEN_FILE, &t.to_string())?;
        Ok(t)
    }

    /// The saved token reply, if setup has run.
    pub fn load(&self) -> Option<Value> {
        secrets::get(TOKEN_FILE)?.parse().ok()
    }

    /// Whether a token has been saved (setup done).
    pub fn signed_in(&self) -> bool {
        self.load().is_some_and(|t| t["access_token"].is_string())
    }

    /// A usable access token, refreshed when it expires within a minute. A refused refresh
    /// (revoked access, expired refresh token) asks the host to run setup again.
    pub fn access_token(&self) -> Result<String> {
        let t = self.load().ok_or_else(|| {
            crate::mark_setup_required("Not signed in");
            Error::msg("Not signed in — run the plugin's setup")
        })?;
        if t["expires_at"].as_u64().unwrap_or(0) > now() + 60 {
            return Ok(t["access_token"].as_str().unwrap_or("").to_string());
        }
        Ok(self.refresh()?["access_token"].as_str().unwrap_or("").to_string())
    }

    /// Force a refresh (e.g. after an HTTP 401 from the API).
    pub fn refresh(&self) -> Result<Value> {
        let t = self.load().unwrap_or(json!({}));
        let Some(refresh) = t["refresh_token"].as_str() else {
            crate::mark_setup_required("Sign-in expired");
            return Err(Error::msg("Sign-in expired — run the plugin's setup again"));
        };
        let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", self.client_id.as_str())];
        if let Some(s) = &self.client_secret {
            form.push(("client_secret", s));
        }
        match post_form(&self.token_url, &form) {
            Ok(reply) => self.save(&reply),
            Err((400 | 401, body)) => {
                crate::mark_setup_required(format!("Sign-in was revoked ({})", error_code(&body)));
                Err(Error::msg("Sign-in expired — run the plugin's setup again"))
            }
            Err((code, body)) => Err(Error::msg(format!("Could not refresh the sign-in (HTTP {code}): {}", error_code(&body)))),
        }
    }
}

/// Accept connections on the loopback listener until the redirect with our `state` arrives.
fn wait_for_code(listener: TcpListener, state: &str) -> Result<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let state = state.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            // "GET /callback?code=…&state=… HTTP/1.1"; anything else (favicon…) gets a 404.
            let Some(query) = line.split_whitespace().nth(1).and_then(|p| p.strip_prefix("/callback?")) else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            };
            let outcome = if query_param(query, "state").as_deref() != Some(state.as_str()) {
                Err("The sign-in reply did not match this request".to_string())
            } else if let Some(e) = query_param(query, "error") {
                Err(if e == "access_denied" { "Sign-in was declined".to_string() } else { format!("Sign-in failed: {e}") })
            } else {
                query_param(query, "code").ok_or_else(|| "The sign-in reply had no code".to_string())
            };
            let message = if outcome.is_ok() { "Signed in. You can close this tab and go back to KnobDeck." } else { "Sign-in failed. Go back to KnobDeck for details." };
            let body = format!("<!doctype html><meta charset=utf-8><title>KnobDeck</title><body style=\"font:16px system-ui;margin:3em\">{message}</body>");
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = tx.send(outcome);
            return;
        }
    });
    match rx.recv_timeout(SIGN_IN_TIMEOUT) {
        Ok(Ok(code)) => Ok(code),
        Ok(Err(e)) => Err(Error::msg(e)),
        Err(_) => Err(Error::msg("Sign-in timed out — run setup again")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        let s = "a b&c=d/é~";
        assert_eq!(encode(s), "a%20b%26c%3Dd%2F%C3%A9~");
        assert_eq!(decode(&encode(s)), s);
        assert_eq!(decode("a+b%2"), "a b%2");
        assert_eq!(query_param("code=x%2Fy&state=s", "code").as_deref(), Some("x/y"));
        assert_eq!(query_param("code=x", "state"), None);
    }

    #[test]
    fn error_code_reads_oauth_and_twitch_shapes() {
        assert_eq!(error_code(&json!({"error": "authorization_pending"})), "authorization_pending");
        assert_eq!(error_code(&json!({"status": 400, "message": "authorization_pending"})), "authorization_pending");
    }

    /// The loopback receiver: ignores a stray request, checks `state`, returns the code.
    #[test]
    fn loopback_receives_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || wait_for_code(listener, "st4te"));
        let get = |path: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
            out
        };
        assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
        assert!(get("/callback?code=abc%2B1&state=st4te").contains("Signed in"));
        assert_eq!(h.join().unwrap().unwrap(), "abc+1");
    }

    /// An expired token is refreshed through the token endpoint; a reply without a refresh token keeps the old one.
    #[test]
    fn expired_token_refreshes_and_keeps_refresh_token() {
        let _guard = crate::DATA_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("kd-oauth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("KNOBDECK_PLUGIN_DATA", &dir);
        std::fs::write(dir.join(TOKEN_FILE), json!({"access_token": "old", "refresh_token": "r1", "expires_at": 1}).to_string()).unwrap();

        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://127.0.0.1:{}/token", server.local_addr().unwrap().port());
        let h = std::thread::spawn(move || {
            let (mut s, _) = server.accept().unwrap();
            // Read the whole request (headers, then Content-Length bytes) before replying,
            // or closing with unread data resets the connection.
            let mut r = BufReader::new(s.try_clone().unwrap());
            let (mut req, mut len) = (String::new(), 0);
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                req.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; len];
            std::io::Read::read_exact(&mut r, &mut body).unwrap();
            req.push_str(&String::from_utf8_lossy(&body));
            let body = r#"{"access_token":"new","expires_in":3600}"#;
            write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            req
        });
        let auth = OAuth::new(&url, "cid");
        assert_eq!(auth.access_token().unwrap(), "new");
        let req = h.join().unwrap();
        assert!(req.contains("grant_type=refresh_token") && req.contains("refresh_token=r1") && req.contains("client_id=cid"), "{req}");
        assert_eq!(auth.load().unwrap()["refresh_token"], "r1");
        // Now valid: no second request (the mock accepts only one).
        assert_eq!(auth.access_token().unwrap(), "new");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn loopback_rejects_wrong_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || wait_for_code(listener, "good"));
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET /callback?code=abc&state=evil HTTP/1.1\r\n\r\n").unwrap();
        assert!(h.join().unwrap().is_err());
    }
}
