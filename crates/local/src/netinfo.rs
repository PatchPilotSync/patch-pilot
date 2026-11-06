//! What this machine's network looks like, for [`crate::lan_diagnosis`]
//! (decision #123). Read-only and sends nothing: connecting a UDP socket only
//! asks the OS which address it would send from. Every probe gives `None`
//! when it can't tell, never a guess.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

use crate::lan::PORT;
use crate::lan_diagnosis::{Iface, Profile};

/// The address a home-network broadcast leaves from: on Windows, the
/// interface with the lowest-metric route, which is often a VPN's.
pub fn broadcast_source() -> Option<IpAddr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    // Connecting to the broadcast address needs this, or Windows refuses.
    s.set_broadcast(true).ok()?;
    s.connect((Ipv4Addr::BROADCAST, PORT)).ok()?;
    source(&s)
}

/// The address internet traffic leaves from (the default route).
pub fn route_source() -> Option<IpAddr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((Ipv4Addr::new(1, 1, 1, 1), 53)).ok()?;
    source(&s)
}

fn source(s: &UdpSocket) -> Option<IpAddr> {
    Some(s.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
}

/// The network interfaces the OS lists, with their addresses. On Windows,
/// VPN adapters without a hardware address (WireGuard, Wintun) aren't listed.
pub fn interfaces() -> Vec<Iface> {
    let networks = sysinfo::Networks::new_with_refreshed_list();
    let mut out: Vec<Iface> = networks
        .iter()
        .map(|(name, data)| Iface {
            name: name.clone(),
            ips: data
                .ip_networks()
                .iter()
                .map(|n| (n.addr, n.prefix))
                .collect(),
            description: None,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The interface holding `ip`, by name.
pub fn interface_of(ip: IpAddr, ifaces: &[Iface]) -> Option<&str> {
    ifaces
        .iter()
        .find(|i| i.ips.iter().any(|(a, _)| *a == ip))
        .map(|i| i.name.as_str())
}

/// What Windows says about the adapter named `alias`: its description
/// ("PANGP Virtual Ethernet Adapter") and whether it calls the network
/// Public or Private. Runs PowerShell (a second or so), so call it rarely
/// and off the UI's path. None on other systems, or when Windows doesn't say.
#[cfg(windows)]
pub fn adapter_info(alias: &str) -> Option<AdapterInfo> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    // No console window flashing up, and below a DAW's priority: the
    // calling thread's background mode doesn't pass to a child (#114).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    // WMI can hang; never wait on it longer than this.
    const DEADLINE: Duration = Duration::from_secs(5);
    // The name goes in through an environment variable, never into the
    // command text, and is compared exactly (-Name would take it as a
    // wildcard pattern).
    let script = "$n = $env:PATCHPILOT_IFACE; \
                  $a = Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object Name -eq $n; \
                  $p = Get-NetConnectionProfile -ErrorAction SilentlyContinue | Where-Object InterfaceAlias -eq $n; \
                  \"$($a.InterfaceDescription)|$($p.NetworkCategory)\"";
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("PATCHPILOT_IFACE", alias)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < DEADLINE => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // One short line, far under a pipe's buffer, so the child never blocks
    // on it before exiting.
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse_adapter_info(&out)
}

#[cfg(not(windows))]
pub fn adapter_info(_alias: &str) -> Option<AdapterInfo> {
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub description: Option<String>,
    pub profile: Option<Profile>,
}

/// `description|category` from [`adapter_info`]'s script. The category is
/// Public, Private or DomainAuthenticated (a work network, which the
/// firewall treats as trusted).
pub fn parse_adapter_info(out: &str) -> Option<AdapterInfo> {
    let line = out.lines().map(str::trim).find(|l| l.contains('|'))?;
    let (description, category) = line.rsplit_once('|')?;
    let profile = match category.trim() {
        "Public" => Some(Profile::Public),
        "Private" | "DomainAuthenticated" => Some(Profile::Private),
        _ => None,
    };
    let description = Some(description.trim().to_owned()).filter(|d| !d.is_empty());
    (description.is_some() || profile.is_some()).then_some(AdapterInfo {
        description,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_answer_or_say_they_cant_tell() {
        // Whatever network CI has; only that nothing panics or sends.
        let _ = broadcast_source();
        let _ = route_source();
        let ifaces = interfaces();
        if let Some(ip) = route_source() {
            let _ = interface_of(ip, &ifaces);
        }
    }

    /// Sends packets onto this machine's real networks, so not in CI:
    /// `cargo test -p patchpilot-local directed -- --ignored --nocapture`.
    #[test]
    #[ignore = "sends broadcasts on the real network"]
    fn directed_broadcasts_reach_a_listener_on_this_machine() {
        let targets = crate::lan_diagnosis::direct_broadcasts(&interfaces(), route_source());
        println!("direct targets: {targets:?}");
        assert!(!targets.is_empty(), "no Wi-Fi or Ethernet subnet here");
        let rx = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let port = rx.local_addr().unwrap().port();
        let tx = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        tx.set_broadcast(true).unwrap();
        for t in &targets {
            tx.send_to(b"pp-directed", (*t, port)).unwrap();
            let mut buf = [0u8; 32];
            let (n, from) = rx.recv_from(&mut buf).unwrap();
            println!("{t} -> received from {from}");
            assert_eq!(&buf[..n], b"pp-directed");
        }
    }

    #[test]
    fn adapter_info_reads_from_powershell_output() {
        assert_eq!(
            parse_adapter_info("RZ616 Wi-Fi 6E 160MHz|Private\r\n"),
            Some(AdapterInfo {
                description: Some("RZ616 Wi-Fi 6E 160MHz".into()),
                profile: Some(Profile::Private)
            })
        );
        assert_eq!(
            parse_adapter_info("\r\nPANGP Virtual Ethernet Adapter Secure #2|Public\r\n"),
            Some(AdapterInfo {
                description: Some("PANGP Virtual Ethernet Adapter Secure #2".into()),
                profile: Some(Profile::Public)
            })
        );
        assert_eq!(
            parse_adapter_info("Intel(R) Ethernet|DomainAuthenticated").and_then(|a| a.profile),
            Some(Profile::Private)
        );
        assert_eq!(
            parse_adapter_info("Wintun Userspace Tunnel|"),
            Some(AdapterInfo {
                description: Some("Wintun Userspace Tunnel".into()),
                profile: None
            })
        );
        assert_eq!(parse_adapter_info("|"), None);
        assert_eq!(parse_adapter_info(""), None);
    }
}
