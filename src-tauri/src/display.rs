//! What a remote display (RDP, VNC) reports to the window and takes from it: pictures, the pointer, clipboard
//! text and the end of the session out; keys, the mouse and clipboard text in.

#[derive(Debug, Clone)]
pub enum Cursor {
    Default,
    Hidden,
    Bitmap { hot_x: u16, hot_y: u16, width: u16, height: u16, rgba: Vec<u8> },
}

pub trait DisplaySink: Send + Sync + 'static {
    fn size(&self, width: u16, height: u16);
    /// A changed rectangle, as RGBA rows.
    fn frame(&self, x: u16, y: u16, width: u16, height: u16, rgba: &[u8]);
    fn cursor(&self, cursor: Cursor);
    fn clipboard(&self, text: String);
    /// The session is over; `error` says why when it wasn't the person's doing.
    fn ended(&self, error: Option<String>);
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    Key { code: String, down: bool },
    MouseMove { x: u16, y: u16 },
    Button { button: u8, down: bool },
    Wheel { vertical: bool, units: i16 },
    /// Ctrl+Alt+Del, which the window can't send as keys because the computer would act on it first.
    CtrlAltDel,
    /// Let go of every key and button (the window lost focus).
    ReleaseAll,
    /// Offer this text to the remote's clipboard.
    ClipboardText { text: String },
    Close,
}

