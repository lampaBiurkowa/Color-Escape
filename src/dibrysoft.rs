//! Unlocking achievements through the Dibrysoft launcher.
//!
//! The launcher puts the endpoint and a per-session key in our environment when it starts the
//! game. If they are missing we were started outside the launcher, and every unlock quietly turns
//! into a no-op - achievements are a bonus, never a precondition for playing.
//!
//! Only keys declared in `.ndib/achievements.json` are accepted; anything else comes back `404`,
//! so the constants below and that file have to agree. Posting the same key twice is safe on the
//! launcher's side, and we additionally remember what this process already sent, which is what
//! lets [`Tracker::record_frame`] be called on every frame without thinking about it.
//!
//! Posting happens on a background thread so no frame ever waits on HTTP. The flip side is that
//! the process has to call [`flush`] before it leaves, or an unlock that fired on the last frame
//! of a run dies with the process.
//!
//! The request is written straight onto a `TcpStream`: it always goes to the launcher on this
//! machine, over plain HTTP, with a body of one short field - not worth a dependency.

use std::collections::HashSet;
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);
const KEY_HEADER: &str = "X-Dibrysoft-Award-Key";

const FIRST_BRUSH: &str = "first-brush";
const FIRST_AID: &str = "first-aid";
const FIELD_MEDIC: &str = "field-medic";
const HALF_MINUTE: &str = "half-minute";
const FULL_MINUTE: &str = "full-minute";
const TWO_MINUTE_ESCAPE: &str = "two-minute-escape";
const NOT_A_SCRATCH: &str = "not-a-scratch";
const STOCKED_UP: &str = "stocked-up";
const CROWD_CONTROL: &str = "crowd-control";
const RECORD_BREAKER: &str = "record-breaker";
const FULL_SPECTRUM: &str = "full-spectrum";
const SHORT_FUSE: &str = "short-fuse";

/// What each achievement asks for. These mirror the descriptions in
/// `.ndib/achievements.json`; change one and change the other.
const HALF_MINUTE_AT: f32 = 30.0;
const FULL_MINUTE_AT: f32 = 60.0;
const TWO_MINUTES_AT: f32 = 120.0;
const NOT_A_SCRATCH_AT: f32 = 30.0;
const STOCKED_UP_HP: i32 = 25;
const CROWD_CONTROL_BALLS: usize = 20;
const FIELD_MEDIC_BOXES: u32 = 3;
const SHORT_FUSE_WITHIN: f32 = 10.0;

struct Endpoint {
    /// `host:port` to connect to, and the path to post at.
    authority: String,
    path: String,
    /// Session key, valid only while this process lives.
    key: String,
}

/// Endpoint and key, read from the environment once. `None` means unlocks are unavailable.
fn endpoint() -> Option<&'static Endpoint> {
    static ENDPOINT: OnceLock<Option<Endpoint>> = OnceLock::new();

    ENDPOINT
        .get_or_init(|| {
            // Read the address rather than hardcoding it; the port may change.
            let url = env::var("DIBRYSOFT_ACHIEVEMENT_URL").ok()?;
            let key = env::var("DIBRYSOFT_GAME_KEY")
                .or_else(|_| env::var("DIBRYSOFT_AWARD_KEY"))
                .ok()?;
            let (authority, path) = split_url(&url)?;
            Some(Endpoint {
                authority,
                path,
                key,
            })
        })
        .as_ref()
}

/// `http://127.0.0.1:25220/achievement` into `("127.0.0.1:25220", "/achievement")`.
fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.trim().strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    // A URL without a port means the default one.
    let authority = match authority.contains(':') {
        true => authority.to_string(),
        false => format!("{authority}:80"),
    };
    Some((authority, path.to_string()))
}

/// `true` the first time this process sees `key`, `false` on every repeat.
fn claim(key: &'static str) -> bool {
    sent().lock().unwrap_or_else(|e| e.into_inner()).insert(key)
}

/// Let `key` be posted again - for a refusal that may have been a passing problem.
fn release(key: &'static str) {
    sent().lock().unwrap_or_else(|e| e.into_inner()).remove(key);
}

fn sent() -> &'static Mutex<HashSet<&'static str>> {
    static SENT: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    SENT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn in_flight() -> &'static Mutex<Vec<JoinHandle<()>>> {
    static IN_FLIGHT: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(Vec::new()))
}

/// Say once, at startup, whether unlocks are going anywhere. Optional - everything below works
/// without it - but it saves guessing when the game was started outside the launcher.
pub fn init() {
    match endpoint() {
        Some(ep) => println!(
            "Achievements: posting to http://{}{}",
            ep.authority, ep.path
        ),
        None => println!("Achievements: unavailable (not started by the Dibrysoft launcher)"),
    }
}

/// Wait for the posts already on their way. Call this before leaving the process, otherwise an
/// unlock that fired on the last frame of a run never reaches the launcher. Bounded by the HTTP
/// timeout, so it cannot hang the shutdown for long.
pub fn flush() {
    let handles: Vec<_> = {
        let mut in_flight = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *in_flight)
    };
    for handle in handles {
        let _ = handle.join();
    }
}

/// Unlock an achievement. Cheap to call from the game loop: it never blocks, never panics, and
/// posts each key at most once per run.
fn unlock(key: &'static str) {
    if endpoint().is_none() || !claim(key) {
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("dibrysoft-achievement".to_string())
        .spawn(move || post(key));

    match spawned {
        Ok(handle) => {
            let mut in_flight = in_flight().lock().unwrap_or_else(|e| e.into_inner());
            in_flight.retain(|h| !h.is_finished());
            in_flight.push(handle);
        }
        // No thread, no post - let the condition try again on a later frame.
        Err(e) => {
            release(key);
            eprintln!("achievement '{key}' not sent: no thread for it ({e})");
        }
    }
}

fn post(key: &'static str) {
    let Some(ep) = endpoint() else { return };

    match send(ep, key) {
        Ok(200) => {}
        // 400 and 404 mean the request itself is wrong - the key is misspelt or missing from
        // .ndib/achievements.json - so retrying it would only repeat the mistake. 401 means
        // unlocking is over for this session. Anything else may have been a passing problem,
        // so let the condition post it again.
        Ok(status) => {
            if !matches!(status, 400 | 401 | 404) {
                release(key);
            }
            eprintln!("achievement '{key}' refused: {status}");
        }
        Err(e) => {
            release(key);
            eprintln!("achievement '{key}' not sent: launcher unreachable ({e})");
        }
    }
}

/// One request, one response status. No `Origin` header - the launcher answers `403` to anything
/// that looks like it came from a browser.
fn send(ep: &Endpoint, key: &str) -> std::io::Result<u16> {
    // Achievement keys are the constants above: lowercase, digits and dashes, nothing JSON
    // would need escaped.
    debug_assert!(
        key.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "achievement key '{key}' is not a plain kebab-case key"
    );
    let body = format!("{{\"key\":\"{key}\"}}");
    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         {KEY_HEADER}: {session_key}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {length}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        path = ep.path,
        host = ep.authority,
        session_key = ep.key,
        length = body.len(),
    );

    let mut stream = TcpStream::connect(&ep.authority)?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    // The status line is all we need; the body only repeats what we asked for.
    let mut head = Vec::new();
    let mut chunk = [0u8; 256];
    while !head.windows(2).any(|w| w == b"\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e),
        }
    }

    let head = String::from_utf8_lossy(&head);
    head.split_whitespace()
        .nth(1)
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("no status in the launcher's reply: {head:?}"),
            )
        })
}

/// What the run looks like on one frame, as far as the achievements care.
pub struct Frame {
    pub survival_time: f32,
    pub hp: i32,
    /// Hits taken this run.
    pub hits: i32,
    pub ball_count: usize,
}

/// Per-run state for the achievements that are about a sequence of events rather than a single
/// frame. Everything else is stateless - the "already sent" set above does the deduplication.
pub struct Tracker {
    /// Health boxes picked up in the current run.
    health_boxes: u32,
    /// Which player colours the menu has shown, by index into `Player::colors`.
    colors_seen: HashSet<usize>,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker {
            health_boxes: 0,
            colors_seen: HashSet::new(),
        }
    }

    /// Feed this when a new run starts, to forget what the last one collected.
    pub fn record_run_start(&mut self) {
        self.health_boxes = 0;
    }

    /// Feed this on every frame of a run.
    pub fn record_frame(&mut self, frame: Frame) {
        if frame.hits >= 1 {
            unlock(FIRST_BRUSH);
        }
        if frame.survival_time >= HALF_MINUTE_AT {
            unlock(HALF_MINUTE);
        }
        if frame.survival_time >= FULL_MINUTE_AT {
            unlock(FULL_MINUTE);
        }
        if frame.survival_time >= TWO_MINUTES_AT {
            unlock(TWO_MINUTE_ESCAPE);
        }
        if frame.hits == 0 && frame.survival_time >= NOT_A_SCRATCH_AT {
            unlock(NOT_A_SCRATCH);
        }
        if frame.hp >= STOCKED_UP_HP {
            unlock(STOCKED_UP);
        }
        if frame.ball_count >= CROWD_CONTROL_BALLS {
            unlock(CROWD_CONTROL);
        }
    }

    /// Feed this once per health box the player walks into.
    pub fn record_health_box(&mut self) {
        self.health_boxes += 1;
        unlock(FIRST_AID);
        if self.health_boxes >= FIELD_MEDIC_BOXES {
            unlock(FIELD_MEDIC);
        }
    }

    /// Feed this with the colour the menu is showing and how many there are to see.
    pub fn record_color_shown(&mut self, index: usize, total: usize) {
        self.colors_seen.insert(index);
        if self.colors_seen.len() >= total {
            unlock(FULL_SPECTRUM);
        }
    }

    /// Feed this when the run ends. `beat_high_score` is about the score that was already on
    /// record, so a first-ever run is not a broken record.
    pub fn record_game_over(&mut self, survival_time: f32, beat_high_score: bool) {
        if beat_high_score {
            unlock(RECORD_BREAKER);
        }
        if survival_time <= SHORT_FUSE_WITHIN {
            unlock(SHORT_FUSE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Stands in for the launcher: takes one request, hands back what a real unlock replies with.
    fn mock_launcher() -> (u16, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();

            let mut request = Vec::new();
            let mut chunk = [0u8; 512];
            // Read until the JSON body has arrived; headers and body may come separately.
            while !request.ends_with(b"}") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&chunk[..n]),
                }
            }

            let body = br#"{"key":"first-aid","name":"First Aid","alreadyUnlocked":false}"#;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();

            String::from_utf8_lossy(&request).into_owned()
        });

        (port, handle)
    }

    #[test]
    fn posts_an_unlock_the_launcher_would_accept() {
        let (port, launcher) = mock_launcher();
        let ep = Endpoint {
            authority: format!("127.0.0.1:{port}"),
            path: "/achievement".to_string(),
            key: "s3cret".to_string(),
        };

        assert_eq!(send(&ep, FIRST_AID).unwrap(), 200);

        let request = launcher.join().unwrap();
        let lower = request.to_lowercase();

        assert!(request.starts_with("POST /achievement HTTP/1.1"), "{request}");
        assert!(lower.contains("x-dibrysoft-award-key: s3cret\r\n"), "{request}");
        assert!(lower.contains("content-type: application/json"), "{request}");
        assert!(lower.contains("content-length: 19"), "{request}");
        // The launcher answers 403 to anything that looks like it came from a browser.
        assert!(!lower.contains("origin:"), "{request}");
        assert!(!lower.contains("sec-fetch-site:"), "{request}");
        assert!(request.ends_with(r#"{"key":"first-aid"}"#), "{request}");
    }

    #[test]
    fn a_refusal_comes_back_as_its_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let body = br#"{"error":"unknown achievement"}"#;
            let head = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });

        let ep = Endpoint {
            authority: format!("127.0.0.1:{port}"),
            path: "/achievement".to_string(),
            key: "s3cret".to_string(),
        };
        assert_eq!(send(&ep, FIRST_AID).unwrap(), 404);
    }

    #[test]
    fn an_unreachable_launcher_is_an_error_not_a_panic() {
        // Nothing is listening: bind a port, learn it, then drop the listener.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let ep = Endpoint {
            authority: format!("127.0.0.1:{port}"),
            path: "/achievement".to_string(),
            key: "s3cret".to_string(),
        };
        assert!(send(&ep, FIRST_AID).is_err());
    }

    #[test]
    fn the_url_the_launcher_hands_us_splits_into_host_and_path() {
        assert_eq!(
            split_url("http://127.0.0.1:25220/achievement"),
            Some(("127.0.0.1:25220".to_string(), "/achievement".to_string()))
        );
        // No port, no path, and whitespace the environment may have picked up.
        assert_eq!(
            split_url("  http://localhost  "),
            Some(("localhost:80".to_string(), "/".to_string()))
        );
        // We speak plain HTTP to a socket on this machine, and nothing else.
        assert_eq!(split_url("https://example.com/achievement"), None);
        assert_eq!(split_url(""), None);
        assert_eq!(split_url("http:///achievement"), None);
    }

    #[test]
    fn every_key_the_tracker_can_post_is_declared() {
        let declared = include_str!("../.ndib/achievements.json");
        for key in [
            FIRST_BRUSH,
            FIRST_AID,
            FIELD_MEDIC,
            HALF_MINUTE,
            FULL_MINUTE,
            TWO_MINUTE_ESCAPE,
            NOT_A_SCRATCH,
            STOCKED_UP,
            CROWD_CONTROL,
            RECORD_BREAKER,
            FULL_SPECTRUM,
            SHORT_FUSE,
        ] {
            assert!(
                declared.contains(&format!("\"key\": \"{key}\"")),
                "'{key}' is missing from .ndib/achievements.json - the launcher would answer 404"
            );
        }
    }

    #[test]
    fn a_key_is_claimed_once_and_can_be_released() {
        assert!(claim("test-only-key"));
        assert!(!claim("test-only-key"));
        release("test-only-key");
        assert!(claim("test-only-key"));
    }

    #[test]
    fn unlocks_are_no_ops_outside_the_launcher() {
        // Started from the launcher after all - posting real unlocks out of a test would be rude.
        if endpoint().is_some() {
            return;
        }

        let mut tracker = Tracker::new();
        tracker.record_run_start();
        tracker.record_health_box();
        tracker.record_color_shown(0, 7);
        tracker.record_frame(Frame {
            survival_time: 300.0,
            hp: 99,
            hits: 3,
            ball_count: 50,
        });
        tracker.record_game_over(1.0, true);
        flush();
    }
}
