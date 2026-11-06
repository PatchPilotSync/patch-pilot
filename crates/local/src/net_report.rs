//! What each linked machine tells the others about its home network, over
//! the relay (decision #124, amends #123). One machine alone can't see that
//! the other is on another Wi-Fi, has a VPN on, doesn't use the home network
//! or never hears it; the other machine can say so. Pure: the app seals it
//! as `net/<machine id>` and moves the bytes.
//!
//! It goes out when what it says changes and, only while a linked machine is
//! missing or can't hear this one, every [`HEARTBEAT`] so the others know
//! this machine is still running. Nothing goes out while everything works.

use std::net::Ipv4Addr;
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
pub const FRESH_SECS: i64 = 12 * 60;
/// Biggest object this reads; a report is a few hundred bytes.
const MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetReport {
    #[serde(default)]
    pub version: u32,
    /// Unix seconds UTC, on the sending machine's clock.
    pub at: i64,
    /// Home network sync is picked there and listening.
    pub on: bool,
    /// Its home-network subnets (Wi-Fi, Ethernet): network address and
    /// prefix only. Empty when it hasn't looked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnets: Vec<Subnet>,
    /// Its broadcasts leave through a VPN or a virtual adapter. None when it
    /// hasn't looked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn: Option<bool>,
    /// Linked machines it hears on its network now, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hears: Vec<String>,
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

    /// Made recently enough to say something about now. A clock a little
    /// ahead counts as now.
    pub fn is_fresh(&self, now: i64) -> bool {
        now - self.at.min(now) <= FRESH_SECS
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

/// An IPv4 network: its address with the host bits cleared, and the prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Subnet {
    pub net: Ipv4Addr,
    pub prefix: u8,
}

impl Subnet {
    /// The network `ip` is on. None for a prefix over 32.
    pub fn of(ip: Ipv4Addr, prefix: u8) -> Option<Self> {
        (prefix <= 32).then(|| Self {
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
            subnets: vec![net(192, 168, 1, 20, 24)],
            vpn: Some(false),
            hears: vec!["desktop".into()],
        }
    }

    #[test]
    fn a_subnet_keeps_only_the_network_part() {
        let s = net(192, 168, 1, 20, 24);
        assert_eq!(s.net, Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(net(10, 1, 2, 3, 8).net, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(net(10, 1, 2, 3, 32).net, Ipv4Addr::new(10, 1, 2, 3));
        assert_eq!(net(10, 1, 2, 3, 0).net, Ipv4Addr::UNSPECIFIED);
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
    fn a_report_round_trips_and_reads_older_and_newer_ones() {
        let r = on();
        assert_eq!(NetReport::from_bytes(&r.to_bytes()), Some(r));
        let json = String::from_utf8(on().to_bytes()).unwrap();
        assert!(json.contains(r#""net":"192.168.1.0""#), "{json}");
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
        assert_eq!(NetReport::from_bytes(b"not json"), None);
        assert_eq!(NetReport::from_bytes(&vec![b' '; MAX_BYTES + 1]), None);
        // Off says nothing else.
        let off = String::from_utf8(NetReport::off(5).to_bytes()).unwrap();
        assert_eq!(off, r#"{"version":1,"at":5,"on":false}"#);
    }

    #[test]
    fn fresh_means_recent_on_the_senders_clock() {
        let r = on();
        assert!(r.is_fresh(r.at));
        assert!(r.is_fresh(r.at + FRESH_SECS));
        assert!(!r.is_fresh(r.at + FRESH_SECS + 1));
        assert!(r.is_fresh(r.at - 60), "a clock a little ahead");
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
