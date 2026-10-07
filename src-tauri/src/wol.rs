//! Wake-on-LAN: a "magic packet" (six 0xFF bytes, then the machine's MAC address sixteen times) sent as a UDP
//! broadcast. It only works for a machine on the same network as this computer, and the machine must be set up
//! to wake (BIOS and network card), which nothing here can check.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WolError {
    #[error("that isn't a MAC address (six pairs of hex digits, like 00:1A:2B:3C:4D:5E)")]
    BadMac,
    #[error("couldn't use the broadcast address {0}")]
    BadAddress(String),
    #[error("couldn't send the packet: {0}")]
    Send(String),
}

/// Six bytes from `00:1A:2B:3C:4D:5E`, `00-1A-2B-3C-4D-5E`, `001A.2B3C.4D5E` or `001A2B3C4D5E`.
pub fn parse_mac(text: &str) -> Result<[u8; 6], WolError> {
    let digits: Vec<char> = text.trim().chars().filter(|c| !matches!(c, ':' | '-' | '.')).collect();
    if digits.len() != 12 || !digits.iter().all(char::is_ascii_hexdigit) {
        return Err(WolError::BadMac);
    }
    // Separators only where a MAC has them: five colons or dashes, or two dots (the Cisco style), or none.
    let t = text.trim();
    let count = |c: char| t.chars().filter(|x| *x == c).count();
    let (colons, dashes, dots) = (count(':'), count('-'), count('.'));
    let ok = matches!((colons, dashes, dots), (0, 0, 0) | (5, 0, 0) | (0, 5, 0) | (0, 0, 2));
    if !ok {
        return Err(WolError::BadMac);
    }
    let mut mac = [0u8; 6];
    for (i, pair) in digits.chunks(2).enumerate() {
        mac[i] = u8::from_str_radix(&pair.iter().collect::<String>(), 16).map_err(|_| WolError::BadMac)?;
    }
    Ok(mac)
}

pub fn magic_packet(mac: &[u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFFu8; 6];
    for _ in 0..16 {
        p.extend_from_slice(mac);
    }
    p
}

/// The address to send to: empty means the whole local network (255.255.255.255, port 9). A bare address gets
/// port 9.
pub fn destination(broadcast: &str) -> Result<SocketAddr, WolError> {
    let text = broadcast.trim();
    let with_port = if text.is_empty() {
        "255.255.255.255:9".to_string()
    } else if text.contains(':') || text.starts_with('[') {
        text.to_string()
    } else {
        format!("{text}:9")
    };
    with_port.to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or_else(|| WolError::BadAddress(text.to_string()))
}

/// Send the packet for `mac` to `to`.
pub fn wake(mac: &str, to: SocketAddr) -> Result<(), WolError> {
    let mac = parse_mac(mac)?;
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| WolError::Send(e.to_string()))?;
    socket.set_broadcast(true).map_err(|e| WolError::Send(e.to_string()))?;
    let sent = socket.send_to(&magic_packet(&mac), to).map_err(|e| WolError::Send(e.to_string()))?;
    if sent != 102 {
        return Err(WolError::Send("the packet was cut short".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_in_the_usual_spellings() {
        let want = [0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E];
        for s in ["00:1A:2B:3C:4D:5E", "00-1a-2b-3c-4d-5e", "001A.2B3C.4D5E", "001a2b3c4d5e", "  00:1A:2B:3C:4D:5E  "] {
            assert_eq!(parse_mac(s), Ok(want), "{s}");
        }
    }

    #[test]
    fn things_that_are_not_macs() {
        for s in ["", "00:1A:2B:3C:4D", "00:1A:2B:3C:4D:5E:6F", "00:1A:2B:3C:4D:5G", "0:0:0:0:0:0:0:0:0:0:0:0", "001A:2B3C:4D5E", "00 1A 2B 3C 4D 5E", "ÿÿ:ÿÿ:ÿÿ:ÿÿ:ÿÿ:ÿÿ"] {
            assert_eq!(parse_mac(s), Err(WolError::BadMac), "{s:?}");
        }
    }

    #[test]
    fn the_packet_is_six_ff_then_sixteen_macs() {
        let p = magic_packet(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert!(p[6..].chunks(6).all(|c| c == [1, 2, 3, 4, 5, 6]));
    }

    #[test]
    fn where_it_goes() {
        assert_eq!(destination("").unwrap().to_string(), "255.255.255.255:9");
        assert_eq!(destination("192.168.1.255").unwrap().to_string(), "192.168.1.255:9");
        assert_eq!(destination("192.168.1.255:7").unwrap().to_string(), "192.168.1.255:7");
        assert!(matches!(destination("not an address"), Err(WolError::BadAddress(_))));
    }

    #[test]
    fn a_packet_arrives_whole() {
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        listener.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        wake("00:1A:2B:3C:4D:5E", listener.local_addr().unwrap()).unwrap();
        let mut buf = [0u8; 256];
        let n = listener.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], &magic_packet(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E])[..]);
        assert_eq!(wake("nope", listener.local_addr().unwrap()), Err(WolError::BadMac));
    }
}
