//! Sync over the home network (decision #94). While the backup folder is the
//! home-network copy in app-data, this machine broadcasts a sealed beacon on
//! the local network every few seconds, answers linked machines' sealed
//! requests for its copy over TCP, and pulls any folder a linked machine
//! nearby has a newer backup of. Keeping files in sync then places what came
//! over, exactly as from a cloud folder. The logic and the wire format live
//! in `patchpilot_local::lan`; this is the sockets and threads.
//!
//! The sockets open the first time the home network is picked (or at launch
//! when it already is) and stay open until Patch Pilot quits; while another
//! way of syncing is picked they send nothing and answer nobody.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use patchpilot_local::backup::{self, BackupSettings};
use patchpilot_local::github::FileIndex;
use patchpilot_local::lan::{self, Beacon, Connection, Progress, PullReport};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::backup::BACKUP_CHANGED;
use crate::sync::{app_dir, load_rig, this_machine};

/// How often a beacon goes out while the home network is in use.
const BEACON_EVERY: Duration = Duration::from_secs(10);
/// A machine not heard from for this long is taken as gone.
const GONE: Duration = Duration::from_secs(45);
/// After a pull that didn't finish, wait this long before trying that
/// machine again.
const RETRY: Duration = Duration::from_secs(60);
/// Linked machines answered at once; more wait to connect.
const MAX_CONNECTIONS: usize = 8;
/// Emitted with a [`Progress`] while a pull runs, then with null.
pub const LAN_PROGRESS: &str = "lan-progress";

struct Peer {
    name: String,
    addr: SocketAddr,
    seen: Instant,
    seen_at: i64,
    /// Its copy's state when last heard.
    state: String,
    /// The state last pulled in full.
    pulled: Option<String>,
    failed: Option<Instant>,
}

#[derive(Default)]
struct LanState {
    started: AtomicBool,
    /// The TCP port this machine answers on.
    port: Mutex<Option<u16>>,
    udp: Mutex<Option<Arc<UdpSocket>>>,
    peers: Mutex<HashMap<String, Peer>>,
    index: Arc<Mutex<FileIndex>>,
    /// One pull at a time.
    pulling: Mutex<()>,
    connections: Arc<AtomicUsize>,
    /// Why the network can't be used, or the last pull didn't finish.
    error: Mutex<Option<String>>,
    progress: Mutex<Option<Progress>>,
    last: Mutex<Option<LastPull>>,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct LastPull {
    pub at: i64,
    pub report: PullReport,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct LanPeer {
    pub machine_name: String,
    pub seen_at: i64,
    /// Heard from in the last minute or so.
    pub here: bool,
}

/// The home network on the Sync & backup line. Mirrored in `src/api.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct LanView {
    /// Linked machines heard on this network, most recent first.
    pub peers: Vec<LanPeer>,
    pub error: Option<String>,
    pub progress: Option<Progress>,
    pub last: Option<LastPull>,
}

pub fn setup(app: &AppHandle) {
    app.manage(LanState::default());
    if let Ok(dir) = app_dir() {
        *app.state::<LanState>().index.lock().unwrap() = lan::load_index(&dir);
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            if in_use() {
                start(&handle);
                announce(&handle, None);
            }
            std::thread::sleep(BEACON_EVERY);
        }
    });
}

/// The backup folder is the home-network copy.
pub fn in_use() -> bool {
    app_dir().is_ok_and(|d| uses_lan(&backup::load_settings(&d)))
}

fn uses_lan(settings: &BackupSettings) -> bool {
    settings.service.as_deref() == Some(lan::SERVICE)
}

/// Open the sockets, once.
fn start(app: &AppHandle) {
    let state = app.state::<LanState>();
    if state.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, lan::PORT))
        .and_then(|s| s.set_broadcast(true).map(|()| s));
    let udp = match udp {
        Ok(s) => Arc::new(s),
        Err(e) => {
            set_error(
                app,
                Some(format!(
                    "Can't listen for your other machines: another app is using network port {} ({e}).",
                    lan::PORT
                )),
            );
            state.started.store(false, Ordering::SeqCst);
            return;
        }
    };
    // The port others ask on; any free one will do when ours is taken.
    let tcp = TcpListener::bind((Ipv4Addr::UNSPECIFIED, lan::PORT))
        .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)));
    let tcp = match tcp {
        Ok(l) => l,
        Err(e) => {
            set_error(
                app,
                Some(format!(
                    "Can't open a network port for your other machines ({e})."
                )),
            );
            state.started.store(false, Ordering::SeqCst);
            return;
        }
    };
    *state.port.lock().unwrap() = tcp.local_addr().ok().map(|a| a.port());
    *state.udp.lock().unwrap() = Some(udp.clone());

    let handle = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; lan::MAX_BEACON];
        loop {
            let Ok((n, from)) = udp.recv_from(&mut buf) else {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };
            if !in_use() {
                continue;
            }
            let Ok(Some(rig)) = load_rig() else {
                continue;
            };
            if let Some(beacon) = lan::read_beacon(&rig, &buf[..n]) {
                heard(&handle, beacon, from.ip());
            }
        }
    });

    let handle = app.clone();
    std::thread::spawn(move || {
        for stream in tcp.incoming().flatten() {
            let state = handle.state::<LanState>();
            if !in_use() || state.connections.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                continue;
            }
            state.connections.fetch_add(1, Ordering::SeqCst);
            let connections = state.connections.clone();
            let index = state.index.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, &index);
                connections.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
}

fn serve(mut stream: TcpStream, index: &Mutex<FileIndex>) -> Result<(), String> {
    stream
        .set_read_timeout(Some(lan::TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(lan::TIMEOUT)))
        .map_err(|e| e.to_string())?;
    let rig = load_rig()?.ok_or("not linked")?;
    lan::answer(&mut stream, &rig, &lan::mirror_dir(&app_dir()?), index)
}

/// Send this machine's beacon to the whole network, or to one machine.
fn announce(app: &AppHandle, to: Option<IpAddr>) {
    let state = app.state::<LanState>();
    let (Some(udp), Some(port)) = (
        state.udp.lock().unwrap().clone(),
        *state.port.lock().unwrap(),
    ) else {
        return;
    };
    let (Ok(Some(rig)), Ok(this), Ok(dir)) = (load_rig(), this_machine(), app_dir()) else {
        return;
    };
    let beacon = Beacon {
        machine_name: crate::github::machine_name(&this),
        machine_id: this,
        port,
        state: lan::state(&lan::mirror_dir(&dir)),
    };
    let bytes = lan::beacon_bytes(&rig, &beacon);
    let mut targets: Vec<IpAddr> = match to {
        Some(ip) => vec![ip],
        None => {
            // Machines heard before too, for networks that drop broadcasts.
            let mut t: Vec<IpAddr> = state
                .peers
                .lock()
                .unwrap()
                .values()
                .map(|p| p.addr.ip())
                .collect();
            t.push(IpAddr::V4(Ipv4Addr::BROADCAST));
            t
        }
    };
    targets.sort();
    targets.dedup();
    for ip in targets {
        let _ = udp.send_to(&bytes, (ip, lan::PORT));
    }
}

/// Send a beacon now, so linked machines nearby pull a new backup soon.
pub fn backed_up(app: &AppHandle) {
    if in_use() {
        let handle = app.clone();
        std::thread::spawn(move || announce(&handle, None));
    }
}

fn heard(app: &AppHandle, beacon: Beacon, ip: IpAddr) {
    if this_machine().is_ok_and(|t| t == beacon.machine_id) {
        return;
    }
    let state = app.state::<LanState>();
    let (new, wanted) = {
        let mut peers = state.peers.lock().unwrap();
        let addr = SocketAddr::new(ip, beacon.port);
        let new = peers
            .get(&beacon.machine_id)
            .is_none_or(|p| p.seen.elapsed() > GONE);
        let peer = peers.entry(beacon.machine_id.clone()).or_insert(Peer {
            name: String::new(),
            addr,
            seen: Instant::now(),
            seen_at: 0,
            state: String::new(),
            pulled: None,
            failed: None,
        });
        peer.name = beacon.machine_name;
        peer.addr = addr;
        peer.seen = Instant::now();
        peer.seen_at = patchpilot_local::now();
        peer.state = beacon.state;
        let wanted = peer.pulled.as_ref() != Some(&peer.state)
            && peer.failed.is_none_or(|f| f.elapsed() > RETRY);
        (new, wanted)
    };
    if new {
        // Say hello straight back, so it doesn't wait for the next beacon.
        announce(app, Some(ip));
        let _ = app.emit(BACKUP_CHANGED, ());
    }
    if wanted {
        let handle = app.clone();
        let id = beacon.machine_id;
        std::thread::spawn(move || {
            if let Ok(true) = pull_from(&handle, &id, false) {
                // Keeping files in sync places what came over.
                if crate::autosync::enabled() {
                    crate::autosync::look(&handle);
                }
            }
        });
    }
}

/// Pull from every linked machine heard on the network lately, whatever
/// their state, before keeping files in sync looks (and for Check now).
/// Quietly nothing while another way of syncing is picked.
pub fn sync_now(app: &AppHandle) -> Result<(), String> {
    if !in_use() {
        return Ok(());
    }
    announce(app, None);
    let ids: Vec<String> = app
        .state::<LanState>()
        .peers
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, p)| p.seen.elapsed() < GONE)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        pull_from(app, &id, true)?;
    }
    Ok(())
}

/// Bring the copy up to date from one machine. True when anything changed.
/// `wait`: wait for a pull that's running instead of leaving it to that one.
fn pull_from(app: &AppHandle, id: &str, wait: bool) -> Result<bool, String> {
    let state = app.state::<LanState>();
    let _one = if wait {
        state.pulling.lock().map_err(|e| e.to_string())?
    } else {
        match state.pulling.try_lock() {
            Ok(g) => g,
            Err(_) => return Ok(false),
        }
    };
    let Some((addr, name, peer_state)) = state
        .peers
        .lock()
        .unwrap()
        .get(id)
        .map(|p| (p.addr, p.name.clone(), p.state.clone()))
    else {
        return Ok(false);
    };
    let rig = load_rig()?.ok_or("Link your machines first.")?;
    let dir = app_dir()?;
    let this = this_machine()?;
    // A copy of the index, so answering the other machine meanwhile never waits on it.
    let mut index = state.index.lock().unwrap().clone();
    let mut shown: Option<Instant> = None;
    let result = Connection::open(addr, &rig).and_then(|mut conn| {
        lan::pull(
            &mut conn,
            &lan::mirror_dir(&dir),
            &this,
            &mut index,
            &mut |done, total| {
                if total == 0 {
                    return;
                }
                let p = Progress {
                    from: name.clone(),
                    done,
                    total,
                };
                if let Ok(mut slot) = state.progress.lock() {
                    *slot = Some(p.clone());
                }
                if shown.is_none_or(|t| t.elapsed() >= Duration::from_millis(300)) || done == total
                {
                    shown = Some(Instant::now());
                    let _ = app.emit(LAN_PROGRESS, Some(p));
                }
            },
        )
    });
    *state.index.lock().unwrap() = index.clone();
    let _ = lan::save_index(&dir, &index);
    if let Ok(mut slot) = state.progress.lock() {
        *slot = None;
    }
    if shown.is_some() {
        let _ = app.emit(LAN_PROGRESS, None::<Progress>);
    }
    let had_error = state.error.lock().is_ok_and(|e| e.is_some());
    {
        let mut peers = state.peers.lock().unwrap();
        if let Some(p) = peers.get_mut(id) {
            match &result {
                Ok(_) => {
                    p.pulled = Some(peer_state);
                    p.failed = None;
                }
                Err(_) => p.failed = Some(Instant::now()),
            }
        }
    }
    set_error(
        app,
        result
            .as_ref()
            .err()
            .map(|e| format!("Couldn't copy from {name}: {e}")),
    );
    let changed = result.as_ref().is_ok_and(|r| r.copied + r.removed > 0);
    if let (true, Ok(r)) = (changed, &result) {
        *state.last.lock().unwrap() = Some(LastPull {
            at: patchpilot_local::now(),
            report: r.clone(),
        });
    }
    if changed || had_error || result.is_err() {
        let _ = app.emit(BACKUP_CHANGED, ());
    }
    result.map(|_| changed)
}

fn set_error(app: &AppHandle, error: Option<String>) {
    if let Ok(mut e) = app.state::<LanState>().error.lock() {
        *e = error;
    }
}

/// For the Sync & backup line, while the home network is in use.
pub fn view(app: &AppHandle) -> Option<LanView> {
    if !in_use() {
        return None;
    }
    let state = app.state::<LanState>();
    let mut peers: Vec<LanPeer> = state
        .peers
        .lock()
        .unwrap()
        .values()
        .map(|p| LanPeer {
            machine_name: p.name.clone(),
            seen_at: p.seen_at,
            here: p.seen.elapsed() < GONE,
        })
        .collect();
    peers.sort_by_key(|p| std::cmp::Reverse(p.seen_at));
    Some(LanView {
        peers,
        error: state.error.lock().unwrap().clone(),
        progress: state.progress.lock().unwrap().clone(),
        last: state.last.lock().unwrap().clone(),
    })
}

/// Sync over the home network from now on: the backup folder becomes the
/// copy in app-data, and the beacons start.
#[tauri::command]
pub async fn lan_use(app: AppHandle) -> Result<(), String> {
    if load_rig()?.is_none() {
        return Err(
            "Link this machine with your other machines first, so they can find each other.".into(),
        );
    }
    let dir = app_dir()?;
    std::fs::create_dir_all(lan::mirror_dir(&dir)).map_err(|e| e.to_string())?;
    let settings = BackupSettings {
        service: Some(lan::SERVICE.into()),
        folder: Some(lan::backup_folder(&dir)),
        ..backup::load_settings(&dir)
    };
    backup::save_settings(&dir, &settings)?;
    set_error(&app, None);
    start(&app);
    let _ = app.emit(BACKUP_CHANGED, ());
    // Get the other machines' backups first, then make this one's.
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        announce(&handle, None);
        // Give machines nearby a moment to answer the beacon.
        std::thread::sleep(Duration::from_secs(3));
        let _ = sync_now(&handle);
        crate::backup::auto_back_up(&handle);
        crate::autosync::look(&handle);
        let _ = handle.emit(BACKUP_CHANGED, ());
    });
    Ok(())
}
