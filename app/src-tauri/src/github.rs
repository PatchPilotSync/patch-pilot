//! Advanced: sync with GitHub (decision #73). Signing in (GitHub's device
//! sign-in, or a pasted token in builds without an OAuth app), picking or
//! making a private repo, and GitHub's REST API for
//! `patchpilot_local::github::sync`, which keeps the backup folder's copy in
//! app-data and the repo the same. Backups push after they're made
//! (backup.rs); keeping files in sync pulls before it looks (autosync.rs),
//! and every 10 minutes while it's on. Pushes go over Git's own smart HTTP
//! protocol (thousands of files a request) and a big first pull comes as one
//! archive, since the REST API's limits stop after a few hundred files.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use patchpilot_local::backup::{self, BackupSettings};
use patchpilot_local::github::{
    self, GitHubReport, GitHubSettings, Progress, Remote, RemoteFile, RepoRef,
};
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::backup::BACKUP_CHANGED;
use crate::rig::{db_path, open};
use crate::sync::{app_dir, this_machine};

const API: &str = "https://api.github.com";
/// The OAuth app's client ID (not a secret), set when a release is built.
/// Without one, signing in takes a pasted token.
const CLIENT_ID: Option<&str> = option_env!("PATCHPILOT_GITHUB_CLIENT_ID");
const TOKEN_PAGE: &str =
    "https://github.com/settings/tokens/new?scopes=repo&description=Patch%20Pilot";
/// Emitted when signing in finishes or fails, for the UI.
pub const GITHUB_CHANGED: &str = "github-changed";
/// Emitted with a [`Progress`] while a long sync runs, then with null.
pub const GITHUB_PROGRESS: &str = "github-progress";
/// After a sync that didn't finish, try again after this, then twice as
/// long each time up to [`RETRY_MAX`]. A part already sent stays sent.
const RETRY: Duration = Duration::from_secs(120);
const RETRY_MAX: Duration = Duration::from_secs(1800);
/// How often to look for other machines' changes while keeping files in sync.
const POLL: Duration = Duration::from_secs(600);
/// A linked machine's scan arrives before its push; look again after this.
const AFTER_SCAN: Duration = Duration::from_secs(60);

#[derive(Default)]
struct GitHubState {
    device: Mutex<Option<DeviceCode>>,
    /// Bumped to stop a sign-in in progress.
    sign_in: AtomicU64,
    error: Mutex<Option<String>>,
    last: Mutex<Option<GitHubReport>>,
    /// One sync at a time.
    running: Mutex<()>,
    look_pending: AtomicBool,
    /// How far the running sync has got.
    progress: Mutex<Option<Progress>>,
    retry_pending: AtomicBool,
    /// Seconds until the next try after a failure; 0 after a success.
    retry_after: AtomicU64,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct GitHubView {
    /// Signing in opens GitHub with a code; otherwise it takes a token.
    pub device_sign_in: bool,
    pub login: Option<String>,
    /// `owner/name` of the repo picked.
    pub repo: Option<String>,
    /// The backup folder is the repo's copy.
    pub in_use: bool,
    pub synced_at: Option<i64>,
    /// A sign-in waiting for the code to be entered on GitHub.
    pub device: Option<DeviceCode>,
    pub error: Option<String>,
    pub last: Option<GitHubReport>,
    pub progress: Option<Progress>,
    pub default_repo: &'static str,
}

pub fn setup(app: &AppHandle) {
    app.manage(GitHubState::default());
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(POLL);
            if in_use() && crate::autosync::enabled() {
                crate::autosync::look(&handle);
            }
        }
    });
    let handle = app.clone();
    app.listen_any("data-changed", move |_| {
        let state = handle.state::<GitHubState>();
        if !in_use() || state.look_pending.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(AFTER_SCAN);
            handle
                .state::<GitHubState>()
                .look_pending
                .store(false, Ordering::SeqCst);
            crate::autosync::look(&handle);
        });
    });
}

/// The backup folder is the repo's copy.
pub fn in_use() -> bool {
    app_dir().is_ok_and(|d| uses_github(&backup::load_settings(&d)))
}

fn uses_github(settings: &BackupSettings) -> bool {
    settings.service.as_deref() == Some(github::SERVICE)
}

/// The running sync's progress and the last sync's error, for the Sync &
/// backup line.
pub fn status(app: &AppHandle) -> (Option<Progress>, Option<String>) {
    let state = app.state::<GitHubState>();
    let progress = state.progress.lock().ok().and_then(|p| *p);
    let error = state.error.lock().ok().and_then(|e| e.clone());
    (progress, error)
}

/// Bring the copy and the repo up to date, when the backup folder is the
/// repo's copy. Quietly nothing otherwise. One that doesn't finish tries
/// again on its own, picking up where it stopped.
pub fn sync_now(app: &AppHandle) -> Result<Option<GitHubReport>, String> {
    let dir = app_dir()?;
    if !uses_github(&backup::load_settings(&dir)) {
        return Ok(None);
    }
    let state = app.state::<GitHubState>();
    let _one = state.running.lock().map_err(|e| e.to_string())?;
    let mut shown: Option<Instant> = None;
    let result = sync_blocking(&dir, &mut |p| {
        if let Ok(mut slot) = state.progress.lock() {
            *slot = Some(p);
        }
        // A few times a second at most.
        if shown.is_none_or(|t| t.elapsed() >= Duration::from_millis(300)) || p.done == p.total {
            shown = Some(Instant::now());
            let _ = app.emit(GITHUB_PROGRESS, Some(p));
        }
    });
    if let Ok(mut slot) = state.progress.lock() {
        *slot = None;
    }
    let had_error = state.error.lock().is_ok_and(|e| e.is_some());
    if let Ok(mut e) = state.error.lock() {
        *e = result.as_ref().err().cloned();
    }
    if let (Ok(r), Ok(mut last)) = (&result, state.last.lock()) {
        *last = Some(r.clone());
    }
    if shown.is_some() {
        let _ = app.emit(GITHUB_PROGRESS, None::<Progress>);
    }
    let changed = result.as_ref().map_or(true, |r| r.pulled + r.pushed > 0);
    if shown.is_some() || changed || had_error {
        let _ = app.emit(BACKUP_CHANGED, ());
    }
    match &result {
        Ok(_) => state.retry_after.store(0, Ordering::SeqCst),
        Err(_) => retry_later(app),
    }
    result.map(Some)
}

/// Try a sync that didn't finish again, waiting longer each time.
fn retry_later(app: &AppHandle) {
    let state = app.state::<GitHubState>();
    if state.retry_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let wait = match state.retry_after.load(Ordering::SeqCst) {
        0 => RETRY,
        s => (Duration::from_secs(s) * 2).min(RETRY_MAX),
    };
    state.retry_after.store(wait.as_secs(), Ordering::SeqCst);
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        handle
            .state::<GitHubState>()
            .retry_pending
            .store(false, Ordering::SeqCst);
        if !in_use() {
            return;
        }
        // Keeping files in sync syncs with GitHub first, then looks.
        if crate::autosync::enabled() {
            crate::autosync::look(&handle);
        } else {
            let _ = sync_now(&handle);
        }
    });
}

fn sync_blocking(
    dir: &std::path::Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<GitHubReport, String> {
    let token = github::load_token(dir).ok_or("Sign in to GitHub again on This machine.")?;
    let mut settings = github::load_settings(dir);
    let repo = settings
        .repo
        .clone()
        .ok_or("Pick a GitHub repo on This machine.")?;
    let this = this_machine()?;
    let name = machine_name(&this);
    let mut index = github::load_index(dir);
    let api = Api::new(&token, settings.login.as_deref(), repo);
    let report = github::sync(
        &api,
        &github::mirror_dir(dir),
        &this,
        &name,
        &mut settings,
        &mut index,
        patchpilot_local::now(),
        progress,
    );
    // A part that went keeps its commit, so the next try starts after it.
    github::save_settings(dir, &settings)?;
    // The index is right for the files read even when the sync stopped.
    github::save_index(dir, &index)?;
    report
}

pub(crate) fn machine_name(this: &str) -> String {
    db_path()
        .ok()
        .and_then(|db| open(&db).ok())
        .and_then(|s| s.latest_manifest(this).ok().flatten())
        .map(|m| m.machine.name)
        .unwrap_or_else(|| "Patch Pilot".into())
}

fn client_id() -> Option<&'static str> {
    CLIENT_ID.map(str::trim).filter(|c| !c.is_empty())
}

#[tauri::command]
pub async fn github_view(app: AppHandle) -> Result<GitHubView, String> {
    let dir = app_dir()?;
    let settings = github::load_settings(&dir);
    let signed_in = github::load_token(&dir).is_some();
    let state = app.state::<GitHubState>();
    Ok(GitHubView {
        device_sign_in: client_id().is_some(),
        login: settings.login.filter(|_| signed_in),
        repo: settings.repo.as_ref().map(RepoRef::full_name),
        in_use: uses_github(&backup::load_settings(&dir)),
        synced_at: settings.synced_at,
        device: state.device.lock().ok().and_then(|d| d.clone()),
        error: state.error.lock().ok().and_then(|e| e.clone()),
        last: state.last.lock().ok().and_then(|l| l.clone()),
        progress: state.progress.lock().ok().and_then(|p| *p),
        default_repo: github::DEFAULT_REPO,
    })
}

/// Start GitHub's device sign-in: the view carries the code to enter on
/// GitHub, and `github-changed` fires when it's done.
#[tauri::command]
pub async fn github_sign_in(app: AppHandle) -> Result<GitHubView, String> {
    let client = client_id().ok_or("Paste a token instead.")?;
    let start = tauri::async_runtime::spawn_blocking(move || {
        let res = http()
            .post("https://github.com/login/device/code")
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("client_id={client}&scope=repo"))
            .send()
            .map_err(|e| format!("Can't reach GitHub: {e}"))?;
        res.json::<Value>().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let field = |k: &str| start[k].as_str().unwrap_or_default().to_owned();
    let (device_code, user_code) = (field("device_code"), field("user_code"));
    if device_code.is_empty() || user_code.is_empty() {
        return Err(format!(
            "GitHub didn't start signing in: {}",
            field("error_description")
        ));
    }
    let code = DeviceCode {
        user_code,
        verification_uri: Some(field("verification_uri"))
            .filter(|u| u.starts_with("https://github.com/"))
            .unwrap_or_else(|| "https://github.com/login/device".into()),
    };
    let state = app.state::<GitHubState>();
    let generation = state.sign_in.fetch_add(1, Ordering::SeqCst) + 1;
    *state.device.lock().map_err(|e| e.to_string())? = Some(code.clone());
    *state.error.lock().map_err(|e| e.to_string())? = None;
    let _ = patchpilot_local::open::open_url(&code.verification_uri);
    let interval = start["interval"].as_u64().unwrap_or(5);
    let expires = start["expires_in"].as_u64().unwrap_or(900);
    let handle = app.clone();
    std::thread::spawn(move || {
        let result = wait_for_token(&handle, client, &device_code, interval, expires, generation);
        let state = handle.state::<GitHubState>();
        if state.sign_in.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Ok(mut d) = state.device.lock() {
            *d = None;
        }
        let result = result.and_then(|token| signed_in(&token));
        if let (Err(e), Ok(mut slot)) = (result, state.error.lock()) {
            *slot = Some(e);
        }
        let _ = handle.emit(GITHUB_CHANGED, ());
    });
    github_view(app).await
}

/// Ask GitHub every `interval` seconds until the code was entered.
fn wait_for_token(
    app: &AppHandle,
    client: &str,
    device_code: &str,
    mut interval: u64,
    expires: u64,
    generation: u64,
) -> Result<String, String> {
    let http = http();
    let mut waited = 0;
    while waited < expires {
        std::thread::sleep(Duration::from_secs(interval));
        waited += interval;
        if app.state::<GitHubState>().sign_in.load(Ordering::SeqCst) != generation {
            return Err("Stopped.".into());
        }
        let Ok(res) = http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "client_id={client}&device_code={device_code}\
                 &grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"
            ))
            .send()
        else {
            continue;
        };
        let body: Value = res.json().unwrap_or_default();
        if let Some(token) = body["access_token"].as_str() {
            return Ok(token.to_owned());
        }
        match body["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some("access_denied") => return Err("Signing in to GitHub was cancelled.".into()),
            Some("expired_token") => break,
            _ => {}
        }
    }
    Err("The GitHub code ran out. Sign in again.".into())
}

#[tauri::command]
pub async fn github_cancel_sign_in(app: AppHandle) -> Result<GitHubView, String> {
    let state = app.state::<GitHubState>();
    state.sign_in.fetch_add(1, Ordering::SeqCst);
    *state.device.lock().map_err(|e| e.to_string())? = None;
    github_view(app).await
}

/// Sign in with a token the user made on GitHub (builds without an OAuth app).
#[tauri::command]
pub async fn github_use_token(app: AppHandle, token: String) -> Result<GitHubView, String> {
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err("Paste the token GitHub showed you.".into());
    }
    tauri::async_runtime::spawn_blocking(move || signed_in(&token))
        .await
        .map_err(|e| e.to_string())??;
    github_view(app).await
}

/// Check `token` with GitHub and keep it, with who it belongs to.
fn signed_in(token: &str) -> Result<(), String> {
    let (status, user) = call(http().get(format!("{API}/user")), token)?;
    if status == StatusCode::UNAUTHORIZED {
        return Err("GitHub didn't accept that token.".into());
    }
    let login = user["login"]
        .as_str()
        .filter(|_| status.is_success())
        .ok_or_else(|| api_error(status, &user))?;
    let dir = app_dir()?;
    github::save_token(&dir, token)?;
    let mut settings = github::load_settings(&dir);
    if settings.login.as_deref() != Some(login) {
        settings = GitHubSettings {
            login: Some(login.to_owned()),
            ..GitHubSettings::default()
        };
    }
    github::save_settings(&dir, &settings)
}

/// Sync with the repo `name` (`name`, `owner/name` or a link), making it,
/// private, when it isn't there. The backup folder becomes its copy.
#[tauri::command]
pub async fn github_use_repo(app: AppHandle, name: String) -> Result<GitHubView, String> {
    let dir = app_dir()?;
    let token = github::load_token(&dir).ok_or("Sign in to GitHub first.")?;
    let mut settings = github::load_settings(&dir);
    let login = settings.login.clone().unwrap_or_default();
    let (owner, name) = github::parse_repo(&name, &login)
        .ok_or("That isn't a repo name, like patch-pilot-backup.")?;
    let repo =
        tauri::async_runtime::spawn_blocking(move || find_or_make(&token, &login, &owner, &name))
            .await
            .map_err(|e| e.to_string())??;
    if settings.repo.as_ref() != Some(&repo) {
        settings.head = None;
        settings.synced_at = None;
    }
    settings.repo = Some(repo);
    github::save_settings(&dir, &settings)?;
    std::fs::create_dir_all(github::mirror_dir(&dir)).map_err(|e| e.to_string())?;
    let backups = BackupSettings {
        service: Some(github::SERVICE.into()),
        folder: Some(github::backup_folder(&dir)),
        ..backup::load_settings(&dir)
    };
    backup::save_settings(&dir, &backups)?;
    let _ = app.emit(BACKUP_CHANGED, ());
    // Get the other machines' backups first, then send this one's.
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = sync_now(&handle);
        crate::backup::auto_back_up(&handle);
        crate::autosync::look(&handle);
        let _ = handle.emit(BACKUP_CHANGED, ());
    });
    github_view(app).await
}

fn find_or_make(token: &str, login: &str, owner: &str, name: &str) -> Result<RepoRef, String> {
    let http = http();
    let (status, repo) = call(http.get(format!("{API}/repos/{owner}/{name}")), token)?;
    let repo = if status == StatusCode::NOT_FOUND {
        let url = if owner.eq_ignore_ascii_case(login) {
            format!("{API}/user/repos")
        } else {
            format!("{API}/orgs/{owner}/repos")
        };
        let body = json!({
            "name": name,
            "private": true,
            "auto_init": true,
            "description": "Patch Pilot: backups and synced files from my machines",
        });
        let (status, made) = call(http.post(url).json(&body), token)?;
        if !status.is_success() {
            return Err(format!(
                "Couldn't make the repo: {}",
                api_error(status, &made)
            ));
        }
        made
    } else if status.is_success() {
        if repo["private"].as_bool() != Some(true) {
            return Err(
                "That repo is public. Pick a private one, so your files stay yours.".into(),
            );
        }
        if repo["permissions"]["push"].as_bool() == Some(false) {
            return Err("This GitHub account can't change that repo.".into());
        }
        repo
    } else {
        return Err(api_error(status, &repo));
    };
    let field = |k: &str| repo[k].as_str().map(str::to_owned);
    Ok(RepoRef {
        owner: repo["owner"]["login"]
            .as_str()
            .map_or_else(|| owner.to_owned(), str::to_owned),
        name: field("name").unwrap_or_else(|| name.to_owned()),
        branch: field("default_branch").unwrap_or_else(|| "main".into()),
    })
}

/// Stop syncing with GitHub and sign out. The repo and the copy in app-data
/// stay; backups go nowhere until a folder is picked.
#[tauri::command]
pub async fn github_sign_out(app: AppHandle) -> Result<GitHubView, String> {
    let dir = app_dir()?;
    let backups = backup::load_settings(&dir);
    if uses_github(&backups) {
        backup::save_settings(
            &dir,
            &BackupSettings {
                service: None,
                folder: None,
                keep_in_sync: false,
                ..backups
            },
        )?;
        crate::tray::refresh_sync_check(&app);
    }
    github::forget(&dir)?;
    let state = app.state::<GitHubState>();
    if let Ok(mut e) = state.error.lock() {
        *e = None;
    }
    let _ = app.emit(BACKUP_CHANGED, ());
    github_view(app).await
}

/// Open the device sign-in page, the token page, or the repo, in the browser.
#[tauri::command]
pub async fn github_open(page: String) -> Result<(), String> {
    let url = match page.as_str() {
        "device" => "https://github.com/login/device".to_owned(),
        "token" => TOKEN_PAGE.to_owned(),
        "repo" => {
            let repo = github::load_settings(&app_dir()?)
                .repo
                .ok_or("No repo picked yet.")?;
            format!("https://github.com/{}", repo.full_name())
        }
        _ => return Err("Unknown page.".into()),
    };
    patchpilot_local::open::open_url(&url).map_err(|e| e.to_string())
}

fn http() -> Client {
    client(Duration::from_secs(120))
}

/// For a push or an archive: up to 96 MB each way on a slow line.
fn long_http() -> Client {
    client(Duration::from_secs(3600))
}

fn client(timeout: Duration) -> Client {
    crate::sync::install_crypto();
    Client::builder()
        .user_agent(concat!("Patch Pilot/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

/// Send with the token; the status and JSON body (null when there's none).
/// When GitHub asks to slow down, waits as long as it says (a few minutes
/// at most) and asks again.
fn call(mut req: RequestBuilder, token: &str) -> Result<(StatusCode, Value), String> {
    for tries in 1.. {
        let again = req.try_clone();
        let res = req
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|e| format!("Can't reach GitHub: {e}"))?;
        let status = res.status();
        let wait = slow_down(&res);
        let body = res.json::<Value>().unwrap_or(Value::Null);
        match (wait, again) {
            (Some(wait), Some(next)) if tries < 4 && is_rate_limit(status, &body) => {
                std::thread::sleep(wait);
                req = next;
            }
            _ => return Ok((status, body)),
        }
    }
    unreachable!()
}

/// How long GitHub asked to wait, when it did.
fn slow_down(res: &reqwest::blocking::Response) -> Option<Duration> {
    let status = res.status();
    if status != StatusCode::FORBIDDEN && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }
    let header = |k: &str| {
        res.headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<i64>().ok())
    };
    let secs = header("retry-after").or_else(|| {
        (header("x-ratelimit-remaining") == Some(0))
            .then(|| header("x-ratelimit-reset").map(|t| t - patchpilot_local::now()))
            .flatten()
    });
    Some(Duration::from_secs(secs.unwrap_or(60).clamp(1, 300) as u64))
}

fn is_rate_limit(status: StatusCode, body: &Value) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || body["message"]
            .as_str()
            .is_some_and(|m| m.to_ascii_lowercase().contains("rate limit"))
}

fn api_error(status: StatusCode, body: &Value) -> String {
    let message = body["message"].as_str().unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED => {
            "The GitHub sign-in ran out. Sign in again on This machine.".into()
        }
        _ if is_rate_limit(status, body) => {
            "GitHub asked to slow down; picking up where it stopped in a few minutes.".into()
        }
        StatusCode::NOT_FOUND => {
            "GitHub can't find that repo (deleted, or this sign-in can't see it).".into()
        }
        _ => format!("GitHub said {}: {message}", status.as_u16()),
    }
}

/// The repo: reads through GitHub's REST API, pushes over Git's smart HTTP
/// protocol (`git-receive-pack`), the same way `git push` does.
struct Api {
    http: Client,
    long: Client,
    token: String,
    /// Git's sign-in takes a user name with the token.
    login: String,
    repo: RepoRef,
    /// Each commit's tree, read once.
    trees: Mutex<HashMap<String, String>>,
}

impl Api {
    fn new(token: &str, login: Option<&str>, repo: RepoRef) -> Self {
        Self {
            http: http(),
            long: long_http(),
            token: token.to_owned(),
            login: login.unwrap_or("x-access-token").to_owned(),
            repo,
            trees: Mutex::new(HashMap::new()),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{API}/repos/{}/{}/{path}", self.repo.owner, self.repo.name)
    }

    fn git_url(&self, path: &str) -> String {
        format!(
            "https://github.com/{}/{}.git/{path}",
            self.repo.owner, self.repo.name
        )
    }

    fn ok(&self, req: RequestBuilder) -> Result<Value, String> {
        let (status, body) = call(req, &self.token)?;
        if status.is_success() {
            Ok(body)
        } else {
            Err(api_error(status, &body))
        }
    }

    fn branch_head(&self) -> Result<(StatusCode, Value), String> {
        let url = self.url(&format!("git/ref/heads/{}", self.repo.branch));
        call(self.http.get(url), &self.token)
    }

    fn tree_of(&self, commit: &str) -> Result<String, String> {
        if let Some(t) = self.trees.lock().ok().and_then(|t| t.get(commit).cloned()) {
            return Ok(t);
        }
        let c = self.ok(self.http.get(self.url(&format!("git/commits/{commit}"))))?;
        let tree = c["tree"]["sha"]
            .as_str()
            .ok_or("GitHub sent a commit without files.")?
            .to_owned();
        if let Ok(mut t) = self.trees.lock() {
            t.insert(commit.to_owned(), tree.clone());
        }
        Ok(tree)
    }

    /// A Git request's answer, or what went wrong in words.
    fn git_ok(
        &self,
        res: reqwest::Result<reqwest::blocking::Response>,
    ) -> Result<reqwest::blocking::Response, String> {
        let res = res.map_err(|e| format!("Can't reach GitHub: {e}"))?;
        match res.status() {
            s if s.is_success() => Ok(res),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(format!(
                "GitHub didn't let this sign-in change {}. Sign in again with access to it.",
                self.repo.full_name()
            )),
            StatusCode::NOT_FOUND => Err(api_error(StatusCode::NOT_FOUND, &Value::Null)),
            s if s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error() => {
                Err("GitHub is busy; picking up where it stopped in a few minutes.".into())
            }
            s => Err(format!("GitHub said {}.", s.as_u16())),
        }
    }
}

const README: &str = "# Patch Pilot\n\nBackups and synced files from your machines, one folder per machine. \
Patch Pilot keeps this repo up to date; plug-ins and sample libraries are never here.\n";

impl Remote for Api {
    fn head(&self) -> Result<String, String> {
        let (mut status, mut body) = self.branch_head()?;
        // An empty repo has no branch yet: start it with a README.
        if status == StatusCode::CONFLICT || status == StatusCode::NOT_FOUND {
            let readme = json!({
                "message": "Start Patch Pilot backups",
                "content": base64::engine::general_purpose::STANDARD.encode(README),
            });
            let (s, b) = call(
                self.http.put(self.url("contents/README.md")).json(&readme),
                &self.token,
            )?;
            if !s.is_success() && s != StatusCode::UNPROCESSABLE_ENTITY {
                return Err(api_error(s, &b));
            }
            (status, body) = self.branch_head()?;
        }
        if !status.is_success() {
            return Err(api_error(status, &body));
        }
        body["object"]["sha"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "GitHub sent a branch without a commit.".into())
    }

    fn files(&self, commit: &str) -> Result<Vec<RemoteFile>, String> {
        let tree = self.tree_of(commit)?;
        let list = |tree: &str, recursive: bool| {
            let query = if recursive { "?recursive=1" } else { "" };
            let body = self.ok(self.http.get(self.url(&format!("git/trees/{tree}{query}"))))?;
            let entries = body["tree"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let is_tree = e["type"] == "tree";
                    let file = e["type"] == "blob"
                        && matches!(e["mode"].as_str(), Some("100644" | "100755"));
                    (is_tree || file).then(|| {
                        Some(github::TreeEntry {
                            path: e["path"].as_str()?.to_owned(),
                            id: e["sha"].as_str()?.to_owned(),
                            is_tree,
                        })
                    })?
                })
                .collect();
            Ok((entries, body["truncated"].as_bool() == Some(true)))
        };
        github::walk_tree(&list, &tree)
    }

    fn download(&self, id: &str) -> Result<Vec<u8>, String> {
        let res = self
            .http
            .get(self.url(&format!("git/blobs/{id}")))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github.raw+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|e| format!("Can't reach GitHub: {e}"))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.json::<Value>().unwrap_or(Value::Null);
            return Err(api_error(status, &body));
        }
        res.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Can't reach GitHub: {e}"))
    }

    fn archive(&self, commit: &str) -> Result<Box<dyn std::io::Read + '_>, String> {
        let res = self
            .long
            .get(self.url(&format!("tarball/{commit}")))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send();
        Ok(Box::new(self.git_ok(res)?))
    }

    fn receive_pack(&self, body: Vec<u8>) -> Result<Vec<u8>, String> {
        // Ask first, as `git push` does, so a sign-in that can't push stops
        // before the upload.
        let res = self
            .http
            .get(self.git_url("info/refs?service=git-receive-pack"))
            .basic_auth(&self.login, Some(&self.token))
            .send();
        self.git_ok(res)?;
        let res = self
            .long
            .post(self.git_url("git-receive-pack"))
            .basic_auth(&self.login, Some(&self.token))
            .header("Content-Type", "application/x-git-receive-pack-request")
            .header("Accept", "application/x-git-receive-pack-result")
            .body(body)
            .send();
        self.git_ok(res)?
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("The connection to GitHub dropped: {e}"))
    }
}
