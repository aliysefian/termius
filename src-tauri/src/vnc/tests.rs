use super::*;
use crate::display::{Cursor, DisplaySink, Input};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn des_matches_the_textbook_vector() {
    // The classic worked example: key 133457799BBCDFF1, text 0123456789ABCDEF.
    let key: [u8; 8] = hex("133457799BBCDFF1").try_into().unwrap();
    assert_eq!(des_block(&key, &hex("0123456789ABCDEF")).to_vec(), hex("85E813540F0AB405"));
}

#[test]
fn a_vnc_key_reverses_the_bits_of_each_byte_and_pads_with_zeros() {
    assert_eq!(auth_key("a"), [0x86, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(auth_key("abcdefghij"), [0x86, 0x46, 0xc6, 0x26, 0xa6, 0x66, 0xe6, 0x16], "only eight bytes count");
    assert_eq!(auth_key(""), [0; 8]);
}

#[test]
fn the_response_is_two_blocks_under_the_vnc_key() {
    let challenge: [u8; 16] = *b"0123456789abcdef";
    let r = challenge_response("secret", &challenge);
    let key = auth_key("secret");
    assert_eq!(r[..8], des_block(&key, &challenge[..8]));
    assert_eq!(r[8..], des_block(&key, &challenge[8..]));
    assert_ne!(r, challenge_response("Secret", &challenge));
}

/// A server for the handshake: `script` plays the server's side.
async fn serve<F, Fut>(script: F) -> DuplexStream
where
    F: FnOnce(DuplexStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (client, server) = duplex(1 << 20);
    tokio::spawn(script(server));
    client
}

async fn server_init(s: &mut DuplexStream, w: u16, h: u16) {
    let mut m = Vec::new();
    m.extend_from_slice(&w.to_be_bytes());
    m.extend_from_slice(&h.to_be_bytes());
    m.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
    m.extend_from_slice(&4u32.to_be_bytes());
    m.extend_from_slice(b"test");
    s.write_all(&m).await.unwrap();
}

async fn read_n(s: &mut DuplexStream, n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    s.read_exact(&mut b).await.unwrap();
    b
}

#[tokio::test]
async fn signs_in_with_a_password_on_3_8() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.008\n").await.unwrap();
        assert_eq!(read_n(&mut s, 12).await, b"RFB 003.008\n");
        s.write_all(&[2, 1, 2]).await.unwrap(); // none and VNC authentication
        assert_eq!(read_n(&mut s, 1).await, [2], "with a password, the stronger one is chosen");
        let challenge = [7u8; 16];
        s.write_all(&challenge).await.unwrap();
        let got = read_n(&mut s, 16).await;
        let ok = got == challenge_response("hunter2", &challenge);
        s.write_all(&(if ok { 0u32 } else { 1u32 }).to_be_bytes()).await.unwrap();
        if !ok {
            s.write_all(&21u32.to_be_bytes()).await.unwrap();
            s.write_all(b"Authentication failed").await.unwrap();
            return;
        }
        assert_eq!(read_n(&mut s, 1).await, [1], "ClientInit shares the desktop");
        server_init(&mut s, 640, 480).await;
    })
    .await;
    let info = handshake(&mut c, Some("hunter2")).await.unwrap();
    assert_eq!(info, ServerInfo { width: 640, height: 480, name: "test".into() });
}

#[tokio::test]
async fn a_wrong_password_is_called_that() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut s, 12).await;
        s.write_all(&[1, 2]).await.unwrap();
        read_n(&mut s, 1).await;
        s.write_all(&[7u8; 16]).await.unwrap();
        read_n(&mut s, 16).await;
        s.write_all(&1u32.to_be_bytes()).await.unwrap();
        s.write_all(&21u32.to_be_bytes()).await.unwrap();
        s.write_all(b"Authentication failed").await.unwrap();
    })
    .await;
    assert!(matches!(handshake(&mut c, Some("nope")).await, Err(VncError::BadPassword)));
}

#[tokio::test]
async fn a_server_that_needs_a_password_says_so_before_anything_is_sent_for_it() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut s, 12).await;
        s.write_all(&[1, 2]).await.unwrap();
    })
    .await;
    assert!(matches!(handshake(&mut c, None).await, Err(VncError::PasswordRequired)));
}

#[tokio::test]
async fn no_authentication_needs_no_password_and_3_7_has_no_result_for_it() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.007\n").await.unwrap();
        assert_eq!(read_n(&mut s, 12).await, b"RFB 003.007\n");
        s.write_all(&[1, 1]).await.unwrap();
        assert_eq!(read_n(&mut s, 1).await, [1]);
        // 3.7 with no authentication goes straight to ClientInit.
        assert_eq!(read_n(&mut s, 1).await, [1]);
        server_init(&mut s, 100, 50).await;
    })
    .await;
    assert_eq!(handshake(&mut c, None).await.unwrap().width, 100);
}

#[tokio::test]
async fn the_old_3_3_way_works_and_3_5_is_spoken_as_3_3() {
    for greeting in [&b"RFB 003.003\n"[..], &b"RFB 003.005\n"[..]] {
        let g = greeting.to_vec();
        let mut c = serve(move |mut s| async move {
            s.write_all(&g).await.unwrap();
            assert_eq!(read_n(&mut s, 12).await, b"RFB 003.003\n");
            s.write_all(&1u32.to_be_bytes()).await.unwrap(); // the server decides: none
            assert_eq!(read_n(&mut s, 1).await, [1]);
            server_init(&mut s, 10, 10).await;
        })
        .await;
        assert_eq!(handshake(&mut c, None).await.unwrap().height, 10);
    }
}

#[tokio::test]
async fn a_refusal_carries_the_servers_reason() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut s, 12).await;
        s.write_all(&[0]).await.unwrap();
        s.write_all(&13u32.to_be_bytes()).await.unwrap();
        s.write_all(b"Too many tries").await.unwrap();
    })
    .await;
    match handshake(&mut c, None).await {
        Err(VncError::Refused(why)) => assert!(why.starts_with("Too many")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn something_that_is_not_vnc_is_not_trusted() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"SSH-2.0-Open").await.unwrap();
    })
    .await;
    assert!(matches!(handshake(&mut c, None).await, Err(VncError::Protocol(_))));
}

#[tokio::test]
async fn offers_nothing_this_app_speaks() {
    let mut c = serve(|mut s| async move {
        s.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut s, 12).await;
        s.write_all(&[1, 19]).await.unwrap(); // VeNCrypt only
    })
    .await;
    assert!(matches!(handshake(&mut c, Some("x")).await, Err(VncError::Protocol(m)) if m.contains("VeNCrypt")));
}

#[test]
fn input_becomes_messages() {
    let mut s = InputState::default();
    assert_eq!(s.encode(&Input::Key { code: "KeyA".into(), down: true }), [4, 1, 0, 0, 0, 0, 0, 0x61]);
    assert_eq!(s.encode(&Input::Key { code: "KeyA".into(), down: false }), [4, 0, 0, 0, 0, 0, 0, 0x61]);
    assert!(s.encode(&Input::Key { code: "Nope".into(), down: true }).is_empty());
    assert_eq!(s.encode(&Input::MouseMove { x: 258, y: 3 }), [5, 0, 1, 2, 0, 3]);
    assert_eq!(s.encode(&Input::Button { button: 0, down: true }), [5, 1, 1, 2, 0, 3]);
    assert_eq!(s.encode(&Input::Button { button: 2, down: true }), [5, 5, 1, 2, 0, 3], "left and right together");
    assert_eq!(s.encode(&Input::Button { button: 0, down: false }), [5, 4, 1, 2, 0, 3]);
    assert!(s.encode(&Input::Button { button: 9, down: true }).is_empty());
    // One notch up: wheel-up pressed and released, keeping what was held.
    assert_eq!(s.encode(&Input::Wheel { vertical: true, units: 120 }), [5, 4 | 8, 1, 2, 0, 3, 5, 4, 1, 2, 0, 3]);
    assert_eq!(s.encode(&Input::Wheel { vertical: true, units: -1 })[1], 4 | 16, "a tiny scroll down is still a notch");
    assert_eq!(s.encode(&Input::Wheel { vertical: false, units: 240 }).len(), 2 * 6 * 2, "two notches right");
    // Never a runaway of notches.
    assert_eq!(s.encode(&Input::Wheel { vertical: true, units: i16::MAX }).len(), 10 * 12);
}

#[test]
fn letting_go_releases_every_key_and_button() {
    let mut s = InputState::default();
    s.encode(&Input::Key { code: "ShiftLeft".into(), down: true });
    s.encode(&Input::Key { code: "KeyA".into(), down: true });
    s.encode(&Input::Button { button: 0, down: true });
    let out = s.encode(&Input::ReleaseAll);
    assert_eq!(out.len(), 8 + 8 + 6);
    assert!(s.encode(&Input::ReleaseAll).is_empty(), "nothing left to release");
}

#[test]
fn ctrl_alt_del_goes_down_then_up_in_reverse() {
    let out = InputState::default().encode(&Input::CtrlAltDel);
    assert_eq!(out.len(), 6 * 8);
    assert_eq!(&out[..8], &key_event(true, keys::CONTROL_LEFT)[..]);
    assert_eq!(&out[16..24], &key_event(true, keys::DELETE)[..]);
    assert_eq!(&out[24..32], &key_event(false, keys::DELETE)[..]);
    assert_eq!(&out[40..], &key_event(false, keys::CONTROL_LEFT)[..]);
}

#[test]
fn clipboard_text_is_latin_1() {
    let out = InputState::default().encode(&Input::ClipboardText { text: "aé✓".into() });
    assert_eq!(out, [6, 0, 0, 0, 0, 0, 0, 3, b'a', 0xe9, b'?']);
}

// -- a whole session --------------------------------------------------------------------

#[derive(Default)]
struct Collect(Mutex<Vec<String>>, Mutex<Vec<Vec<u8>>>);

impl DisplaySink for Collect {
    fn size(&self, w: u16, h: u16) {
        self.0.lock().unwrap().push(format!("size {w}x{h}"));
    }
    fn frame(&self, x: u16, y: u16, w: u16, h: u16, rgba: &[u8]) {
        self.0.lock().unwrap().push(format!("frame {x},{y} {w}x{h}"));
        self.1.lock().unwrap().push(rgba.to_vec());
    }
    fn cursor(&self, c: Cursor) {
        self.0.lock().unwrap().push(match c {
            Cursor::Default => "cursor default".into(),
            Cursor::Hidden => "cursor hidden".into(),
            Cursor::Bitmap { hot_x, hot_y, width, height, rgba } => format!("cursor {hot_x},{hot_y} {width}x{height} {:?}", &rgba[..4]),
        });
    }
    fn clipboard(&self, t: String) {
        self.0.lock().unwrap().push(format!("clip {t}"));
    }
    fn ended(&self, e: Option<String>) {
        self.0.lock().unwrap().push(format!("ended {}", e.unwrap_or_default()));
    }
}

fn rect(x: u16, y: u16, w: u16, h: u16, enc: i32) -> Vec<u8> {
    let mut m = Vec::new();
    for n in [x, y, w, h] {
        m.extend_from_slice(&n.to_be_bytes());
    }
    m.extend_from_slice(&enc.to_be_bytes());
    m
}

fn update(rects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut m = vec![0u8, 0];
    m.extend_from_slice(&(rects.len() as u16).to_be_bytes());
    for r in rects {
        m.extend(r);
    }
    m
}

async fn wait_for(c: &Collect, what: &str) {
    for _ in 0..200 {
        if c.0.lock().unwrap().iter().any(|e| e.starts_with(what)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("never saw {what:?}; saw {:?}", c.0.lock().unwrap());
}

#[tokio::test]
async fn a_session_draws_what_the_server_sends_and_sends_what_is_typed() {
    let (client, mut server) = duplex(1 << 22);
    let sink = Arc::new(Collect::default());
    let (told_tx, told_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    tokio::spawn(async move {
        server.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut server, 12).await;
        server.write_all(&[1, 1]).await.unwrap();
        read_n(&mut server, 1).await;
        server.write_all(&0u32.to_be_bytes()).await.unwrap();
        read_n(&mut server, 1).await;
        server_init(&mut server, 4, 2).await;
        // The client sets its format and encodings, and asks for the whole picture.
        let setup = read_n(&mut server, 20 + 4 + 16 + 10).await;
        assert_eq!(setup[0], 0, "SetPixelFormat first");
        assert_eq!(setup[20], 2, "then SetEncodings");
        assert_eq!(setup[24 + 16], 3, "then a full update request");
        assert_eq!(setup[24 + 16 + 1], 0, "which is not incremental");

        // One raw rectangle: the first pixel blue (B, G, R, X), the rest red.
        let mut raw = rect(0, 0, 2, 2, 0);
        raw.extend_from_slice(&[255, 0, 0, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0]);
        server.write_all(&update(vec![raw])).await.unwrap();
        assert_eq!(read_n(&mut server, 10).await[1], 1, "then an incremental request");
        // Copy it right by two, and set a cursor with a mask that hides the second pixel.
        let mut copy = rect(2, 0, 2, 2, 1);
        copy.extend_from_slice(&[0, 0, 0, 0]);
        let mut cursor = rect(1, 1, 2, 1, -239);
        cursor.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]); // two pixels
        cursor.push(0b1000_0000); // only the first is solid
        server.write_all(&update(vec![copy, cursor])).await.unwrap();
        read_n(&mut server, 10).await;
        // Text for the clipboard, in Latin-1.
        server.write_all(&[3, 0, 0, 0, 0, 0, 0, 3, b'h', 0xe9, b'!']).await.unwrap();
        // A resize.
        server.write_all(&update(vec![rect(0, 0, 8, 6, -223)])).await.unwrap();
        assert_eq!(read_n(&mut server, 10).await[1], 0, "a resize asks for the whole picture");
        // What the client typed, up to the close.
        let mut got = Vec::new();
        let _ = server.read_to_end(&mut got).await;
        let _ = told_tx.send(got);
    });

    let manager = Arc::new(VncManager::new());
    let info = manager.connect("pane".into(), client, None, sink.clone(), None).await.unwrap();
    assert_eq!((info.width, info.height), (4, 2));
    wait_for(&sink, "clip").await;
    wait_for(&sink, "size 8x6").await;
    let log = sink.0.lock().unwrap().clone();
    assert_eq!(log[0], "size 4x2");
    assert_eq!(log[1], "frame 0,0 2x2");
    assert_eq!(sink.1.lock().unwrap()[0][..8], [0, 0, 255, 255, 255, 0, 0, 255], "B,G,R,X becomes R,G,B,A");
    assert_eq!(log[2], "frame 2,0 2x2", "a copy is drawn where it lands");
    let frames = sink.1.lock().unwrap().clone();
    assert_eq!(frames[1], frames[0], "and holds what was copied");
    assert_eq!(log[3], "cursor 1,1 2x1 [255, 0, 0, 255]", "the cursor's own hot spot, solid where the mask says");
    assert_eq!(log[4], "clip hé!");
    assert_eq!(log[5], "size 8x6");

    manager.send("pane", Input::Key { code: "KeyA".into(), down: true }).unwrap();
    manager.send("pane", Input::MouseMove { x: 1, y: 2 }).unwrap();
    manager.close("pane");
    let typed = told_rx.await.unwrap();
    assert_eq!(typed, [4, 1, 0, 0, 0, 0, 0, 0x61, 5, 0, 0, 1, 0, 2]);
    wait_for(&sink, "ended").await;
    assert!(matches!(manager.send("pane", Input::ReleaseAll), Err(VncError::NoSession)));
}

#[tokio::test]
async fn a_picture_outside_the_screen_or_an_unknown_encoding_ends_the_session_with_a_reason() {
    for (bad, expect) in [(rect(3, 0, 2, 1, 0), "outside"), (rect(0, 0, 1, 1, 16), "encoding")] {
        let (client, mut server) = duplex(1 << 20);
        let sink = Arc::new(Collect::default());
        tokio::spawn(async move {
            server.write_all(b"RFB 003.008\n").await.unwrap();
            read_n(&mut server, 12).await;
            server.write_all(&[1, 1]).await.unwrap();
            read_n(&mut server, 1).await;
            server.write_all(&0u32.to_be_bytes()).await.unwrap();
            read_n(&mut server, 1).await;
            server_init(&mut server, 4, 2).await;
            read_n(&mut server, 50).await;
            server.write_all(&update(vec![bad])).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        });
        let manager = Arc::new(VncManager::new());
        manager.connect("p".into(), client, None, sink.clone(), None).await.unwrap();
        wait_for(&sink, "ended ").await;
        let last = sink.0.lock().unwrap().last().unwrap().clone();
        assert!(last.contains(expect), "{last}");
    }
}

#[tokio::test]
async fn the_server_closing_is_a_quiet_end() {
    let (client, mut server) = duplex(1 << 20);
    let sink = Arc::new(Collect::default());
    tokio::spawn(async move {
        server.write_all(b"RFB 003.008\n").await.unwrap();
        read_n(&mut server, 12).await;
        server.write_all(&[1, 1]).await.unwrap();
        read_n(&mut server, 1).await;
        server.write_all(&0u32.to_be_bytes()).await.unwrap();
        read_n(&mut server, 1).await;
        server_init(&mut server, 4, 2).await;
        read_n(&mut server, 50).await;
        // dropped: the connection closes
    });
    let manager = Arc::new(VncManager::new());
    manager.connect("p".into(), client, None, sink.clone(), None).await.unwrap();
    wait_for(&sink, "ended").await;
    assert_eq!(sink.0.lock().unwrap().last().unwrap(), "ended ");
}
