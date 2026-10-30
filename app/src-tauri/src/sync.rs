//! Live sync (decision #38): keeps this machine's sealed scan on the relay and
//! pulls in every other linked machine's, live. Transport only; what is sealed
//! and how it merges lives in `patchpilot_local::sync` and `patchpilot_sync`.
//!
//! Idle cost: one hibernating WebSocket and a ping every 50 s. Uploads happen
//! after a changed scan (`rig::RIG_CHANGED`), on start, and after making Primary Rig.
//!
//! The relay's Durable Objects bill by rows read (5 million a day free), and a
//! rig holds one row per shared file, often tens of thousands. So nothing here
//! lists every object: the start lists `machine/` only, and uploads ask about
//! files this run hasn't already seen on the relay.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use patchpilot_sync::{LinkCode, RigKey, link};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

use crate::rig::{db_path, open};

/// The relay deployed from `relay/` (`.github/workflows/relay.yml`).
/// `PATCHPILOT_RELAY` overrides it, e.g. `http://localhost:8787` for `wrangler dev`.
const DEFAULT_RELAY: &str = "https://patch-pilot-relay.sunburner.workers.dev";
const PING_EVERY: Duration = Duration::from_secs(50);
/// Tries per relay request: a dropped connection is retried, with a pause.
const TRIES: u32 = 3;
/// Shared files that may fail in one upload before it stops trying.
const MAX_FAILED_FILES: u32 = 3;
/// Longest wait before reconnecting after a failed session.
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Names per `present` request; the relay takes up to 1,000.
const PRESENT_BATCH: usize = 1000;

/// Shown in the UI. Mirrored in `src/api.ts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub linked: bool,
    pub online: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Sync {
    status: Mutex<SyncStatus>,
    on_relay: Mutex<OnRelay>,
    running: AtomicBool,
    nudge: Notify,
    stop: Notify,
}

pub type SyncState = Arc<Sync>;

/// Shared-file objects known to be on the relay, for one rig. Files are
/// content-addressed and never deleted, so once seen they stay there.
#[derive(Default)]
struct OnRelay {
    rig_id: String,
    names: std::collections::HashSet<String>,
}

/// Start syncing if this machine is linked, and upload after changed scans.
pub fn setup(app: &AppHandle) {
    install_crypto();
    let state: SyncState = Arc::default();
    app.manage(state.clone());
    let s = state.clone();
    app.listen_any(crate::rig::RIG_CHANGED, move |_| s.nudge.notify_one());
    start(app, &state);
}

/// Upload this machine's object soon (after a scan or a Primary Rig change).
pub fn nudge(app: &AppHandle) {
    app.state::<SyncState>().nudge.notify_one();
}

fn start(app: &AppHandle, state: &SyncState) {
    if state.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        run(&app, &state).await;
        state.running.store(false, Ordering::SeqCst);
    });
}

async fn run(app: &AppHandle, state: &Sync) {
    let mut backoff = Duration::from_secs(2);
    loop {
        let rig = match load_rig() {
            Ok(Some(rig)) => rig,
            Ok(None) => {
                set(app, state, |s| *s = SyncStatus::default());
                return;
            }
            Err(e) => {
                set(app, state, |s| s.error = Some(e));
                return;
            }
        };
        set(app, state, |s| s.linked = true);
        tokio::select! {
            r = session(app, state, &rig) => {
                let e = r.err().unwrap_or_else(|| "the relay closed the connection".to_owned());
                let was_online = state.status.lock().expect("sync status lock").online;
                set(app, state, |s| {
                    s.online = false;
                    s.error = Some(e);
                });
                // A session that got going dropped (a relay deploy, sleep):
                // come back quickly. One failing at the start waits longer
                // each time, as every attempt costs relay reads.
                if was_online {
                    backoff = Duration::from_secs(2);
                }
            }
            () = state.stop.notified() => {
                set(app, state, |s| *s = SyncStatus::default());
                return;
            }
        }
        // Retry, sooner if something changes here.
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            () = state.nudge.notified() => {}
            () = state.stop.notified() => {
                set(app, state, |s| *s = SyncStatus::default());
                return;
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(app: &AppHandle, state: &Sync, rig: &RigKey) -> Result<(), String> {
    let relay = Relay::new(rig);
    let this = this_machine()?;
    // Upload first: the first upload claims a new rig, and live needs a claimed one.
    upload(&relay, state, &this, rig).await?;
    let mut ws = relay.live().await?;
    let mut changed = false;
    for o in relay.list("machine/").await? {
        if patchpilot_local::sync::is_other_machine(&o.name, &this) {
            changed |= pull(&relay, &this, rig, &o.name).await?;
        }
    }
    if changed {
        let _ = app.emit("data-changed", ());
    }
    set(app, state, |s| {
        s.online = true;
        s.error = None;
    });

    let mut ping = tokio::time::interval(PING_EVERY);
    loop {
        tokio::select! {
            msg = ws.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.to_string()),
                };
                let Ok(event) = serde_json::from_str::<LiveEvent>(&text) else { continue };
                if event.kind == "changed"
                    && patchpilot_local::sync::is_other_machine(&event.name, &this)
                    && pull(&relay, &this, rig, &event.name).await?
                {
                    let _ = app.emit("data-changed", ());
                }
            }
            () = state.nudge.notified() => upload(&relay, state, &this, rig).await?,
            _ = ping.tick() => ws.send(Message::text("ping")).await.map_err(|e| e.to_string())?,
        }
    }
}

async fn upload(relay: &Relay, state: &Sync, this: &str, rig: &RigKey) -> Result<(), String> {
    let (out, files) = {
        let store = open(&db_path()?)?;
        let files = store
            .latest_manifest(this)
            .map_err(crate::rig::err)?
            .map(|m| patchpilot_local::bring::shared_files(&m))
            .unwrap_or_default();
        (patchpilot_local::sync::outgoing(&store, this, rig)?, files)
    };
    let Some((object, sealed)) = out else {
        return Ok(());
    };
    relay.put(&object, sealed).await?;
    upload_files(relay, rig, &files, &state.on_relay).await
}

/// Put this machine's shareable files (templates, scripts, themes) on the
/// relay, so others can bring them over (decision #45). Content-addressed:
/// only files no machine has uploaded yet are sent, and only files not seen
/// on the relay before in this run are asked about.
async fn upload_files(
    relay: &Relay,
    rig: &RigKey,
    files: &[patchpilot_local::bring::SharedFile],
    on_relay: &Mutex<OnRelay>,
) -> Result<(), String> {
    let mut unknown: Vec<(String, &patchpilot_local::bring::SharedFile)> = {
        let mut known = on_relay.lock().expect("on-relay lock");
        if known.rig_id != rig.rig_id() {
            *known = OnRelay {
                rig_id: rig.rig_id(),
                ..OnRelay::default()
            };
        }
        let mut asked = std::collections::HashSet::new();
        files
            .iter()
            .map(|f| (rig.file_object(&f.hash), f))
            .filter(|(name, _)| !known.names.contains(name) && asked.insert(name.clone()))
            .collect()
    };
    if unknown.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = unknown.iter().map(|(n, _)| n.clone()).collect();
    let there = relay.present(&names).await?;
    unknown.retain(|(name, _)| !there.contains(name));
    remember(on_relay, there);
    let mut failed = 0;
    for (name, file) in unknown {
        // Changed since the scan: the next scan uploads the new one.
        let Some(bytes) = patchpilot_local::bring::read_shared(file) else {
            continue;
        };
        let sealed = rig.seal_file(&file.hash, &bytes);
        // A file that won't go up stays off the relay until the next upload;
        // it doesn't take live sync offline. A few in a row means the
        // connection is gone, and the next upload starts over.
        match relay.put(&name, sealed).await {
            Ok(()) => remember(on_relay, [name]),
            Err(e) => {
                eprintln!("sync: {} not shared: {e}", file.rel);
                failed += 1;
                if failed == MAX_FAILED_FILES {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn remember(on_relay: &Mutex<OnRelay>, names: impl IntoIterator<Item = String>) {
    on_relay.lock().expect("on-relay lock").names.extend(names);
}

/// Fetch one shared file by hash; `None` when no machine has uploaded it.
pub(crate) async fn fetch_file(
    rig: &RigKey,
    hash: &patchpilot_core::ContentHash,
) -> Result<Option<Vec<u8>>, String> {
    let relay = Relay::new(rig);
    match relay.get(&rig.file_object(hash)).await? {
        Some(sealed) => rig
            .open_file(hash, &sealed)
            .map(Some)
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// Fetch and merge another machine's object. True if anything changed.
async fn pull(relay: &Relay, this: &str, rig: &RigKey, object: &str) -> Result<bool, String> {
    let Some(sealed) = relay.get(object).await? else {
        return Ok(false);
    };
    let mut store = open(&db_path()?)?;
    let report = patchpilot_local::sync::incoming(&mut store, this, rig, object, &sealed)?;
    Ok(patchpilot_local::sync::changed(&report))
}

fn set(app: &AppHandle, state: &Sync, f: impl FnOnce(&mut SyncStatus)) {
    let status = {
        let mut s = state.status.lock().expect("sync status lock");
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("sync-status", status);
}

#[tauri::command]
pub fn sync_status(state: State<'_, SyncState>) -> SyncStatus {
    state.status.lock().expect("sync status lock").clone()
}

/// This machine shows a code; another machine types it to join this rig.
/// Makes a rig first if this machine has none. Returns the code at once and
/// emits `link-finished` (`{ ok, error }`) when the other side is done.
#[tauri::command]
pub async fn link_start(app: AppHandle, state: State<'_, SyncState>) -> Result<String, String> {
    this_machine()?;
    let rig = match load_rig()? {
        Some(rig) => rig,
        None => {
            let rig = RigKey::generate();
            patchpilot_local::sync::save_rig(&app_dir()?, &rig)?;
            rig
        }
    };
    let code = LinkCode::generate();
    let shown = code.to_string();
    let (app2, state2) = (app.clone(), state.inner().clone());
    tauri::async_runtime::spawn(async move {
        let result = offer(&code, &rig).await;
        let _ = app2.emit(
            "link-finished",
            LinkFinished {
                ok: result.is_ok(),
                error: result.err(),
            },
        );
        start(&app2, &state2);
        state2.nudge.notify_one();
    });
    start(&app, state.inner());
    Ok(shown)
}

/// Join the rig of the machine showing `code`.
#[tauri::command]
pub async fn link_join(
    app: AppHandle,
    state: State<'_, SyncState>,
    code: String,
) -> Result<(), String> {
    this_machine()?;
    if load_rig()?.is_some() {
        return Err("This machine is already linked. Unlink it first.".to_owned());
    }
    let code: LinkCode = code
        .parse()
        .map_err(|e: patchpilot_sync::Error| e.to_string())?;
    let rig = join(&code).await?;
    patchpilot_local::sync::save_rig(&app_dir()?, &rig)?;
    start(&app, state.inner());
    Ok(())
}

/// Stop syncing on this machine. Machines already received stay.
#[tauri::command]
pub fn unlink(state: State<'_, SyncState>) -> Result<(), String> {
    patchpilot_local::sync::forget_rig(&app_dir()?)?;
    state.stop.notify_waiters();
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct LinkFinished {
    ok: bool,
    error: Option<String>,
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The linked machine's side of the handshake (`patchpilot_sync::link`).
async fn offer(code: &LinkCode, rig: &RigKey) -> Result<(), String> {
    let mut ws = mailbox(code).await?;
    let (state, mine) = link::start(code);
    send(&mut ws, mine).await?;
    let session = state
        .finish(&recv(&mut ws).await?)
        .map_err(|e| e.to_string())?;
    send(&mut ws, session.offer(rig)).await?;
    session
        .check_confirm(&recv(&mut ws).await?)
        .map_err(|e| e.to_string())
}

/// The new machine's side.
async fn join(code: &LinkCode) -> Result<RigKey, String> {
    let mut ws = mailbox(code).await?;
    let (state, mine) = link::start(code);
    send(&mut ws, mine).await?;
    let session = state
        .finish(&recv(&mut ws).await?)
        .map_err(|e| e.to_string())?;
    let rig = session
        .accept(&recv(&mut ws).await?)
        .map_err(|e| e.to_string())?;
    send(&mut ws, session.confirm()).await?;
    Ok(rig)
}

async fn mailbox(code: &LinkCode) -> Result<Socket, String> {
    install_crypto();
    let url = format!("{}/v1/link/{}", ws_base(), code.mailbox());
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| match e {
            tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == 409 => {
                "That code was already used. Make a new one on the other machine.".to_owned()
            }
            e => format!("can't reach the sync service: {e}"),
        })?;
    Ok(ws)
}

async fn send(ws: &mut Socket, bytes: Vec<u8>) -> Result<(), String> {
    ws.send(Message::binary(bytes))
        .await
        .map_err(|e| e.to_string())
}

/// The next handshake frame. Waiting can take minutes (someone walks to the
/// other machine), and the relay drops a socket idle for 100 s, so this pings
/// meanwhile; the relay answers "pong", and text frames are skipped.
async fn recv(ws: &mut Socket) -> Result<Vec<u8>, String> {
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
    loop {
        tokio::select! {
            msg = ws.next() => match msg {
                Some(Ok(Message::Binary(b))) => return Ok(b.to_vec()),
                Some(Ok(Message::Close(_))) | None => {
                    return Err("the other machine stopped linking, or the code expired".to_owned());
                }
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(format!("lost the connection to the sync service: {e}")),
            },
            _ = ping.tick() => send_text(ws, "ping").await?,
        }
    }
}

async fn send_text(ws: &mut Socket, text: &str) -> Result<(), String> {
    ws.send(Message::text(text))
        .await
        .map_err(|e| format!("lost the connection to the sync service: {e}"))
}

#[derive(Debug, Deserialize)]
struct LiveEvent {
    #[serde(rename = "type")]
    kind: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ObjectInfo {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ObjectList {
    objects: Vec<ObjectInfo>,
}

#[derive(Debug, Deserialize)]
struct Present {
    present: Vec<String>,
}

/// The relay's HTTP API for one rig (`relay/README.md`).
struct Relay {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Relay {
    fn new(rig: &RigKey) -> Self {
        install_crypto();
        Self {
            http: reqwest::Client::new(),
            base: format!("{}/v1/rigs/{}", relay_url(), rig.rig_id()),
            token: rig.relay_token(),
        }
    }

    async fn put(&self, object: &str, sealed: Vec<u8>) -> Result<(), String> {
        let url = format!("{}/objects/{object}", self.base);
        let res = self
            .send(|| self.http.put(&url).body(sealed.clone()))
            .await?;
        check(res).await.map(|_| ())
    }

    /// Send with the token, again after a pause if the connection drops
    /// (Windows' os error 10054). Every request here is safe to repeat.
    async fn send(
        &self,
        request: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        let mut tries = 1;
        loop {
            match request().bearer_auth(&self.token).send().await {
                Ok(res) => return Ok(res),
                Err(e) if tries < TRIES && (e.is_connect() || e.is_request()) => {
                    tokio::time::sleep(Duration::from_secs(tries.into())).await;
                    tries += 1;
                }
                Err(e) => return Err(describe(e)),
            }
        }
    }

    async fn get(&self, object: &str) -> Result<Option<Vec<u8>>, String> {
        let url = format!("{}/objects/{object}", self.base);
        let res = self.send(|| self.http.get(&url)).await?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let res = check(res).await?;
        Ok(Some(res.bytes().await.map_err(|e| e.to_string())?.to_vec()))
    }

    /// The objects whose names start with `prefix`. A relay from before
    /// prefixes sends them all, so callers still filter.
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectInfo>, String> {
        let url = format!("{}/objects?prefix={prefix}", self.base);
        let res = self.send(|| self.http.get(&url)).await?;
        // A rig nobody has uploaded to yet isn't claimed.
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(Vec::new());
        }
        let list: ObjectList = check(res).await?.json().await.map_err(|e| e.to_string())?;
        Ok(list
            .objects
            .into_iter()
            .filter(|o| o.name.starts_with(prefix))
            .collect())
    }

    /// Which of `names` are on the relay. Reads only those rows, where a
    /// list reads every object the rig has.
    async fn present(&self, names: &[String]) -> Result<std::collections::HashSet<String>, String> {
        let url = format!("{}/present", self.base);
        let mut there = std::collections::HashSet::new();
        for chunk in names.chunks(PRESENT_BATCH) {
            let body = serde_json::json!({ "names": chunk });
            let res = self.send(|| self.http.post(&url).json(&body)).await?;
            // A relay from before `present`: fall back to one full list.
            if res.status() == reqwest::StatusCode::NOT_FOUND {
                let all = self.list("").await?;
                return Ok(all.into_iter().map(|o| o.name).collect());
            }
            let found: Present = check(res).await?.json().await.map_err(|e| e.to_string())?;
            there.extend(found.present);
        }
        Ok(there)
    }

    async fn live(&self) -> Result<Socket, String> {
        let url = format!("{}/live", self.base.replacen("http", "ws", 1));
        let mut req = url.into_client_request().map_err(|e| e.to_string())?;
        let auth =
            HeaderValue::from_str(&format!("Bearer {}", self.token)).map_err(|e| e.to_string())?;
        req.headers_mut().insert("Authorization", auth);
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| format!("can't reach the sync service: {e}"))?;
        Ok(ws)
    }
}

/// "can't reach the sync service: connection reset (os error 10054)" rather
/// than reqwest's "error sending request for url (…)", which hides the cause
/// behind a long URL.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut cause = String::new();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        cause = s.to_string();
        source = s.source();
    }
    if cause.is_empty() {
        cause = e.to_string();
    }
    format!("can't reach the sync service: {cause}")
}

async fn check(res: reqwest::Response) -> Result<reqwest::Response, String> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    if status == reqwest::StatusCode::PAYLOAD_TOO_LARGE {
        return Err("this machine's list is too big for the sync service".to_owned());
    }
    let body = res.text().await.unwrap_or_default();
    Err(format!("sync service: {status} {}", body.trim()))
}

/// rustls needs a process-wide crypto provider; ring, as the updater uses.
pub(crate) fn install_crypto() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn relay_url() -> String {
    std::env::var("PATCHPILOT_RELAY")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_RELAY.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn ws_base() -> String {
    relay_url().replacen("http", "ws", 1)
}

pub(crate) fn app_dir() -> Result<std::path::PathBuf, String> {
    Ok(patchpilot_local::app_dir(&db_path()?).to_owned())
}

pub(crate) fn load_rig() -> Result<Option<RigKey>, String> {
    patchpilot_local::sync::load_rig(&app_dir()?)
}

pub(crate) fn this_machine() -> Result<String, String> {
    patchpilot_store::read_machine_id(&app_dir()?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Scan this machine first.".to_owned())
}

/// Against a running relay: `cd relay && npx wrangler dev`, then
/// `PATCHPILOT_RELAY=http://localhost:8787 cargo test -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "needs a relay (PATCHPILOT_RELAY)"]
    async fn link_then_upload_and_list_through_a_real_relay() {
        let rig = RigKey::generate();
        let code = LinkCode::generate();
        let (offered, joined) = tokio::join!(offer(&code, &rig), async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            join(&code).await
        });
        offered.unwrap();
        assert_eq!(joined.unwrap(), rig);
        // One attempt per code.
        assert!(join(&code).await.unwrap_err().contains("already used"));

        let relay = Relay::new(&rig);
        assert!(relay.list("").await.unwrap().is_empty());
        relay.put("machine/abc", b"first".to_vec()).await.unwrap();
        let mut ws = relay.live().await.unwrap();
        relay.put("machine/abc", b"sealed".to_vec()).await.unwrap();
        let Some(Ok(Message::Text(t))) = ws.next().await else {
            panic!("no live event")
        };
        let event: LiveEvent = serde_json::from_str(&t).unwrap();
        assert_eq!(
            (event.kind.as_str(), event.name.as_str()),
            ("changed", "machine/abc")
        );
        assert_eq!(relay.get("machine/abc").await.unwrap().unwrap(), b"sealed");
        assert_eq!(relay.list("machine/").await.unwrap()[0].name, "machine/abc");
        assert_eq!(relay.get("machine/nope").await.unwrap(), None);

        // Another rig can't read it.
        let other = Relay::new(&RigKey::generate());
        assert!(other.get("machine/abc").await.is_err());
    }

    #[tokio::test]
    #[ignore = "needs a relay (PATCHPILOT_RELAY)"]
    async fn shared_files_travel_once_and_open_only_by_hash() {
        let path =
            std::env::temp_dir().join(format!("pp-{}.RfxChain", RigKey::generate().rig_id()));
        std::fs::write(&path, "<FXCHAIN\n>\n").unwrap();
        let hash = patchpilot_sync::content_hash(b"<FXCHAIN\n>\n");
        let files = [patchpilot_local::bring::SharedFile {
            hash: hash.clone(),
            path: path.clone(),
            root: "REAPER_RESOURCES".into(),
            rel: "FXChains/x.RfxChain".into(),
        }];

        let rig = RigKey::generate();
        let relay = Relay::new(&rig);
        // The machine object claims the rig first, as `upload` does.
        relay.put("machine/abc", b"sealed".to_vec()).await.unwrap();
        assert_eq!(fetch_file(&rig, &hash).await.unwrap(), None);
        let on_relay = Mutex::default();
        upload_files(&relay, &rig, &files, &on_relay).await.unwrap();
        upload_files(&relay, &rig, &files, &on_relay).await.unwrap();
        // A fresh run asks the relay, finds it there and sends nothing.
        upload_files(&relay, &rig, &files, &Mutex::default())
            .await
            .unwrap();
        assert_eq!(relay.list("").await.unwrap().len(), 2);
        assert_eq!(relay.list("file/").await.unwrap().len(), 1);
        assert_eq!(
            fetch_file(&rig, &hash).await.unwrap().unwrap(),
            b"<FXCHAIN\n>\n"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    #[ignore = "needs a relay (PATCHPILOT_RELAY)"]
    async fn a_code_still_works_after_keep_alive_pings() {
        let rig = RigKey::generate();
        let code = LinkCode::generate();
        let (offered, joined) = tokio::join!(offer(&code, &rig), async {
            tokio::time::sleep(PING_EVERY + Duration::from_secs(5)).await;
            join(&code).await
        });
        offered.unwrap();
        assert_eq!(joined.unwrap(), rig);
    }

    #[tokio::test]
    #[ignore = "needs a relay (PATCHPILOT_RELAY)"]
    async fn a_wrong_code_fails_cleanly() {
        let rig = RigKey::generate();
        let code = LinkCode::generate();
        let wrong: LinkCode = format!("{}-0000-0000", code.mailbox()).parse().unwrap();
        let (offered, joined) = tokio::join!(offer(&code, &rig), async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            join(&wrong).await
        });
        assert!(joined.is_err());
        assert!(offered.is_err());
    }
}
