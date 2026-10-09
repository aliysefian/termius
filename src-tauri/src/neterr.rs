//! Telling network failures apart, so the message says what happened instead of "could not resolve or reach".

use std::io::{Error, ErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The name didn't resolve.
    Dns,
    /// The machine answered that nothing is listening, or a firewall rejected the connection.
    Refused,
    /// No answer in time.
    Timeout,
    /// No route to the network or machine.
    Unreachable,
    Other,
}

impl Kind {
    /// The short code the window can branch on.
    pub fn code(self) -> &'static str {
        match self {
            Kind::Dns => "dns",
            Kind::Refused => "refused",
            Kind::Timeout => "timeout",
            Kind::Unreachable | Kind::Other => "unreachable",
        }
    }
}

pub fn kind(e: &Error) -> Kind {
    match e.kind() {
        ErrorKind::ConnectionRefused => return Kind::Refused,
        ErrorKind::TimedOut | ErrorKind::WouldBlock => return Kind::Timeout,
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable | ErrorKind::NetworkDown => return Kind::Unreachable,
        _ => {}
    }
    // A failed lookup has no dedicated error kind, so it is recognised by what the operating system says.
    let text = e.to_string().to_lowercase();
    const LOOKUP: [&str; 6] = [
        "failed to lookup address",
        "name or service not known",
        "temporary failure in name resolution",
        "no such host",
        "nodename nor servname",
        "no address associated",
    ];
    if LOOKUP.iter().any(|w| text.contains(w)) {
        Kind::Dns
    } else {
        Kind::Other
    }
}

/// One sentence for a failed connection to `addr` (`host:port`).
pub fn connect_text(addr: &str, e: &Error) -> String {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr).trim_matches(['[', ']']);
    match kind(e) {
        Kind::Dns => format!("could not find {host}: the name did not resolve ({e})"),
        Kind::Refused => format!("{addr} refused the connection: nothing is listening on that port, or a firewall rejected it ({e})"),
        Kind::Timeout => format!("{addr} did not answer in time ({e})"),
        Kind::Unreachable => format!("there is no route to {addr} ({e})"),
        Kind::Other => format!("could not connect to {addr}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(kind: ErrorKind, msg: &str) -> Error {
        Error::new(kind, msg.to_string())
    }

    #[test]
    fn each_failure_is_told_apart() {
        assert_eq!(kind(&err(ErrorKind::ConnectionRefused, "x")), Kind::Refused);
        assert_eq!(kind(&err(ErrorKind::TimedOut, "x")), Kind::Timeout);
        assert_eq!(kind(&err(ErrorKind::HostUnreachable, "x")), Kind::Unreachable);
        for m in [
            "failed to lookup address information: Name or service not known",
            "No such host is known. (os error 11001)",
            "nodename nor servname provided, or not known",
            "Temporary failure in name resolution",
        ] {
            assert_eq!(kind(&err(ErrorKind::Other, m)), Kind::Dns, "{m}");
        }
        assert_eq!(kind(&err(ErrorKind::Other, "something else")), Kind::Other);
    }

    #[test]
    fn codes_and_sentences_name_the_cause() {
        assert_eq!(Kind::Dns.code(), "dns");
        assert_eq!(Kind::Refused.code(), "refused");
        assert_eq!(Kind::Timeout.code(), "timeout");
        assert_eq!(Kind::Other.code(), "unreachable");
        let t = connect_text("db.example.com:22", &err(ErrorKind::Other, "failed to lookup address information: Name or service not known"));
        assert!(t.starts_with("could not find db.example.com: the name did not resolve"), "{t}");
        let t = connect_text("[::1]:22", &err(ErrorKind::ConnectionRefused, "Connection refused (os error 111)"));
        assert!(t.starts_with("[::1]:22 refused the connection"), "{t}");
        let t = connect_text("[fe80::1]:22", &err(ErrorKind::Other, "failed to lookup address information"));
        assert!(t.contains("could not find fe80::1:"), "{t}");
    }
}
