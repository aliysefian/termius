//! Which algorithms a connection offers, and what to say when the server and this app share none.
//!
//! The defaults are the modern ones. Old network gear, printers and appliances often offer nothing newer than SHA-1 key
//! exchange, CBC ciphers and SHA-1 MACs, and connecting to them fails with a bare "no common algorithm". A host can opt in,
//! per host, to also offering those older ones (after the modern ones, so a server that has both still gets the modern
//! one). Each is weaker, which is why it is a deliberate choice for one host and never a global setting.

use std::borrow::Cow;

use russh::{cipher, kex, mac, AlgorithmKind, Preferred};

/// The algorithms to offer: the defaults, plus the older ones when `legacy` is set.
pub fn preferred(legacy: bool) -> Preferred {
    let mut p = Preferred::DEFAULT;
    if legacy {
        p.kex = Cow::Owned([p.kex.as_ref(), &[kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1]].concat());
        p.cipher = Cow::Owned([p.cipher.as_ref(), &[cipher::AES_256_CBC, cipher::AES_192_CBC, cipher::AES_128_CBC]].concat());
        p.mac = Cow::Owned([p.mac.as_ref(), &[mac::HMAC_SHA1_ETM, mac::HMAC_SHA1]].concat());
    }
    p
}

fn kind_word(kind: &AlgorithmKind) -> &'static str {
    match kind {
        AlgorithmKind::Kex => "key exchange",
        AlgorithmKind::Key => "host key",
        AlgorithmKind::Cipher => "cipher",
        AlgorithmKind::Compression => "compression",
        AlgorithmKind::Mac => "message authentication",
    }
}

/// A message a person can act on for a failed algorithm negotiation; anything else keeps the library's own words.
pub fn explain(e: &russh::Error) -> String {
    match e {
        russh::Error::NoCommonAlgo { kind, ours, theirs } => {
            let older = matches!(kind, AlgorithmKind::Kex | AlgorithmKind::Cipher | AlgorithmKind::Mac);
            let list = |v: &[String]| if v.is_empty() { "nothing".to_string() } else { v.join(", ") };
            format!(
                "The server and this app have no {} method in common. The server offers: {}. This app offers: {}.{}",
                kind_word(kind),
                list(theirs),
                list(ours),
                if older { " If this is an old device, turn on \"Allow older algorithms\" in this host's settings (weaker, so only for a host you trust)." } else { "" }
            )
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_untouched_and_the_older_ones_come_after_the_modern_ones() {
        let modern = preferred(false);
        assert_eq!(modern.kex.as_ref(), Preferred::DEFAULT.kex.as_ref());
        assert!(!modern.kex.contains(&kex::DH_G1_SHA1) && !modern.cipher.contains(&cipher::AES_128_CBC) && !modern.mac.contains(&mac::HMAC_SHA1));

        let old = preferred(true);
        for (modern_list, old_list) in [(modern.kex.len(), old.kex.len()), (modern.cipher.len(), old.cipher.len()), (modern.mac.len(), old.mac.len())] {
            assert!(old_list > modern_list);
        }
        assert_eq!(&old.kex[..modern.kex.len()], modern.kex.as_ref(), "modern ones keep their place first");
        assert_eq!(&old.cipher[..modern.cipher.len()], modern.cipher.as_ref());
        assert_eq!(&old.mac[..modern.mac.len()], modern.mac.as_ref());
        assert!(old.kex.contains(&kex::DH_G14_SHA1) && old.cipher.contains(&cipher::AES_128_CBC) && old.mac.contains(&mac::HMAC_SHA1));
    }

    #[test]
    fn a_failed_negotiation_says_what_each_side_offered_and_what_to_do() {
        let e = russh::Error::NoCommonAlgo { kind: AlgorithmKind::Kex, ours: vec!["curve25519-sha256".into()], theirs: vec!["diffie-hellman-group1-sha1".into()] };
        let text = explain(&e);
        assert!(text.contains("no key exchange method in common"), "{text}");
        assert!(text.contains("server offers: diffie-hellman-group1-sha1") && text.contains("app offers: curve25519-sha256"), "{text}");
        assert!(text.contains("Allow older algorithms"), "{text}");
        // A host-key mismatch is not fixed by the older list, so it is not suggested.
        let key = russh::Error::NoCommonAlgo { kind: AlgorithmKind::Key, ours: vec!["ssh-ed25519".into()], theirs: vec![] };
        let text = explain(&key);
        assert!(text.contains("host key") && text.contains("server offers: nothing") && !text.contains("older"), "{text}");
        // Everything else keeps the library's words.
        assert_eq!(explain(&russh::Error::Kex), russh::Error::Kex.to_string());
    }
}
