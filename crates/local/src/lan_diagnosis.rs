//! Why the home network can't find a linked machine (decision #123, amends
//! #94). Pure: the app collects the facts (who's expected, who was heard,
//! which way this machine's packets leave, what Windows calls the network)
//! and [`diagnose`] turns them into what the Sync & backup line says. Nothing
//! here opens a socket or runs a command; [`crate::netinfo`] does the looking.
//!
//! A cause is only named when this machine can tell. What only the other
//! machine knows (it's on another Wi-Fi, its VPN is on, it never hears this
//! one) comes from its [`NetReport`] over the relay (decision #124) and is
//! named per machine as a [`Why`], only while that report is fresh.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;

use crate::lan::PORT;
use crate::net_report::{FRESH_SECS, Received, Subnet, same_public, share_a_network};

/// How long the line says "Looking for…" before it says "Can't find…". The
/// app checks every beacon (10 s), so in practice 30-40 s.
pub const GRACE: Duration = Duration::from_secs(30);

/// A machine the relay lists as linked to this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub id: String,
    pub name: String,
}

/// A linked machine heard on this network since Patch Pilot started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub id: String,
    pub name: String,
    /// Heard from in the last 45 s.
    pub here: bool,
    pub seen_at: i64,
    /// When this stretch of hearing it began (Unix seconds).
    pub since: i64,
}

/// A machine that's heard here only counts as unable to hear this one once
/// its report reached the relay this long after it was first heard here
/// (both on this machine's clock)…
const DEAF_AFTER_SECS: i64 = 90;
/// …and it had been listening this long when it made the report (on its
/// own clock): time enough for several beacons and the hello back, so a
/// machine that just restarted isn't taken as deaf.
const LISTEN_FIRST_SECS: i64 = 30;
/// A missing machine's report says how things are now only if it reached
/// the relay at least this long after this machine last heard it (two
/// beacons, for one lost and clocks in whole seconds): an earlier one was
/// made while it was still here, before it went to sleep, quit or moved.
const AFTER_HEARD_SECS: i64 = 20;
/// How long a report's `hears` keeps a machine after last hearing it (the
/// app's 45 s "gone"), plus a beacon: "it hears this machine" only means now
/// in a report made at least this long after this machine last heard it, or
/// after this machine started listening (it may have heard the run before).
const HEARS_FOR_SECS: i64 = 55;

/// A network interface: its name as the OS shows it ("Wi-Fi", "en0"), its
/// addresses with their prefix lengths, and on Windows the adapter's
/// description when it was looked up ("PANGP Virtual Ethernet Adapter"):
/// many VPNs name their adapter "Ethernet 4" and say what they are only there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Iface {
    pub name: String,
    pub ips: Vec<(IpAddr, u8)>,
    pub description: Option<String>,
}

impl Iface {
    pub fn kind(&self) -> AdapterKind {
        match &self.description {
            Some(d) => classify_adapter(&format!("{} {d}", self.name)),
            None => classify_adapter(&self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    /// Wi-Fi, Ethernet: the home network.
    Lan,
    Vpn,
    /// Hyper-V, WSL, Docker, VirtualBox, VMware: a network inside this machine.
    Virtual,
}

/// Which way packets leave this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Egress {
    Lan,
    Vpn {
        adapter: Option<String>,
    },
    Virtual {
        name: String,
    },
    /// The probe failed, or the address is on no interface while there's no
    /// home-network address to compare with.
    Unknown,
}

/// What Windows calls the network (Settings › Network › Network profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Windows' firewall blocks other machines from reaching this one.
    Public,
    /// Private, or a work domain.
    Private,
}

/// Everything [`diagnose`] goes on. The app fills it; tests build it by hand.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// How long this machine has listened on the home network.
    pub listening_for: Duration,
    /// Machines linked to this one (from the relay), this one left out.
    pub expected: Vec<Expected>,
    /// Linked machines heard since launch.
    pub seen: Vec<Seen>,
    /// A beacon of this rig that couldn't be read came in lately: another
    /// Patch Pilot version.
    pub other_version: bool,
    /// A Patch Pilot beacon under another rig key came in lately.
    pub other_rig: bool,
    /// The OS error code of the last broadcast that failed to go out.
    pub broadcast_error: Option<i32>,
    /// Which way a broadcast leaves.
    pub broadcast_egress: Option<Egress>,
    /// Which way internet traffic leaves.
    pub route_egress: Option<Egress>,
    /// This machine has an address on a home network (Wi-Fi, Ethernet).
    /// None: couldn't tell.
    pub has_lan: Option<bool>,
    /// Windows only; None elsewhere or when unknown.
    pub profile: Option<Profile>,
    pub macos: bool,
    /// This machine's id, to find it in the others' reports.
    pub this_id: String,
    /// Unix seconds now.
    pub now: i64,
    /// Connected to the relay, so a quiet machine is really quiet.
    pub relay_online: bool,
    /// This machine's home-network subnets, as last looked at; empty when
    /// it hasn't looked.
    pub subnets: Vec<Subnet>,
    /// This machine's public address as the relay saw it
    /// ([`crate::net_report::public_tag`]).
    pub public: Option<String>,
    /// The other machines' latest reports, by id (decision #124).
    pub reports: BTreeMap<String, Received>,
}

/// What the Sync & backup line says about the home network. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnosis {
    /// Linked machines on this network now, by name.
    pub here: Vec<String>,
    /// Linked machines not on this network now.
    pub missing: Vec<Missing>,
    /// Still in the first [`GRACE`]: "Looking for…", not "Can't find…".
    pub looking: bool,
    /// The most likely reason this machine can see, while one is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    /// Machines heard here that say they don't hear this one, by name: they
    /// get nothing from this machine.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deaf: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Missing {
    pub name: String,
    /// When it was last heard on this network, since launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_at: Option<i64>,
    /// What its own report says is wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<Why>,
}

/// Why one machine can't be found, from its own report. Mirrored in
/// `app/src/api.ts`; the words are in `Backup.tsx`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Why {
    /// Home network sync isn't picked there.
    Off,
    /// It's picked there, but Patch Pilot couldn't open its network port.
    CantListen,
    /// No report for a while: asleep, Patch Pilot closed, or offline.
    Quiet { since: i64 },
    /// Its broadcasts leave through a VPN.
    TheirVpn,
    /// Its broadcasts leave into a virtual network inside it.
    TheirVirtual { name: String },
    /// It hears this machine, but nothing it sends arrives here.
    OneWay,
    /// It's on another network: no home subnet in common, or the same
    /// range behind another router (another public address).
    OtherNetwork,
    /// The same network (same subnet and public address), both listening,
    /// no VPN either side, and neither hears the other: a firewall, or the
    /// router keeping devices apart.
    Blocked,
}

/// Mirrored in `app/src/api.ts`; the words are in `Backup.tsx`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// A beacon of this rig came in that this version can't read.
    OtherVersion,
    /// No Wi-Fi or Ethernet address: not on a network at all.
    NoLan,
    /// Broadcasts leave through a VPN instead of the home network.
    Vpn {
        #[serde(skip_serializing_if = "Option::is_none")]
        adapter: Option<String>,
    },
    /// Broadcasts leave into a virtual network inside this machine.
    VirtualAdapter { name: String },
    /// macOS refused to send on the local network: almost surely Local
    /// Network access is off for Patch Pilot.
    LocalNetworkDenied,
    /// Windows calls this network Public, so its firewall keeps other
    /// machines out.
    PublicNetwork,
    /// A Patch Pilot that isn't linked to these machines is on the network.
    OtherRig,
    /// A VPN carries internet traffic; broadcasts still leave on the home
    /// network, but many VPNs block local traffic. A guess.
    VpnOn {
        #[serde(skip_serializing_if = "Option::is_none")]
        adapter: Option<String>,
    },
}

/// macOS's EHOSTUNREACH: what a send to the local network gets while Local
/// Network access is off (or not answered yet).
const EHOSTUNREACH_MACOS: i32 = 65;

pub fn diagnose(f: &Facts) -> Diagnosis {
    let mut here: Vec<String> = f
        .seen
        .iter()
        .filter(|s| s.here)
        .map(|s| s.name.clone())
        .collect();
    here.sort();
    let missing: Vec<Missing> = f
        .expected
        .iter()
        .filter(|e| !f.seen.iter().any(|s| s.here && s.id == e.id))
        .map(|e| {
            let seen = f.seen.iter().find(|s| s.id == e.id);
            Missing {
                // The name it last broadcast is the freshest.
                name: seen.map_or_else(|| e.name.clone(), |s| s.name.clone()),
                seen_at: seen.map(|s| s.seen_at),
                why: why(f, &e.id, seen),
            }
        })
        .collect();
    let mut deaf: Vec<String> = f
        .seen
        .iter()
        .filter(|s| s.here && f.expected.iter().any(|e| e.id == s.id))
        .filter(|s| {
            f.reports.get(&s.id).is_some_and(|rx| {
                let r = &rx.report;
                r.on && r.listening
                    && rx.is_fresh(f.now)
                    && rx.at.is_some_and(|at| at >= s.since + DEAF_AFTER_SECS)
                    && r.listened_for(LISTEN_FIRST_SECS)
                    && !r.hears.contains(&f.this_id)
            })
        })
        .map(|s| s.name.clone())
        .collect();
    deaf.sort();
    let looking = !missing.is_empty() && f.listening_for < GRACE;
    let mut cause = if !missing.is_empty() && !looking {
        cause(f, !here.is_empty())
    } else {
        None
    };
    if cause.is_none() && !deaf.is_empty() {
        cause = deaf_cause(f);
    }
    Diagnosis {
        here,
        missing,
        looking,
        cause,
        deaf,
    }
}

/// What a missing machine's own report says, surest first. Only "off" holds
/// however old the report is: the machine would have said if it turned the
/// home network on.
fn why(f: &Facts, id: &str, seen: Option<&Seen>) -> Option<Why> {
    let rx = f.reports.get(id)?;
    let r = &rx.report;
    if !r.on {
        return Some(Why::Off);
    }
    // How old it is is unknown (an older relay): only "off" holds.
    let at = rx.at?;
    if !rx.is_fresh(f.now) {
        // Offline here, every report goes stale; and one heard here lately
        // was running, its report just hadn't needed refreshing.
        let heard_lately = seen.is_some_and(|s| f.now - s.seen_at < FRESH_SECS);
        let last = seen.map_or(at, |s| s.seen_at.max(at));
        return (f.relay_online && !heard_lately).then_some(Why::Quiet { since: last });
    }
    // Offline here, a newer report could be waiting unseen. And one that
    // reached the relay before this machine last heard that machine was
    // made while it was still here.
    if !f.relay_online || seen.is_some_and(|s| at < s.seen_at + AFTER_HEARD_SECS) {
        return None;
    }
    if !r.listening {
        return Some(Why::CantListen);
    }
    if r.vpn == Some(true) {
        return Some(Why::TheirVpn);
    }
    if let Some(name) = &r.virtual_adapter {
        return Some(Why::TheirVirtual { name: name.clone() });
    }
    let both_looked = !r.subnets.is_empty() && !f.subnets.is_empty();
    if both_looked && !share_a_network(&r.subnets, &f.subnets) {
        return Some(Why::OtherNetwork);
    }
    // Both sides' sends go where they should, so what it hears or doesn't
    // says something about the firewalls and the network between.
    let sends_fine = r.vpn == Some(false)
        && !r.send_error
        && matches!(f.broadcast_egress, Some(Egress::Lan))
        && f.broadcast_error.is_none();
    let listening_from = f.now - f.listening_for.as_secs() as i64;
    if r.hears.contains(&f.this_id) {
        // Its `hears` keeps a machine 45 s: it must postdate both this
        // machine losing it and this run starting.
        let current = at >= listening_from + HEARS_FOR_SECS
            && seen.is_none_or(|s| at >= s.seen_at + HEARS_FOR_SECS);
        return (sends_fine && current).then_some(Why::OneWay);
    }
    if !both_looked {
        return None;
    }
    // The same range. Most routers hand out the same few, so it's the same
    // network only if the public address matches too, and that only says
    // something when neither side's internet goes through a VPN.
    let direct = matches!(f.route_egress, Some(Egress::Lan)) && r.internet_vpn == Some(false);
    let same = match (&r.public, &f.public) {
        (Some(a), Some(b)) if direct => same_public(a, b)?,
        _ => return None,
    };
    if !same {
        return Some(Why::OtherNetwork);
    }
    // Same network: blocked only when neither side's packets go astray, and
    // it had time to hear this run's beacons.
    (sends_fine && at >= listening_from + AFTER_HEARD_SECS).then_some(Why::Blocked)
}

impl Diagnosis {
    /// A linked machine is missing that might still turn up: one whose
    /// report says it doesn't use the home network won't, and needs
    /// nothing more from this machine.
    pub fn awaiting(&self) -> bool {
        self.missing.iter().any(|m| m.why != Some(Why::Off))
    }
}

/// Why machines heard here can't hear this one, when this machine can
/// tell: its own packets leave the wrong way, or it runs a version they
/// can't read.
fn deaf_cause(f: &Facts) -> Option<Cause> {
    if f.other_version {
        return Some(Cause::OtherVersion);
    }
    match &f.broadcast_egress {
        Some(Egress::Vpn { adapter }) => Some(Cause::Vpn {
            adapter: adapter.clone(),
        }),
        Some(Egress::Virtual { name }) => Some(Cause::VirtualAdapter { name: name.clone() }),
        _ => None,
    }
}

/// The first cause that fits, surest first. While another linked machine is
/// here, this machine's network works, so only causes on the other side
/// (another version, another rig) are named.
fn cause(f: &Facts, someone_here: bool) -> Option<Cause> {
    if f.other_version {
        return Some(Cause::OtherVersion);
    }
    if !someone_here {
        if f.has_lan == Some(false) {
            return Some(Cause::NoLan);
        }
        match &f.broadcast_egress {
            Some(Egress::Vpn { adapter }) => {
                return Some(Cause::Vpn {
                    adapter: adapter.clone(),
                });
            }
            Some(Egress::Virtual { name }) => {
                return Some(Cause::VirtualAdapter { name: name.clone() });
            }
            _ => {}
        }
        // With no default route macOS refuses a broadcast with the same
        // error, so a working route to the internet is needed to tell.
        if f.macos
            && f.broadcast_error == Some(EHOSTUNREACH_MACOS)
            && f.has_lan == Some(true)
            && f.route_egress.is_some()
        {
            return Some(Cause::LocalNetworkDenied);
        }
        if f.profile == Some(Profile::Public) {
            return Some(Cause::PublicNetwork);
        }
    }
    if f.other_rig {
        return Some(Cause::OtherRig);
    }
    if !someone_here && let Some(Egress::Vpn { adapter }) = &f.route_egress {
        return Some(Cause::VpnOn {
            adapter: adapter.clone(),
        });
    }
    None
}

/// What kind of interface a name is. Names come from the OS: Windows'
/// adapter names ("Wi-Fi", "NordLynx", "vEthernet (WSL)") or the BSD names
/// on a Mac ("en0", "utun3", "bridge100").
pub fn classify_adapter(name: &str) -> AdapterKind {
    let n = name.to_lowercase();
    const VIRTUAL: &[&str] = &[
        "vethernet",
        "hyper-v",
        "wsl",
        "docker",
        "virtualbox",
        "vboxnet",
        "vmware",
        "vmnet",
        "bridge1",
        "anpi",
    ];
    const VPN: &[&str] = &[
        "vpn",
        "wireguard",
        "wintun",
        "nordlynx",
        "tailscale",
        "zerotier",
        "openvpn",
        "tap-windows",
        "tap0901",
        "utun",
        "ipsec",
        "mullvad",
        "proton",
        "surfshark",
        "anyconnect",
        "forti",
        "globalprotect",
        "pangp",
        "pulse secure",
        "cloudflare warp",
        "hamachi",
        "juniper",
        "sonicwall",
        "netextender",
        "check point",
        "zscaler",
        "nord",
        "expressvpn",
        "ppp",
    ];
    let starts = |p: &str| n.starts_with(p);
    if VIRTUAL.iter().any(|p| n.contains(p)) {
        AdapterKind::Virtual
    } else if VPN.iter().any(|p| n.contains(p)) || starts("tun") || starts("tap") || starts("wg") {
        AdapterKind::Vpn
    } else {
        AdapterKind::Lan
    }
}

/// An IPv4 address a home network would give: not loopback or link-local.
fn lan_v4(ip: &IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified())
}

/// What kind an interface is here. A "virtual" one carrying the internet
/// route is the home network itself: a Hyper-V external switch or bridged
/// WSL moves the machine's real address onto a "vEthernet" adapter, while an
/// internal switch never holds the default route.
fn kind_with_route(i: &Iface, route: Option<IpAddr>) -> AdapterKind {
    match i.kind() {
        AdapterKind::Virtual if route.is_some_and(|r| i.ips.iter().any(|(ip, _)| *ip == r)) => {
            AdapterKind::Lan
        }
        k => k,
    }
}

/// This machine's home-network subnets: the Wi-Fi and Ethernet ones,
/// sorted, for its [`NetReport`].
pub fn subnets(ifaces: &[Iface], route: Option<IpAddr>) -> Vec<Subnet> {
    let mut out: Vec<Subnet> = ifaces
        .iter()
        .filter(|i| kind_with_route(i, route) == AdapterKind::Lan)
        .flat_map(|i| &i.ips)
        .filter(|(ip, _)| lan_v4(ip))
        .filter_map(|(ip, prefix)| match ip {
            IpAddr::V4(v4) => Subnet::of(*v4, *prefix),
            IpAddr::V6(_) => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// True when some Wi-Fi or Ethernet interface has a home-network address.
/// `route`: the address internet traffic leaves from, if known.
pub fn has_lan(ifaces: &[Iface], route: Option<IpAddr>) -> bool {
    ifaces.iter().any(|i| {
        kind_with_route(i, route) == AdapterKind::Lan && i.ips.iter().any(|(ip, _)| lan_v4(ip))
    })
}

/// Which way packets from `source` leave. An address on no listed interface
/// is a VPN's when there's a home-network address to compare with: Windows
/// leaves VPN adapters without a hardware address (WireGuard, Wintun) out
/// of the interface list.
pub fn egress_kind(source: IpAddr, ifaces: &[Iface], route: Option<IpAddr>) -> Egress {
    if source.is_unspecified() || source.is_loopback() {
        return Egress::Unknown;
    }
    match ifaces
        .iter()
        .find(|i| i.ips.iter().any(|(ip, _)| *ip == source))
    {
        Some(i) => match kind_with_route(i, route) {
            AdapterKind::Lan => Egress::Lan,
            AdapterKind::Vpn => Egress::Vpn {
                adapter: Some(i.name.clone()),
            },
            AdapterKind::Virtual => Egress::Virtual {
                name: i.name.clone(),
            },
        },
        None if has_lan(ifaces, route) => Egress::Vpn { adapter: None },
        None => Egress::Unknown,
    }
}

/// What the Sync & backup line says when the home-network port can't be
/// opened, by what actually went wrong.
pub fn bind_error_text(kind: ErrorKind, raw_os_error: Option<i32>, detail: &str) -> String {
    // WSAEACCES: on Windows, usually a port range Hyper-V, WSL or Docker
    // reserved (`netsh int ipv4 show excludedportrange protocol=udp`).
    if raw_os_error == Some(10013) {
        return format!(
            "Can't listen for your other machines: Windows has reserved network port {PORT}, often for Hyper-V, WSL or Docker ({detail})."
        );
    }
    match kind {
        ErrorKind::AddrInUse => format!(
            "Can't listen for your other machines: another app is using network port {PORT} ({detail})."
        ),
        ErrorKind::PermissionDenied => format!(
            "Can't listen for your other machines: Patch Pilot isn't allowed to use network port {PORT} ({detail})."
        ),
        _ => format!("Can't listen for your other machines on network port {PORT} ({detail})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_report::NetReport;
    use std::net::Ipv4Addr;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn iface(name: &str, addr: IpAddr) -> Iface {
        Iface {
            name: name.into(),
            ips: vec![(addr, 24)],
            description: None,
        }
    }

    fn laptop() -> Expected {
        Expected {
            id: "laptop".into(),
            name: "Brian's Laptop".into(),
        }
    }

    /// Listening long enough, the laptop expected, nobody heard.
    fn alone() -> Facts {
        Facts {
            listening_for: GRACE,
            expected: vec![laptop()],
            has_lan: Some(true),
            broadcast_egress: Some(Egress::Lan),
            route_egress: Some(Egress::Lan),
            ..Facts::default()
        }
    }

    #[test]
    fn looks_for_a_while_before_saying_it_cant_find() {
        let f = Facts {
            listening_for: GRACE - Duration::from_secs(1),
            broadcast_egress: Some(Egress::Vpn { adapter: None }),
            ..alone()
        };
        let d = diagnose(&f);
        assert!(d.looking);
        assert_eq!(d.cause, None, "no cause while still looking");
        assert_eq!(d.missing[0].name, "Brian's Laptop");

        let d = diagnose(&Facts {
            listening_for: GRACE,
            ..f
        });
        assert!(!d.looking);
        assert_eq!(d.cause, Some(Cause::Vpn { adapter: None }));
    }

    #[test]
    fn nothing_to_say_when_no_machine_is_expected() {
        let d = diagnose(&Facts {
            expected: vec![],
            ..alone()
        });
        assert!(d.missing.is_empty());
        assert!(!d.looking);
        assert_eq!(d.cause, None);
    }

    #[test]
    fn no_cause_it_can_see_leaves_the_checklist() {
        let d = diagnose(&alone());
        assert_eq!(d.missing.len(), 1);
        assert_eq!(d.cause, None);
    }

    #[test]
    fn a_machine_that_left_keeps_when_it_was_seen_and_its_latest_name() {
        let f = Facts {
            seen: vec![Seen {
                id: "laptop".into(),
                name: "Laptop (renamed)".into(),
                here: false,
                seen_at: 1_700_000_000,
                since: 1_699_999_000,
            }],
            ..alone()
        };
        let d = diagnose(&f);
        assert_eq!(
            d.missing,
            vec![Missing {
                name: "Laptop (renamed)".into(),
                seen_at: Some(1_700_000_000),
                why: None,
            }]
        );
    }

    #[test]
    fn one_here_one_missing_names_only_causes_on_the_other_side() {
        let desk = Seen {
            id: "desk".into(),
            name: "Desktop".into(),
            here: true,
            seen_at: 1,
            since: 1,
        };
        let f = Facts {
            expected: vec![
                laptop(),
                Expected {
                    id: "desk".into(),
                    name: "Desktop".into(),
                },
            ],
            seen: vec![desk],
            profile: Some(Profile::Public),
            broadcast_egress: Some(Egress::Vpn { adapter: None }),
            ..alone()
        };
        let d = diagnose(&f);
        assert_eq!(d.here, vec!["Desktop".to_string()]);
        assert_eq!(d.missing.len(), 1);
        assert_eq!(
            d.cause, None,
            "this machine's network works: Desktop is here"
        );
        let d = diagnose(&Facts {
            other_rig: true,
            ..f
        });
        assert_eq!(d.cause, Some(Cause::OtherRig));
    }

    #[test]
    fn a_heard_machine_not_yet_expected_still_counts_as_here() {
        let d = diagnose(&Facts {
            expected: vec![],
            seen: vec![Seen {
                id: "mac".into(),
                name: "MacBook".into(),
                here: true,
                seen_at: 1,
                since: 1,
            }],
            ..alone()
        });
        assert_eq!(d.here, vec!["MacBook".to_string()]);
        assert!(d.missing.is_empty());
    }

    #[test]
    fn causes_come_surest_first() {
        let base = Facts {
            other_rig: true,
            profile: Some(Profile::Public),
            route_egress: Some(Egress::Vpn {
                adapter: Some("NordLynx".into()),
            }),
            ..alone()
        };
        // Another version beats everything: its beacon got here.
        let d = diagnose(&Facts {
            other_version: true,
            has_lan: Some(false),
            ..base.clone()
        });
        assert_eq!(d.cause, Some(Cause::OtherVersion));
        let d = diagnose(&Facts {
            has_lan: Some(false),
            ..base.clone()
        });
        assert_eq!(d.cause, Some(Cause::NoLan));
        let d = diagnose(&Facts {
            broadcast_egress: Some(Egress::Vpn {
                adapter: Some("NordLynx".into()),
            }),
            ..base.clone()
        });
        assert_eq!(
            d.cause,
            Some(Cause::Vpn {
                adapter: Some("NordLynx".into())
            })
        );
        let d = diagnose(&Facts {
            broadcast_egress: Some(Egress::Virtual {
                name: "vEthernet (WSL)".into(),
            }),
            ..base.clone()
        });
        assert_eq!(
            d.cause,
            Some(Cause::VirtualAdapter {
                name: "vEthernet (WSL)".into()
            })
        );
        assert_eq!(diagnose(&base).cause, Some(Cause::PublicNetwork));
        let d = diagnose(&Facts {
            profile: Some(Profile::Private),
            ..base.clone()
        });
        assert_eq!(d.cause, Some(Cause::OtherRig));
        let d = diagnose(&Facts {
            profile: Some(Profile::Private),
            other_rig: false,
            ..base
        });
        assert_eq!(
            d.cause,
            Some(Cause::VpnOn {
                adapter: Some("NordLynx".into())
            }),
            "a VPN on the internet route is only a guess, so it comes last"
        );
    }

    #[test]
    fn local_network_denied_only_on_a_mac_with_a_lan_address() {
        let mac = Facts {
            macos: true,
            broadcast_error: Some(65),
            ..alone()
        };
        assert_eq!(diagnose(&mac).cause, Some(Cause::LocalNetworkDenied));
        let no_gateway = Facts {
            route_egress: None,
            ..mac.clone()
        };
        assert_eq!(
            diagnose(&no_gateway).cause,
            None,
            "with no default route macOS refuses broadcasts the same way"
        );
        let pc = Facts {
            macos: false,
            ..mac.clone()
        };
        assert_eq!(
            diagnose(&pc).cause,
            None,
            "65 means something else on Windows"
        );
        let off_network = Facts {
            has_lan: None,
            ..mac
        };
        assert_eq!(diagnose(&off_network).cause, None);
    }

    #[test]
    fn adapters_are_told_apart_by_name() {
        for lan in [
            "Wi-Fi",
            "Ethernet",
            "Ethernet 2",
            "en0",
            "en7",
            "WLAN",
            "Local Area Connection",
        ] {
            assert_eq!(classify_adapter(lan), AdapterKind::Lan, "{lan}");
        }
        for vpn in [
            "NordLynx",
            "WireGuard Tunnel",
            "Tailscale",
            "ProtonVPN TUN",
            "OpenVPN Data Channel Offload",
            "TAP-Windows Adapter V9",
            "utun3",
            "ppp0",
            "ipsec0",
            "wg0",
            "Mullvad",
        ] {
            assert_eq!(classify_adapter(vpn), AdapterKind::Vpn, "{vpn}");
        }
        for virt in [
            "vEthernet (WSL)",
            "vEthernet (Default Switch)",
            "VirtualBox Host-Only Network",
            "VMware Network Adapter VMnet8",
            "docker0",
            "bridge100",
        ] {
            assert_eq!(classify_adapter(virt), AdapterKind::Virtual, "{virt}");
        }
    }

    #[test]
    fn egress_is_the_interface_holding_the_source_address() {
        let ifaces = vec![
            iface("Wi-Fi", ip(192, 168, 1, 20)),
            iface("vEthernet (WSL)", ip(172, 29, 0, 1)),
            iface("ProtonVPN TUN", ip(10, 2, 0, 2)),
        ];
        assert_eq!(egress_kind(ip(192, 168, 1, 20), &ifaces, None), Egress::Lan);
        assert_eq!(
            egress_kind(ip(172, 29, 0, 1), &ifaces, None),
            Egress::Virtual {
                name: "vEthernet (WSL)".into()
            }
        );
        assert_eq!(
            egress_kind(ip(10, 2, 0, 2), &ifaces, None),
            Egress::Vpn {
                adapter: Some("ProtonVPN TUN".into())
            }
        );
        // WireGuard-style adapters aren't listed at all.
        assert_eq!(
            egress_kind(ip(10, 5, 0, 2), &ifaces, None),
            Egress::Vpn { adapter: None }
        );
        assert_eq!(egress_kind(ip(0, 0, 0, 0), &ifaces, None), Egress::Unknown);
        // Nothing to compare with: can't tell.
        let no_lan = vec![iface("ProtonVPN TUN", ip(10, 2, 0, 2))];
        assert_eq!(egress_kind(ip(10, 5, 0, 2), &no_lan, None), Egress::Unknown);
    }

    #[test]
    fn a_vpn_named_like_ethernet_is_told_by_its_description() {
        // GlobalProtect calls its adapter "Ethernet 4".
        let mut gp = iface("Ethernet 4", ip(10, 20, 0, 7));
        let ifaces = vec![iface("Wi-Fi", ip(192, 168, 1, 20)), gp.clone()];
        assert_eq!(
            egress_kind(ip(10, 20, 0, 7), &ifaces, None),
            Egress::Lan,
            "the name alone can't tell"
        );
        gp.description = Some("PANGP Virtual Ethernet Adapter Secure #2".into());
        let ifaces = vec![iface("Wi-Fi", ip(192, 168, 1, 20)), gp];
        assert_eq!(
            egress_kind(ip(10, 20, 0, 7), &ifaces, None),
            Egress::Vpn {
                adapter: Some("Ethernet 4".into())
            }
        );
        let hyperv = Iface {
            description: Some("Hyper-V Virtual Ethernet Adapter".into()),
            ..iface("vEthernet (Default Switch)", ip(172, 20, 0, 1))
        };
        assert_eq!(hyperv.kind(), AdapterKind::Virtual);
        let wifi = Iface {
            description: Some("RZ616 Wi-Fi 6E 160MHz".into()),
            ..iface("Wi-Fi", ip(192, 168, 1, 20))
        };
        assert_eq!(wifi.kind(), AdapterKind::Lan);
    }

    #[test]
    fn a_virtual_switch_carrying_the_internet_is_the_home_network() {
        // Hyper-V external switch: the real address moves onto vEthernet.
        let ext = Iface {
            description: Some("Hyper-V Virtual Ethernet Adapter".into()),
            ..iface("vEthernet (External Switch)", ip(192, 168, 1, 20))
        };
        let wsl = iface("vEthernet (WSL)", ip(172, 29, 0, 1));
        let ifaces = vec![ext, wsl];
        let route = Some(ip(192, 168, 1, 20));
        assert!(has_lan(&ifaces, route));
        assert!(
            !has_lan(&ifaces, None),
            "without the route it looks virtual"
        );
        assert_eq!(
            egress_kind(ip(192, 168, 1, 20), &ifaces, route),
            Egress::Lan
        );
        // An internal switch still counts as virtual.
        assert_eq!(
            egress_kind(ip(172, 29, 0, 1), &ifaces, route),
            Egress::Virtual {
                name: "vEthernet (WSL)".into()
            }
        );
    }

    #[test]
    fn a_home_network_address_is_ipv4_on_wifi_or_ethernet() {
        assert!(has_lan(&[iface("Wi-Fi", ip(192, 168, 1, 20))], None));
        assert!(
            !has_lan(&[iface("Wi-Fi", ip(169, 254, 3, 4))], None),
            "link-local"
        );
        assert!(!has_lan(&[iface("Loopback", ip(127, 0, 0, 1))], None));
        assert!(!has_lan(&[iface("NordLynx", ip(10, 5, 0, 2))], None));
        assert!(!has_lan(&[], None));
    }

    #[test]
    fn bind_errors_say_what_went_wrong() {
        let reserved = bind_error_text(ErrorKind::PermissionDenied, Some(10013), "os error 10013");
        assert!(
            reserved.contains("Windows has reserved network port 47219"),
            "{reserved}"
        );
        let taken = bind_error_text(ErrorKind::AddrInUse, Some(10048), "os error 10048");
        assert!(
            taken.contains("another app is using network port 47219"),
            "{taken}"
        );
        let denied = bind_error_text(ErrorKind::PermissionDenied, Some(13), "os error 13");
        assert!(denied.contains("isn't allowed"), "{denied}");
        let other = bind_error_text(ErrorKind::Other, None, "boom");
        assert!(other.ends_with("(boom)."), "{other}");
    }

    #[test]
    fn the_line_gets_optional_fields_only_when_set() {
        let d = diagnose(&alone());
        let json = serde_json::to_value(&d).unwrap();
        assert!(json.get("cause").is_none());
        assert!(json["missing"][0].get("seen_at").is_none());
        let c = serde_json::to_value(Cause::Vpn { adapter: None }).unwrap();
        assert_eq!(c, serde_json::json!({ "kind": "vpn" }));
        let c = serde_json::to_value(Cause::VirtualAdapter {
            name: "docker0".into(),
        })
        .unwrap();
        assert_eq!(
            c,
            serde_json::json!({ "kind": "virtual_adapter", "name": "docker0" })
        );
    }

    const NOW: i64 = 1_800_000_000;
    const HOME_IP: &str = "v4:203.0.113.5";

    fn home() -> Subnet {
        Subnet::of(Ipv4Addr::new(192, 168, 1, 20), 24).unwrap()
    }

    /// The laptop's report: on and listening for ten minutes, fresh, on the
    /// home network behind the home router, no VPN, hears nobody.
    fn report() -> NetReport {
        NetReport {
            version: 1,
            at: NOW - 60,
            on: true,
            listening_since: Some(NOW - 660),
            subnets: vec![home()],
            vpn: Some(false),
            internet_vpn: Some(false),
            public: Some(HOME_IP.into()),
            ..NetReport::default()
        }
    }

    /// As if it reached the relay when it was made.
    fn rx(r: NetReport) -> Received {
        Received {
            at: Some(r.at),
            report: r,
        }
    }

    /// Alone for an hour, with the laptop's report in and this machine's
    /// network known.
    fn compared(r: NetReport) -> Facts {
        Facts {
            listening_for: Duration::from_secs(3600),
            this_id: "desk".into(),
            now: NOW,
            relay_online: true,
            subnets: vec![home()],
            public: Some(HOME_IP.into()),
            reports: [("laptop".to_string(), rx(r))].into(),
            ..alone()
        }
    }

    fn why_of(f: &Facts) -> Option<Why> {
        diagnose(f).missing[0].why.clone()
    }

    #[test]
    fn the_other_machines_report_says_why_surest_first() {
        assert_eq!(why_of(&compared(report())), Some(Why::Blocked));
        let off = NetReport {
            at: NOW - 86_400,
            ..NetReport::off(0)
        };
        assert_eq!(
            why_of(&compared(off)),
            Some(Why::Off),
            "off holds however old"
        );
        let old = NetReport {
            at: NOW - 3_600,
            ..report()
        };
        assert_eq!(
            why_of(&compared(old.clone())),
            Some(Why::Quiet { since: NOW - 3_600 })
        );
        assert_eq!(
            why_of(&Facts {
                relay_online: false,
                ..compared(old.clone())
            }),
            None,
            "offline here, every report looks old"
        );
        // Heard here a few minutes ago: running, its report just old.
        let lately = Facts {
            seen: vec![Seen {
                id: "laptop".into(),
                name: "Brian's Laptop".into(),
                here: false,
                seen_at: NOW - 120,
                since: NOW - 7_200,
            }],
            ..compared(old)
        };
        assert_eq!(why_of(&lately), None);
        let no_port = NetReport {
            listening: false,
            ..report()
        };
        assert_eq!(why_of(&compared(no_port)), Some(Why::CantListen));
        let vpn = NetReport {
            vpn: Some(true),
            hears: vec!["desk".into()],
            ..report()
        };
        assert_eq!(why_of(&compared(vpn)), Some(Why::TheirVpn));
        let hears_us = NetReport {
            hears: vec!["desk".into()],
            ..report()
        };
        assert_eq!(why_of(&compared(hears_us)), Some(Why::OneWay));
        let elsewhere = NetReport {
            subnets: vec![Subnet::of(Ipv4Addr::new(10, 0, 0, 7), 24).unwrap()],
            ..report()
        };
        assert_eq!(why_of(&compared(elsewhere)), Some(Why::OtherNetwork));
    }

    #[test]
    fn the_same_range_behind_another_router_is_another_network() {
        // A friend's router hands out 192.168.1.x too.
        let friends = NetReport {
            public: Some("v4:198.51.100.7".into()),
            ..report()
        };
        assert_eq!(why_of(&compared(friends.clone())), Some(Why::OtherNetwork));
        // Can't compare: IPv6 to the relay there, IPv4 here.
        let v6 = NetReport {
            public: Some("v6:2001:db8:1:2::/64".into()),
            ..report()
        };
        assert_eq!(why_of(&compared(v6)), None);
        // Either side's internet through a VPN: its public address is the
        // VPN's.
        let tunneled = NetReport {
            internet_vpn: Some(true),
            ..friends.clone()
        };
        assert_eq!(why_of(&compared(tunneled)), None);
        assert_eq!(
            why_of(&Facts {
                route_egress: Some(Egress::Vpn { adapter: None }),
                ..compared(friends.clone())
            }),
            None
        );
        // An older relay that doesn't say: same range, nothing to name.
        assert_eq!(
            why_of(&Facts {
                public: None,
                ..compared(friends)
            }),
            None
        );
        assert_eq!(
            why_of(&compared(NetReport {
                public: None,
                ..report()
            })),
            None,
            "not blocked without knowing it's the same router"
        );
    }

    #[test]
    fn a_report_names_nothing_it_cant_back() {
        // Neither side looked at its network yet.
        let unknown = NetReport {
            subnets: vec![],
            ..report()
        };
        assert_eq!(why_of(&compared(unknown)), None);
        assert_eq!(
            why_of(&Facts {
                subnets: vec![],
                ..compared(report())
            }),
            None
        );
        // Same network, but its VPN state unknown, or this machine's
        // broadcasts going astray: not "blocked".
        let vpn_unknown = NetReport {
            vpn: None,
            ..report()
        };
        assert_eq!(why_of(&compared(vpn_unknown)), None);
        assert_eq!(
            why_of(&Facts {
                broadcast_egress: Some(Egress::Unknown),
                ..compared(report())
            }),
            None
        );
        // No report at all: an older Patch Pilot there.
        assert_eq!(
            why_of(&Facts {
                reports: BTreeMap::new(),
                ..compared(report())
            }),
            None
        );
        // Another machine's report doesn't speak for the laptop.
        assert_eq!(
            why_of(&Facts {
                reports: [("studio".to_string(), rx(NetReport::off(NOW)))].into(),
                ..compared(report())
            }),
            None
        );
    }

    #[test]
    fn a_machine_heard_here_that_doesnt_hear_this_one_is_deaf() {
        let heard = |since: i64| Seen {
            id: "laptop".into(),
            name: "Brian's Laptop".into(),
            here: true,
            seen_at: NOW,
            since,
        };
        let f = Facts {
            seen: vec![heard(NOW - 600)],
            ..compared(report())
        };
        let d = diagnose(&f);
        assert!(d.missing.is_empty());
        assert_eq!(d.deaf, vec!["Brian's Laptop".to_string()]);
        assert_eq!(d.cause, None, "nothing wrong this machine can see");
        // Its report reached the relay before it could have heard this one.
        let early = Facts {
            seen: vec![heard(NOW - 100)],
            ..f.clone()
        };
        assert!(diagnose(&early).deaf.is_empty());
        // It had only just started (a quick restart, an update) when it
        // made the report, though this machine kept hearing it.
        let restarted = Facts {
            reports: [(
                "laptop".to_string(),
                rx(NetReport {
                    listening_since: Some(NOW - 70),
                    ..report()
                }),
            )]
            .into(),
            ..f.clone()
        };
        assert!(diagnose(&restarted).deaf.is_empty());
        // It does hear this one.
        let fine = compared(NetReport {
            hears: vec!["desk".into()],
            ..report()
        });
        assert!(
            diagnose(&Facts {
                seen: f.seen.clone(),
                ..fine
            })
            .deaf
            .is_empty()
        );
        // Another rig nearby has nothing to do with it.
        let d = diagnose(&Facts {
            other_rig: true,
            ..f.clone()
        });
        assert_eq!(d.cause, None);
        // This machine's broadcasts leave through a VPN: that's why.
        let d = diagnose(&Facts {
            broadcast_egress: Some(Egress::Vpn {
                adapter: Some("NordLynx".into()),
            }),
            ..f
        });
        assert_eq!(
            d.cause,
            Some(Cause::Vpn {
                adapter: Some("NordLynx".into())
            })
        );
    }

    fn left(seen_at: i64) -> Seen {
        Seen {
            id: "laptop".into(),
            name: "Brian's Laptop".into(),
            here: false,
            seen_at,
            since: seen_at - 3_600,
        }
    }

    #[test]
    fn a_report_from_before_the_machine_left_says_nothing_about_now() {
        // It heard this machine, then went to sleep 4 minutes ago; its last
        // report is from 8 minutes ago, when it was still here.
        let asleep = Facts {
            seen: vec![left(NOW - 240)],
            ..compared(NetReport {
                at: NOW - 480,
                hears: vec!["desk".into()],
                ..report()
            })
        };
        assert_eq!(why_of(&asleep), None);
        // A report from after it left does count.
        let moved = Facts {
            seen: vec![left(NOW - 240)],
            ..compared(NetReport {
                at: NOW - 60,
                vpn: Some(true),
                ..report()
            })
        };
        assert_eq!(why_of(&moved), Some(Why::TheirVpn));
        // Offline here: a newer report may be waiting unseen.
        assert_eq!(
            why_of(&Facts {
                relay_online: false,
                ..moved
            }),
            None
        );
        // Unknown age (an older relay): only "off".
        let mut unknown = compared(report());
        unknown.reports.get_mut("laptop").unwrap().at = None;
        assert_eq!(why_of(&unknown), None);
        unknown.reports.get_mut("laptop").unwrap().report.on = false;
        assert_eq!(why_of(&unknown), Some(Why::Off));
    }

    #[test]
    fn hearing_this_machine_only_counts_from_this_run_with_its_own_sends_fine() {
        let hears_us = NetReport {
            hears: vec!["desk".into()],
            ..report()
        };
        // It heard the Patch Pilot that ran here before this one.
        let restarted = Facts {
            listening_for: Duration::from_secs(40),
            ..compared(hears_us.clone())
        };
        assert_eq!(why_of(&restarted), None);
        // Its own VPN state unknown, or its broadcasts failing: this
        // machine's firewall isn't the likely reason.
        let unknown_vpn = NetReport {
            vpn: None,
            ..hears_us.clone()
        };
        assert_eq!(why_of(&compared(unknown_vpn)), None);
        let failing = NetReport {
            send_error: true,
            ..hears_us.clone()
        };
        assert_eq!(why_of(&compared(failing)), None);
        let failing_blocked = NetReport {
            send_error: true,
            ..report()
        };
        assert_eq!(why_of(&compared(failing_blocked)), None);
        // A virtual adapter there is its own reason, not "a VPN".
        let wsl = NetReport {
            virtual_adapter: Some("vEthernet (WSL)".into()),
            ..hears_us
        };
        assert_eq!(
            why_of(&compared(wsl)),
            Some(Why::TheirVirtual {
                name: "vEthernet (WSL)".into()
            })
        );
    }

    #[test]
    fn hearing_this_machine_needs_a_report_well_after_it_was_last_heard() {
        let hears_us = |at: i64| NetReport {
            at,
            hears: vec!["desk".into()],
            ..report()
        };
        // A beacon was lost, then it sent a heartbeat and closed its lid:
        // its "hears" still had this machine in it.
        let lid = Facts {
            seen: vec![left(NOW - 120)],
            ..compared(hears_us(NOW - 107))
        };
        assert_eq!(why_of(&lid), None);
        let later = Facts {
            seen: vec![left(NOW - 120)],
            ..compared(hears_us(NOW - 60))
        };
        assert_eq!(why_of(&later), Some(Why::OneWay));
        // It moved to another network while it still listed this machine.
        let moved = Facts {
            seen: vec![left(NOW - 120)],
            ..compared(NetReport {
                subnets: vec![Subnet::of(Ipv4Addr::new(10, 0, 0, 7), 24).unwrap()],
                ..hears_us(NOW - 100)
            })
        };
        assert_eq!(why_of(&moved), Some(Why::OtherNetwork));
        // It heard the run before this one, which quit moments ago.
        let rerun = Facts {
            listening_for: Duration::from_secs(90),
            ..compared(hears_us(NOW - 60))
        };
        assert_eq!(why_of(&rerun), None);
    }

    #[test]
    fn this_machines_own_failing_sends_rule_out_blocked_and_one_way() {
        let failing = Facts {
            broadcast_error: Some(10051),
            ..compared(report())
        };
        assert_eq!(why_of(&failing), None);
        let hears_us = Facts {
            broadcast_error: Some(10051),
            ..compared(NetReport {
                hears: vec!["desk".into()],
                ..report()
            })
        };
        assert_eq!(why_of(&hears_us), None);
    }

    #[test]
    fn quiet_counts_from_when_it_was_last_heard_here() {
        let old = NetReport {
            at: NOW - 5 * 3_600,
            ..report()
        };
        let d = Facts {
            seen: vec![left(NOW - 13 * 60)],
            ..compared(old)
        };
        assert_eq!(
            why_of(&d),
            Some(Why::Quiet {
                since: NOW - 13 * 60
            })
        );
    }

    #[test]
    fn a_machine_off_the_home_network_isnt_awaited() {
        let off = diagnose(&compared(NetReport::off(NOW)));
        assert!(!off.awaiting(), "it said it doesn't use the home network");
        assert!(diagnose(&compared(report())).awaiting());
        assert!(diagnose(&alone()).awaiting(), "no report: it might turn up");
    }

    #[test]
    fn subnets_come_from_wifi_and_ethernet_only() {
        let ifaces = vec![
            Iface {
                ips: vec![(ip(192, 168, 1, 20), 24), (ip(192, 168, 1, 21), 24)],
                ..iface("Wi-Fi", ip(0, 0, 0, 0))
            },
            iface("NordLynx", ip(10, 5, 0, 2)),
            iface("vEthernet (WSL)", ip(172, 29, 0, 1)),
            Iface {
                ips: vec![
                    (ip(169, 254, 3, 4), 16),
                    (ip(10, 9, 9, 9), 0),
                    ("fe80::1".parse().unwrap(), 64),
                ],
                ..iface("Ethernet", ip(0, 0, 0, 0))
            },
        ];
        assert_eq!(subnets(&ifaces, None), vec![home()]);
        assert!(subnets(&[], None).is_empty());
    }
}
