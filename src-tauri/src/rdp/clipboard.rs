//! Text-only clipboard sharing over the RDP clipboard channel.
//!
//! The window decides when text moves: this side offers the local text when the
//! window sends it, and asks for the remote text when the remote says it copied
//! something. Files and images aren't offered or accepted.

use std::sync::{Arc, Mutex};

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest, FileContentsResponse, FormatDataRequest, FormatDataResponse,
    LockDataId, OwnedFormatDataResponse,
};
use tokio::sync::mpsc::UnboundedSender;

/// The most text taken from the remote, or offered to it.
pub const MAX_TEXT: usize = 1024 * 1024;

/// Text as the Windows clipboard holds it: UTF-16 with CR LF line ends and a NUL.
pub fn encode_text(text: &str) -> Vec<u8> {
    let text: String = text.chars().take(MAX_TEXT).collect();
    let windows = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let mut out: Vec<u8> = windows.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend([0, 0]);
    out
}

/// The text in a `CF_UNICODETEXT` payload, with Unix line ends.
pub fn decode_text(data: &[u8]) -> String {
    let units: Vec<u16> = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|u| *u != 0).take(MAX_TEXT).collect();
    String::from_utf16_lossy(&units).replace("\r\n", "\n")
}

pub struct TextClipboard {
    /// What to offer when the remote asks for it.
    local: Arc<Mutex<Option<String>>>,
    /// Where messages for the channel go; the session loop sends them.
    to_session: UnboundedSender<ClipboardMessage>,
    /// Where text from the remote goes.
    on_remote_text: Box<dyn Fn(String) + Send>,
}

impl std::fmt::Debug for TextClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TextClipboard")
    }
}

ironrdp_core::impl_as_any!(TextClipboard);

impl TextClipboard {
    pub fn new(local: Arc<Mutex<Option<String>>>, to_session: UnboundedSender<ClipboardMessage>, on_remote_text: Box<dyn Fn(String) + Send>) -> Self {
        Self { local, to_session, on_remote_text }
    }
}

impl CliprdrBackend for TextClipboard {
    fn temporary_directory(&self) -> &str {
        ""
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }

    fn on_ready(&mut self) {}

    fn on_request_format_list(&mut self) {
        // Say we have nothing yet; the window offers text when there is some.
        let _ = self.to_session.send(ClipboardMessage::SendInitiateCopy(Vec::new()));
    }

    fn on_process_negotiated_capabilities(&mut self, _capabilities: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        if available_formats.iter().any(|f| f.id == ClipboardFormatId::CF_UNICODETEXT) {
            let _ = self.to_session.send(ClipboardMessage::SendInitiatePaste(ClipboardFormatId::CF_UNICODETEXT));
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        let text = self.local.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let response = match text {
            Some(t) if request.format == ClipboardFormatId::CF_UNICODETEXT => OwnedFormatDataResponse::new_data(encode_text(&t)),
            _ => OwnedFormatDataResponse::new_error(),
        };
        let _ = self.to_session.send(ClipboardMessage::SendFormatData(response));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if !response.is_error() {
            (self.on_remote_text)(decode_text(response.data()));
        }
    }

    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _data_id: LockDataId) {}
    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_through_the_windows_form() {
        for t in ["", "hello", "two\nlines", "tab\tand ünïcode 東京 😀", "already\r\nwindows"] {
            let wire = encode_text(t);
            assert_eq!(&wire[wire.len() - 2..], [0, 0], "NUL terminated");
            assert_eq!(decode_text(&wire), t.replace("\r\n", "\n"), "{t:?}");
        }
    }

    #[test]
    fn line_ends_become_crlf_on_the_way_out_and_lf_on_the_way_in() {
        let wire = encode_text("a\nb");
        let as_text = String::from_utf16_lossy(&wire.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>());
        assert_eq!(as_text, "a\r\nb\0");
        assert_eq!(decode_text(&wire), "a\nb");
    }

    #[test]
    fn decoding_stops_at_the_terminator_and_tolerates_junk() {
        assert_eq!(decode_text(&[b'h', 0, b'i', 0, 0, 0, b'x', 0]), "hi");
        assert_eq!(decode_text(&[b'h', 0, b'i']), "h", "an odd trailing byte is dropped");
        assert_eq!(decode_text(&[]), "");
        // A lone surrogate is replaced, not a panic.
        assert!(decode_text(&[0x00, 0xD8, b'a', 0]).contains('\u{FFFD}'));
    }

    #[test]
    fn text_is_capped() {
        let big = "x".repeat(MAX_TEXT + 500);
        assert_eq!(decode_text(&encode_text(&big)).chars().count(), MAX_TEXT);
    }

    #[test]
    fn the_remote_asking_for_text_gets_it_and_nothing_else() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let local = Arc::new(Mutex::new(Some("secret".to_string())));
        let mut cb = TextClipboard::new(local.clone(), tx, Box::new(|_| {}));
        cb.on_format_data_request(FormatDataRequest { format: ClipboardFormatId::CF_UNICODETEXT });
        match rx.try_recv().unwrap() {
            ClipboardMessage::SendFormatData(r) => assert_eq!(decode_text(r.data()), "secret"),
            other => panic!("{other:?}"),
        }
        // A different format, or no text, is refused.
        cb.on_format_data_request(FormatDataRequest { format: ClipboardFormatId::CF_BITMAP });
        match rx.try_recv().unwrap() {
            ClipboardMessage::SendFormatData(r) => assert!(r.is_error()),
            other => panic!("{other:?}"),
        }
        *local.lock().unwrap() = None;
        cb.on_format_data_request(FormatDataRequest { format: ClipboardFormatId::CF_UNICODETEXT });
        match rx.try_recv().unwrap() {
            ClipboardMessage::SendFormatData(r) => assert!(r.is_error()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remote_copies_are_fetched_only_when_they_include_text() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let mut cb = TextClipboard::new(Arc::default(), tx, Box::new(move |t| g.lock().unwrap().push(t)));
        cb.on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_BITMAP)]);
        assert!(rx.try_recv().is_err(), "an image-only copy isn't fetched");
        cb.on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_BITMAP), ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]);
        assert!(matches!(rx.try_recv().unwrap(), ClipboardMessage::SendInitiatePaste(f) if f == ClipboardFormatId::CF_UNICODETEXT));
        cb.on_format_data_response(FormatDataResponse::new_data(encode_text("from remote")));
        cb.on_format_data_response(FormatDataResponse::new_error());
        assert_eq!(*got.lock().unwrap(), vec!["from remote".to_string()]);
    }
}
