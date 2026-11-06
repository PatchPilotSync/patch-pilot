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
//!
//! When a linked machine can't be found, the same 10 s loop works out why
//! (decision #123): `patchpilot_local::lan_diagnosis` decides from facts
//! gathered here, and `patchpilot_local::netinfo` looks at the network only
//! while a machine is missing, so an idle machine with everyone found does
//! no extra work.
//!
//! The same loop keeps this machine's home-network report on the relay
//! (decision #124): sent when it changes and, while a machine is missing or
//! can't hear this one, every few minutes. The other machines' reports come
//! back through `sync` and name what only they can see.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use patchpilot_local::backup::{self, BackupSettings};
use patchpilot_local::github::FileIndex;
use patchpilot_local::lan::{self, Beacon, Connection, Heard, Progress, PullReport};
use patchpilot_local::lan_diagnosis::{self, Diagnosis, Egress, Expected, Facts, Profile, Seen};
use patchpilot_local::net_report::{self, NetReport, Subnet};
use patchpilot_local::netinfo::{self, AdapterInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};

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
/// While a machine is missing, look at this machine's network this often.
const NET_EVERY: Duration = Duration::from_secs(30);
/// Ask Windows about an adapter (PowerShell, about a second) this often.
const ADAPTER_EVERY: Duration = Duration::from_secs(600);
/// …or this often while it couldn't say (WMI timed out, PowerShell blocked).
const ADAPTER_RETRY: Duration = Duration::from_secs(60);
/// A report's network facts older than this are left out: they were last
/// looked at during an earlier problem and may no longer hold.
const NET_FACTS_FOR: Duration = Duration::from_secs(90);
/// Emitted with a [`Progress`] while a pull runs, then with null.
pub const LAN_PROGRESS: &str = "lan-progress";

struct Peer {
    name: String,
    addr: SocketAddr,
    /// When last heard (Unix seconds). The wall clock, not an `Instant`: a
    /// Mac's stops while it sleeps.
    seen_at: i64,
    /// When this stretch of hearing it began (Unix seconds).
    since: i64,
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
    /// The machine the error is about, when it's a pull's: once that
    /// machine is gone, the error goes too.
    error_from: Mutex<Option<String>>,
    progress: Mutex<Option<Progress>>,
    last: Mutex<Option<LastPull>>,
    /// Since when this machine has listened, for "Looking for…" first.
    listening_since: Mutex<Option<Instant>>,
    /// The same as Unix seconds, for this machine's report: fixed once, so
    /// the report doesn't look changed every time it's built.
    listening_at: Mutex<Option<i64>>,
    /// When a beacon of this rig came in that this version can't read.
    other_version: Mutex<Option<Instant>>,
    /// When a Patch Pilot beacon under another rig key came in.
    other_rig: Mutex<Option<Instant>>,
    /// The OS error of the last broadcast that didn't go out.
    broadcast_error: Mutex<Option<i32>>,
    /// Every known machine's name by id, from the store.
    names: Mutex<HashMap<String, String>>,
    net: Mutex<NetCache>,
    /// The diagnosis last told to the UI.
    diagnosis: Mutex<Option<Diagnosis>>,
    /// When `sync` was last asked to send this machine's report: a put
    /// that keeps failing is tried again, but not every beacon.
    asked: Mutex<Option<Instant>>,
}

/// This machine's network as last looked at, while a machine was missing.
#[derive(Default, Clone)]
struct NetCache {
    /// When it was last looked at (Unix seconds): the wall clock, since a
    /// Mac's `Instant` stops while it sleeps and facts from before sleep
    /// would pass as fresh after waking somewhere else.
    checked_at: Option<i64>,
    broadcast_egress: Option<Egress>,
    route_egress: Option<Egress>,
    has_lan: Option<bool>,
    profile: Option<Profile>,
    /// The Wi-Fi and Ethernet subnets, for this machine's report.
    subnets: Vec<Subnet>,
    /// What Windows said about each adapter, by name, and when.
    adapters: HashMap<String, (Instant, Option<AdapterInfo>)>,
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
    /// Heard from in the last 45 s.
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
    /// Who's here, who's missing and why (decision #123).
    pub diagnosis: Diagnosis,
}

pub fn setup(app: &AppHandle) {
    app.manage(LanState::default());
    if let Ok(dir) = app_dir() {
        *app.state::<LanState>().index.lock().unwrap() = lan::load_index(&dir);
    }
    // Machines' names, for "Can't find Brian's Laptop": a new machine's scan
    // arrives over the link as `data-changed`.
    let handle = app.clone();
    app.listen_any("data-changed", move |_| {
        let handle = handle.clone();
        std::thread::spawn(move || refresh_names(&handle));
    });
    let handle = app.clone();
    std::thread::spawn(move || {
        refresh_names(&handle);
        loop {
            if in_use() {
                start(&handle);
                announce(&handle, None);
                check(&handle);
            }
            share(&handle);
            std::thread::sleep(BEACON_EVERY);
        }
    });
}

/// Every known machine's name by id, from the store.
fn refresh_names(app: &AppHandle) {
    let Ok(Ok(store)) = crate::rig::db_path().map(|db| crate::rig::open(&db)) else {
        return;
    };
    if let Ok(machines) = store.machines() {
        *app.state::<LanState>().names.lock().unwrap() =
            machines.into_iter().map(|m| (m.id, m.name)).collect();
    }
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
                Some(lan_diagnosis::bind_error_text(
                    e.kind(),
                    e.raw_os_error(),
                    &e.to_string(),
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
    state
        .listening_since
        .lock()
        .unwrap()
        .get_or_insert_with(Instant::now);
    state
        .listening_at
        .lock()
        .unwrap()
        .get_or_insert_with(patchpilot_local::now);

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
            let state = handle.state::<LanState>();
            match lan::read(&rig, &buf[..n]) {
                Heard::Ours(beacon) => heard(&handle, beacon, from.ip()),
                Heard::OtherVersion => *state.other_version.lock().unwrap() = Some(Instant::now()),
                Heard::OtherRig => *state.other_rig.lock().unwrap() = Some(Instant::now()),
                Heard::Noise => {}
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
    let broadcast = IpAddr::V4(Ipv4Addr::BROADCAST);
    for ip in targets {
        let sent = udp.send_to(&bytes, (ip, lan::PORT));
        // Only the broadcast says something about this machine's network: a
        // machine heard before that left answers "unreachable" too.
        if ip == broadcast {
            *state.broadcast_error.lock().unwrap() =
                sent.err().map(|e| e.raw_os_error().unwrap_or(-1));
        }
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
        let new = peers.get(&beacon.machine_id).is_none_or(|p| !is_here(p));
        let peer = peers.entry(beacon.machine_id.clone()).or_insert(Peer {
            name: String::new(),
            addr,
            seen_at: 0,
            since: 0,
            state: String::new(),
            pulled: None,
            failed: None,
        });
        peer.name = beacon.machine_name;
        peer.addr = addr;
        peer.seen_at = patchpilot_local::now();
        if new {
            peer.since = peer.seen_at;
        }
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
        .filter(|(_, p)| is_here(p))
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
    *state.error_from.lock().unwrap() = result.is_err().then(|| id.to_owned());
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
    let state = app.state::<LanState>();
    if let Ok(mut e) = state.error.lock() {
        *e = error;
    }
    if let Ok(mut from) = state.error_from.lock() {
        *from = None;
    }
}

/// Every beacon: drop a pull error about a machine that's gone, look at the
/// network if a machine is missing, and tell the UI when what it shows
/// would change. No store, no command, nothing at all while everyone's here.
fn check(app: &AppHandle) {
    let state = app.state::<LanState>();
    if clear_stale_error(&state) {
        let _ = app.emit(BACKUP_CHANGED, ());
    }
    let mut d = lan_diagnosis::diagnose(&facts(app));
    let net_due = state
        .net
        .lock()
        .unwrap()
        .checked_at
        .is_none_or(|t| !net_age_within(t, NET_EVERY));
    // Only while something is wrong: a machine that might still turn up is
    // missing (not one that said it doesn't use the home network), or one
    // here can't hear this one.
    let wrong = (d.awaiting() && !d.looking) || !d.deaf.is_empty();
    if wrong && net_due {
        look_at_network(app);
        d = lan_diagnosis::diagnose(&facts(app));
    }
    let changed = {
        let mut last = state.diagnosis.lock().unwrap();
        let changed = last.as_ref() != Some(&d);
        *last = Some(d);
        changed
    };
    if changed {
        let _ = app.emit(BACKUP_CHANGED, ());
    }
}

/// Drop a pull error about a machine that has left. The check and the clear
/// happen under the error's own lock, so a newer error from another machine
/// that lands meanwhile stays.
fn clear_stale_error(state: &LanState) -> bool {
    let Ok(mut from) = state.error_from.lock() else {
        return false;
    };
    let gone = from.as_ref().is_some_and(|id| {
        state
            .peers
            .lock()
            .unwrap()
            .get(id)
            .is_none_or(|p| !is_here(p))
    });
    if gone {
        *from = None;
        if let Ok(mut e) = state.error.lock() {
            *e = None;
        }
    }
    gone
}

/// Which way this machine's packets leave, its subnets, and what Windows
/// calls the network. At background priority: it can run PowerShell, at
/// most every 10 minutes per adapter.
fn look_at_network(app: &AppHandle) {
    let state = app.state::<LanState>();
    let _quiet = patchpilot_local::quiet::background();
    let mut net = state.net.lock().unwrap().clone();
    let mut ifaces = netinfo::interfaces();
    let broadcast = netinfo::broadcast_source();
    let route = netinfo::route_source();
    // What Windows says about the adapters the packets leave on, since many
    // VPNs call theirs "Ethernet 4" and say what they are only there.
    let mut broadcast_info = None;
    let mut undescribed: Vec<String> = Vec::new();
    for (src, is_broadcast) in [(broadcast, true), (route, false)] {
        let Some(name) = src
            .and_then(|ip| netinfo::interface_of(ip, &ifaces))
            .map(str::to_owned)
        else {
            continue;
        };
        let fresh = net
            .adapters
            .get(&name)
            .filter(|(at, info)| {
                at.elapsed()
                    < if info.is_some() {
                        ADAPTER_EVERY
                    } else {
                        ADAPTER_RETRY
                    }
            })
            .map(|(_, info)| info.clone());
        // Without it a VPN called "Ethernet 4" passes for the home network,
        // so ask even while another machine is here.
        let info = fresh.unwrap_or_else(|| {
            let info = netinfo::adapter_info(&name);
            net.adapters
                .insert(name.clone(), (Instant::now(), info.clone()));
            info
        });
        if let Some(i) = ifaces.iter_mut().find(|i| i.name == name) {
            i.description = info.as_ref().and_then(|a| a.description.clone());
        }
        if info.is_none() {
            undescribed.push(name.clone());
        }
        if is_broadcast {
            broadcast_info = info;
        }
    }
    // On Windows a VPN often calls its adapter "Ethernet 4" and says what it
    // is only in the description: without one, the name alone can't tell.
    let egress = |ip: IpAddr| {
        let kind = lan_diagnosis::egress_kind(ip, &ifaces, route);
        let unsure = cfg!(windows)
            && kind == Egress::Lan
            && netinfo::interface_of(ip, &ifaces)
                .is_some_and(|n| undescribed.iter().any(|u| u == n));
        if unsure { Egress::Unknown } else { kind }
    };
    net.broadcast_egress = broadcast.map(egress);
    net.route_egress = route.map(egress);
    let subnets = lan_diagnosis::subnets(&ifaces, route);
    if !net.subnets.is_empty() && subnets != net.subnets {
        // Another network: the public address the relay saw belongs to the
        // old one until the next upload says.
        crate::sync::forget_public(app);
    }
    net.subnets = subnets;
    // Windows lists no adapter at all when none is connected; with no route
    // anywhere either, this machine isn't on a network.
    net.has_lan = if !ifaces.is_empty() {
        Some(lan_diagnosis::has_lan(&ifaces, route))
    } else if broadcast.is_none() && route.is_none() {
        Some(false)
    } else {
        None
    };
    // The profile only matters for the home network itself.
    net.profile = match net.broadcast_egress {
        Some(Egress::Lan) => broadcast_info.and_then(|a| a.profile),
        _ => None,
    };
    net.checked_at = Some(patchpilot_local::now());
    *state.net.lock().unwrap() = net;
}

/// What [`lan_diagnosis::diagnose`] goes on, from what's already known:
/// quick, so the UI can ask any time.
fn facts(app: &AppHandle) -> Facts {
    let state = app.state::<LanState>();
    let recent =
        |slot: &Mutex<Option<Instant>>| slot.lock().unwrap().is_some_and(|t| t.elapsed() < GONE);
    let seen: Vec<Seen> = state
        .peers
        .lock()
        .unwrap()
        .iter()
        .map(|(id, p)| Seen {
            id: id.clone(),
            name: p.name.clone(),
            here: is_here(p),
            seen_at: p.seen_at,
            since: p.since,
        })
        .collect();
    let mut net = state.net.lock().unwrap().clone();
    if !net
        .checked_at
        .is_some_and(|t| net_age_within(t, NET_FACTS_FOR))
    {
        // From an earlier look that may no longer hold: as if never looked.
        net = NetCache::default();
    }
    Facts {
        listening_for: state
            .listening_since
            .lock()
            .unwrap()
            .map_or(Duration::ZERO, |t| t.elapsed()),
        expected: expected(app),
        seen,
        other_version: recent(&state.other_version),
        other_rig: recent(&state.other_rig),
        broadcast_error: *state.broadcast_error.lock().unwrap(),
        broadcast_egress: net.broadcast_egress,
        route_egress: net.route_egress,
        has_lan: net.has_lan,
        profile: net.profile,
        macos: cfg!(target_os = "macos"),
        this_id: this_machine().unwrap_or_default(),
        now: patchpilot_local::now(),
        relay_online: crate::sync::online(app),
        subnets: net.subnets,
        public: crate::sync::public(app),
        reports: crate::sync::reports(app),
    }
}

/// A wall-clock time at most `within` ago. A clock stepped back makes it
/// "not within", so the network is looked at again and old facts aren't
/// taken as fresh.
fn net_age_within(t: i64, within: Duration) -> bool {
    let age = patchpilot_local::now() - t;
    (0..within.as_secs() as i64).contains(&age)
}

/// Heard from in the last 45 s.
fn is_here(p: &Peer) -> bool {
    patchpilot_local::now() - p.seen_at < GONE.as_secs() as i64
}

/// What this machine tells the others about its home network: on and
/// listening, its subnets and whether its broadcasts go astray as last
/// looked at, and who it hears now. Quick: no store, no command.
pub(crate) fn own_report(app: &AppHandle) -> NetReport {
    let now = patchpilot_local::now();
    // Sync can start before the home network is set up.
    let Some(state) = app.try_state::<LanState>() else {
        return NetReport::off(now);
    };
    if !in_use() {
        return NetReport::off(now);
    }
    if state.udp.lock().unwrap().is_none() {
        // Picked, but the port wouldn't open: this machine says why itself.
        return NetReport {
            on: true,
            listening: false,
            ..NetReport::off(now)
        };
    }
    let net = state.net.lock().unwrap().clone();
    // Facts from an earlier problem may no longer hold: say nothing then.
    let recent = net
        .checked_at
        .is_some_and(|t| net_age_within(t, NET_FACTS_FOR));
    // A virtual adapter isn't a VPN: its own field, so the others can say so.
    let through_vpn = |e: &Option<Egress>| match e {
        Some(Egress::Lan | Egress::Virtual { .. }) => Some(false),
        Some(Egress::Vpn { .. }) => Some(true),
        Some(Egress::Unknown) | None => None,
    };
    let virtual_adapter = match &net.broadcast_egress {
        Some(Egress::Virtual { name }) if recent => Some(name.clone()),
        _ => None,
    };
    let mut hears: Vec<String> = state
        .peers
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, p)| is_here(p))
        .map(|(id, _)| id.clone())
        .collect();
    hears.sort();
    let listening_since = *state.listening_at.lock().unwrap();
    let send_error = state.broadcast_error.lock().unwrap().is_some();
    NetReport {
        version: net_report::REPORT_VERSION,
        at: now,
        on: true,
        listening: true,
        listening_since,
        subnets: if recent { net.subnets } else { Vec::new() },
        vpn: if recent {
            through_vpn(&net.broadcast_egress)
        } else {
            None
        },
        virtual_adapter,
        send_error,
        internet_vpn: if recent {
            through_vpn(&net.route_egress)
        } else {
            None
        },
        hears,
        public: crate::sync::public(app),
    }
}

/// Every beacon: have `sync` send this machine's report when it says
/// something new, or, while a machine is missing or can't hear this one,
/// when the others should hear it's still running. Not linked, `sync`
/// isn't running and nothing goes out.
fn share(app: &AppHandle) {
    let state = app.state::<LanState>();
    let report = own_report(app);
    let heartbeat = report.on
        && state
            .diagnosis
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|d| d.awaiting() || !d.deaf.is_empty());
    // Against what actually reached the relay, so a failed put is retried.
    let last = crate::sync::last_report(app);
    if !net_report::should_send(&report, last.as_ref().map(|(r, ago)| (r, *ago)), heartbeat) {
        return;
    }
    // Offline, the next session sends one at its start anyway.
    if !crate::sync::online(app) {
        return;
    }
    let mut asked = state.asked.lock().unwrap();
    if asked.is_none_or(|t| t.elapsed() >= net_report::MIN_GAP) {
        *asked = Some(Instant::now());
        crate::sync::share_report(app);
    }
}

/// The machines this one should find: the ones the relay lists as linked
/// (machines added from an export file never run Patch Pilot with this rig)
/// that are still in the store (a removed machine stays on the relay).
/// Before the first listing, every other machine in the store. None at all
/// while this machine isn't linked.
fn expected(app: &AppHandle) -> Vec<Expected> {
    if !matches!(load_rig(), Ok(Some(_))) {
        return Vec::new();
    }
    let names = app.state::<LanState>().names.lock().unwrap().clone();
    let this = this_machine().ok();
    let ids = crate::sync::linked_machines(app).unwrap_or_else(|| names.keys().cloned().collect());
    let mut out: Vec<Expected> = ids
        .into_iter()
        .filter(|id| Some(id) != this.as_ref())
        .filter_map(|id| {
            Some(Expected {
                name: names.get(&id)?.clone(),
                id,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
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
            here: is_here(p),
        })
        .collect();
    peers.sort_by_key(|p| std::cmp::Reverse(p.seen_at));
    Some(LanView {
        peers,
        error: state.error.lock().unwrap().clone(),
        progress: state.progress.lock().unwrap().clone(),
        last: state.last.lock().unwrap().clone(),
        diagnosis: lan_diagnosis::diagnose(&facts(app)),
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
    // Picking the home network (again) starts the "Looking for…" time over.
    let state = app.state::<LanState>();
    *state.listening_since.lock().unwrap() = Some(Instant::now());
    *state.listening_at.lock().unwrap() = Some(patchpilot_local::now());
    *state.diagnosis.lock().unwrap() = None;
    {
        // Look at the network afresh; what Windows said about each adapter
        // keeps.
        let mut net = state.net.lock().unwrap();
        *net = NetCache {
            adapters: std::mem::take(&mut net.adapters),
            ..NetCache::default()
        };
    }
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
