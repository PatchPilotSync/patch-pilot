//! What each linked machine tells the others about its home network, over
//! the relay (decision #124, amends #123). One machine alone can't see that
//! the other is on another Wi-Fi, has a VPN on, doesn't use the home network
//! or never hears it; the other machine can say so. Pure: the app seals it
//! as `net/<machine id>` and moves the bytes.
//!
//! It goes out when what it says changes and, only while a linked machine is
//! missing or can't hear this one, every [`HEARTBEAT`] so the others know
//! this machine is still running. Nothing goes out while everything works.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Always this when written by this build. Newer fields are added with
/// defaults, so any version reads.
pub const REPORT_VERSION: u32 = 1;
/// While something is wrong, a report goes out at least this often.
pub const HEARTBEAT: Duration = Duration::from_secs(5 * 60);
/// A changed report waits this long after the last one, so a flapping
/// network doesn't keep writing to the relay.
pub const MIN_GAP: Duration = Duration::from_secs(30);
/// A report older than this says nothing about now: the machine is asleep,
/// Patch Pilot is closed, or it's offline. Seconds; over two heartbeats.
/// Measured on this machine's clock from when the report reached the relay,
/// so the other machine's clock never matters.
pub const FRESH_SECS: i64 = 12 * 60;
/// A subnet wider than this says nothing about which network it is.
const MIN_PREFIX: u8 = 8;
/// Biggest object this reads; a report is a few hundred bytes.
const MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetReport {
    #[serde(default)]
    pub version: u32,
    /// Unix seconds UTC, on the sending machine's clock.
    pub at: i64,
    /// Home network sync is picked there.
    pub on: bool,
    /// It could open its network port. Only meaningful while `on`.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub listening: bool,
    /// Since when it has listened (Unix seconds, its own clock): a machine
    /// that only just started can't have heard anyone yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listening_since: Option<i64>,
    /// Its home-network subnets (Wi-Fi, Ethernet): network address and
    /// prefix only. Empty when it hasn't looked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnets: Vec<Subnet>,
    /// Its broadcasts leave through a VPN. None when it hasn't looked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn: Option<bool>,
    /// Its broadcasts leave into a virtual network inside it (WSL, Hyper-V,
    /// Docker), by adapter name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_adapter: Option<String>,
    /// Its last broadcast failed to go out (macOS Local Network off, say):
    /// nothing it sends reaches anyone, whatever this machine does.
    #[serde(default, skip_serializing_if = "is_false")]
    pub send_error: bool,
    /// Its internet traffic leaves through a VPN, so `public` is the VPN's,
    /// not its home router's. None when it hasn't looked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internet_vpn: Option<bool>,
    /// Linked machines it hears on its network now, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hears: Vec<String>,
    /// Its public address as the relay saw it ([`public_tag`]): two
    /// machines with the same home subnet are on the same network only if
    /// this matches too, since most routers hand out the same few ranges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
}

impl Default for NetReport {
    fn default() -> Self {
        Self {
            version: 0,
            at: 0,
            on: false,
            listening: true,
            listening_since: None,
            subnets: Vec::new(),
            vpn: None,
            virtual_adapter: None,
            send_error: false,
            internet_vpn: None,
            hears: Vec::new(),
            public: None,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl NetReport {
    /// A machine that doesn't use the home network.
    pub fn off(at: i64) -> Self {
        Self {
            version: REPORT_VERSION,
            at,
            ..Self::default()
        }
    }

    /// Says the same as `other`, whenever each was made.
    pub fn same_news(&self, other: &Self) -> bool {
        Self {
            at: 0,
            ..self.clone()
        } == Self {
            at: 0,
            ..other.clone()
        }
    }

    /// It had listened for `secs` when it made this report, by its own
    /// clock. False when it doesn't say.
    pub fn listened_for(&self, secs: i64) -> bool {
        self.listening_since
            .is_some_and(|since| self.at - since >= secs)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a report serializes")
    }

    /// None for anything that isn't a report.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        serde_json::from_slice(bytes).ok()
    }
}

/// Another machine's report, with when it reached the relay on this
/// machine's clock. None when that's unknown: fetched from an older relay
/// that doesn't say how long it held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub report: NetReport,
    pub at: Option<i64>,
}

impl Received {
    /// Known to be recent enough to say something about now.
    pub fn is_fresh(&self, now: i64) -> bool {
        self.at.is_some_and(|at| now - at.min(now) <= FRESH_SECS)
    }
}

/// When a report reached the relay, on this machine's clock. `live`: it
/// just arrived as a change, so now. Otherwise now less how long the relay
/// says it held it, or unknown when it doesn't say.
pub fn received_at(now: i64, held_ms: Option<i64>, live: bool) -> Option<i64> {
    match held_ms {
        _ if live => Some(now),
        Some(ms) => Some(now - ms.max(0) / 1000),
        None => None,
    }
}

/// How a public address is compared: an IPv4 address whole (one per home
/// router), an IPv6 one by its /64 (one per home network).
pub fn public_tag(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => format!("v4:{v4}"),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => format!("v4:{v4}"),
            None => {
                let s = v6.segments();
                format!("v6:{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// Whether two [`public_tag`]s are the same place. None when they can't be
/// compared: one machine reached the relay over IPv4, the other over IPv6.
pub fn same_public(a: &str, b: &str) -> Option<bool> {
    (a.get(..3)? == b.get(..3)?).then(|| a == b)
}

/// An IPv4 network: its address with the host bits cleared, and the prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Subnet {
    pub net: Ipv4Addr,
    pub prefix: u8,
}

impl Subnet {
    /// The network `ip` is on. None for a prefix over 32, or one so wide
    /// (under /8) it would match nearly anything.
    pub fn of(ip: Ipv4Addr, prefix: u8) -> Option<Self> {
        (MIN_PREFIX..=32).contains(&prefix).then(|| Self {
            net: mask(ip, prefix),
            prefix,
        })
    }

    /// One holds the other: the same network, even when two machines were
    /// given different prefixes for it.
    pub fn overlaps(&self, other: &Self) -> bool {
        let p = self.prefix.min(other.prefix).min(32);
        mask(self.net, p) == mask(other.net, p)
    }
}

fn mask(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    let bits = match prefix {
        0 => 0,
        p => u32::MAX << (32 - u32::from(p.min(32))),
    };
    Ipv4Addr::from(u32::from(ip) & bits)
}

/// True when two machines' subnet lists share a network.
pub fn share_a_network(a: &[Subnet], b: &[Subnet]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.overlaps(y)))
}

/// Whether to send `new` now. `last`: the report last sent and how long ago.
/// `heartbeat`: a linked machine is missing or can't hear this one, so the
/// others should know this machine is still running.
pub fn should_send(new: &NetReport, last: Option<(&NetReport, Duration)>, heartbeat: bool) -> bool {
    match last {
        None => true,
        Some((sent, ago)) => {
            (!new.same_news(sent) && ago >= MIN_GAP) || (heartbeat && new.on && ago >= HEARTBEAT)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(a: u8, b: u8, c: u8, d: u8, prefix: u8) -> Subnet {
        Subnet::of(Ipv4Addr::new(a, b, c, d), prefix).unwrap()
    }

    fn on() -> NetReport {
        NetReport {
            version: REPORT_VERSION,
            at: 1_000,
            on: true,
            listening_since: Some(400),
            subnets: vec![net(192, 168, 1, 20, 24)],
            vpn: Some(false),
            internet_vpn: Some(false),
            hears: vec!["desktop".into()],
            public: Some("v4:203.0.113.5".into()),
            ..NetReport::default()
        }
    }

    #[test]
    fn a_subnet_keeps_only_the_network_part() {
        let s = net(192, 168, 1, 20, 24);
        assert_eq!(s.net, Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(net(10, 1, 2, 3, 8).net, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(net(10, 1, 2, 3, 32).net, Ipv4Addr::new(10, 1, 2, 3));
        assert_eq!(
            Subnet::of(Ipv4Addr::new(10, 1, 2, 3), 0),
            None,
            "matches anything"
        );
        assert_eq!(Subnet::of(Ipv4Addr::new(10, 1, 2, 3), 7), None);
        assert_eq!(Subnet::of(Ipv4Addr::LOCALHOST, 33), None);
    }

    #[test]
    fn networks_match_even_with_different_prefixes() {
        let home = net(192, 168, 1, 20, 24);
        assert!(home.overlaps(&net(192, 168, 1, 77, 24)));
        assert!(
            home.overlaps(&net(192, 168, 0, 5, 16)),
            "a /16 holds the /24"
        );
        assert!(!home.overlaps(&net(192, 168, 2, 5, 24)));
        assert!(!home.overlaps(&net(10, 0, 0, 5, 8)));
        assert!(share_a_network(
            &[net(10, 0, 0, 2, 24), home],
            &[net(192, 168, 1, 9, 24)]
        ));
        assert!(!share_a_network(&[home], &[]));
    }

    #[test]
    fn public_addresses_compare_within_one_family() {
        let a = public_tag("203.0.113.5".parse().unwrap());
        assert_eq!(a, "v4:203.0.113.5");
        assert_eq!(public_tag("::ffff:203.0.113.5".parse().unwrap()), a);
        let v6 = public_tag("2001:db8:1:2:aaaa::1".parse().unwrap());
        assert_eq!(v6, "v6:2001:db8:1:2::/64");
        assert_eq!(
            public_tag("2001:db8:1:2:bbbb::9".parse().unwrap()),
            v6,
            "the same home network's /64"
        );
        assert_eq!(same_public(&a, "v4:203.0.113.5"), Some(true));
        assert_eq!(same_public(&a, "v4:198.51.100.7"), Some(false));
        assert_eq!(same_public(&a, &v6), None, "IPv4 here, IPv6 there");
        assert_eq!(same_public("", &a), None);
    }

    #[test]
    fn a_report_round_trips_and_reads_older_and_newer_ones() {
        let r = on();
        assert_eq!(NetReport::from_bytes(&r.to_bytes()), Some(r.clone()));
        let json = String::from_utf8(r.to_bytes()).unwrap();
        assert!(json.contains(r#""net":"192.168.1.0""#), "{json}");
        assert!(
            !json.contains("listening\""),
            "listening is the default: {json}"
        );
        let deaf_port = NetReport {
            listening: false,
            ..on()
        };
        assert_eq!(
            NetReport::from_bytes(&deaf_port.to_bytes()).map(|r| r.listening),
            Some(false)
        );
        // A newer build's extra field, and a bare one.
        let newer = br#"{"version":2,"at":5,"on":true,"wifi":"Home"}"#;
        assert_eq!(NetReport::from_bytes(newer).map(|r| r.on), Some(true));
        let bare = NetReport::from_bytes(br#"{"at":5,"on":false}"#).unwrap();
        assert_eq!(
            bare,
            NetReport {
                at: 5,
                ..NetReport::default()
            }
        );
        assert!(bare.listening);
        assert_eq!(NetReport::from_bytes(b"not json"), None);
        assert_eq!(NetReport::from_bytes(&vec![b' '; MAX_BYTES + 1]), None);
        // Off says nothing else.
        let off = String::from_utf8(NetReport::off(5).to_bytes()).unwrap();
        assert_eq!(off, r#"{"version":1,"at":5,"on":false}"#);
    }

    #[test]
    fn fresh_means_recently_on_the_relay_by_this_machines_clock() {
        let r = Received {
            report: on(),
            at: Some(10_000),
        };
        assert!(r.is_fresh(10_000));
        assert!(r.is_fresh(10_000 + FRESH_SECS));
        assert!(!r.is_fresh(10_000 + FRESH_SECS + 1));
        assert!(r.is_fresh(10_000 - 60));
        let unknown = Received { at: None, ..r };
        assert!(!unknown.is_fresh(10_000), "an unknown age is never fresh");
        assert_eq!(received_at(10_000, Some(90_500), false), Some(10_000 - 90));
        assert_eq!(received_at(10_000, Some(-5), false), Some(10_000));
        assert_eq!(received_at(10_000, None, false), None, "an older relay");
        assert_eq!(
            received_at(10_000, None, true),
            Some(10_000),
            "just arrived"
        );
    }

    #[test]
    fn listened_for_counts_on_the_senders_own_clock() {
        let r = on();
        assert!(r.listened_for(600));
        assert!(!r.listened_for(601));
        assert!(
            !NetReport {
                listening_since: None,
                ..on()
            }
            .listened_for(0)
        );
    }

    #[test]
    fn sends_on_change_with_a_gap_and_heartbeats_only_while_something_is_wrong() {
        let sent = on();
        let later = NetReport { at: 2_000, ..on() };
        assert!(should_send(&later, None, false), "the first one");
        assert!(
            !should_send(&later, Some((&sent, Duration::from_secs(3600))), false),
            "nothing new, nothing wrong"
        );
        let moved = NetReport {
            subnets: vec![net(10, 0, 0, 2, 24)],
            ..later.clone()
        };
        assert!(
            !should_send(&moved, Some((&sent, MIN_GAP / 2)), false),
            "too soon"
        );
        assert!(should_send(&moved, Some((&sent, MIN_GAP)), false));
        assert!(!should_send(&later, Some((&sent, HEARTBEAT / 2)), true));
        assert!(should_send(&later, Some((&sent, HEARTBEAT)), true));
        let off = NetReport::off(2_000);
        assert!(
            !should_send(&off, Some((&NetReport::off(1), HEARTBEAT)), true),
            "off needs no heartbeat"
        );
    }
}
