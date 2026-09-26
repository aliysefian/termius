//! Local terminal tabs: the user's own shell in a pseudo-terminal (ConPTY on
//! Windows), streamed through the same [`TermSink`] as SSH sessions so logs,
//! status dots and the UI treat both alike.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::ssh::{SessionStatus, TermSink};

#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("could not start a local shell: {0}")]
    Spawn(String),
    #[error("no local terminal for this pane")]
    NotRunning,
    #[error("local terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

struct LocalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub struct LocalManager {
    sessions: Mutex<HashMap<String, Arc<Mutex<LocalSession>>>>,
}

fn size(cols: u32, rows: u32) -> PtySize {
    PtySize {
        rows: rows.clamp(1, u16::MAX as u32) as u16,
        cols: cols.clamp(2, u16::MAX as u32) as u16,
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl LocalManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the user's default shell (`$SHELL`, or the Windows default) in
    /// their home directory. `command` runs a specific program instead,
    /// which the tests use.
    pub fn spawn(
        self: &Arc<Self>,
        pane_id: String,
        cols: u32,
        rows: u32,
        command: Option<Vec<String>>,
        cwd: Option<std::path::PathBuf>,
        sink: Arc<dyn TermSink>,
    ) -> Result<(), LocalError> {
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(|e| LocalError::Spawn(e.to_string()))?;
        let mut cmd = match &command {
            Some(argv) if !argv.is_empty() => {
                let mut c = CommandBuilder::new(&argv[0]);
                c.args(&argv[1..]);
                c
            }
            _ => CommandBuilder::new_default_prog(),
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let start = cwd
            .filter(|d| d.is_dir())
            .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(Into::into));
        if let Some(dir) = start {
            cmd.cwd(dir);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| LocalError::Spawn(e.to_string()))?;
        // The child holds its own copy of the slave; ours would keep the PTY
        // open after the shell exits and the reader would never see EOF.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| LocalError::Spawn(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| LocalError::Spawn(e.to_string()))?;
        let killer = child.clone_killer();

        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                pane_id.clone(),
                Arc::new(Mutex::new(LocalSession {
                    master: pair.master,
                    writer,
                    killer,
                })),
            );
        sink.status(SessionStatus::Connected);

        // PTY reads block, so they get a plain thread rather than a task.
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("pty-{pane_id}"))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => sink.data(&buf[..n]),
                    }
                }
                let code = child.wait().ok().map(|s| s.exit_code());
                me.sessions
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&pane_id);
                sink.status(SessionStatus::Disconnected { code });
            })
            .map_err(LocalError::Io)?;
        Ok(())
    }

    fn get(&self, pane_id: &str) -> Result<Arc<Mutex<LocalSession>>, LocalError> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(pane_id)
            .cloned()
            .ok_or(LocalError::NotRunning)
    }

    pub fn write(&self, pane_id: &str, data: &[u8]) -> Result<(), LocalError> {
        let s = self.get(pane_id)?;
        let mut s = s.lock().unwrap_or_else(|p| p.into_inner());
        s.writer.write_all(data)?;
        s.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, pane_id: &str, cols: u32, rows: u32) -> Result<(), LocalError> {
        let s = self.get(pane_id)?;
        let s = s.lock().unwrap_or_else(|p| p.into_inner());
        s.master
            .resize(size(cols, rows))
            .map_err(|e| LocalError::Io(std::io::Error::other(e.to_string())))
    }

    /// Kill the shell. Not an error if it already exited.
    pub fn close(&self, pane_id: &str) {
        let session = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(pane_id);
        if let Some(s) = session {
            let _ = s.lock().unwrap_or_else(|p| p.into_inner()).killer.kill();
        }
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.close(&id);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[derive(Debug)]
    enum Ev {
        Data(Vec<u8>),
        Status(SessionStatus),
    }
    struct Sink(mpsc::Sender<Ev>);
    impl TermSink for Sink {
        fn data(&self, b: &[u8]) {
            let _ = self.0.send(Ev::Data(b.to_vec()));
        }
        fn status(&self, s: SessionStatus) {
            let _ = self.0.send(Ev::Status(s));
        }
    }

    fn collect_until(rx: &mpsc::Receiver<Ev>, needle: &str) -> String {
        let mut out = Vec::new();
        loop {
            match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("pty output")
            {
                Ev::Data(d) => {
                    out.extend(d);
                    let s = String::from_utf8_lossy(&out).into_owned();
                    if s.contains(needle) {
                        return s;
                    }
                }
                Ev::Status(_) => {}
            }
        }
    }

    #[test]
    fn interactive_shell_input_resize_and_exit_code() {
        let m = Arc::new(LocalManager::new());
        let (tx, rx) = mpsc::channel();
        m.spawn(
            "p".into(),
            80,
            24,
            Some(vec!["sh".into()]),
            None,
            Arc::new(Sink(tx)),
        )
        .unwrap();

        m.write("p", b"echo L$((2+3))X\n").unwrap();
        collect_until(&rx, "L5X");

        m.resize("p", 132, 40).unwrap();
        m.write("p", b"stty size\n").unwrap();
        collect_until(&rx, "40 132");

        m.write("p", b"exit 7\n").unwrap();
        let code = loop {
            if let Ev::Status(SessionStatus::Disconnected { code }) =
                rx.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                break code;
            }
        };
        assert_eq!(code, Some(7));
        assert!(matches!(m.write("p", b"x"), Err(LocalError::NotRunning)));
    }

    #[test]
    fn close_kills_the_shell() {
        let m = Arc::new(LocalManager::new());
        let (tx, rx) = mpsc::channel();
        m.spawn(
            "k".into(),
            80,
            24,
            Some(vec!["sh".into()]),
            None,
            Arc::new(Sink(tx)),
        )
        .unwrap();
        m.close("k");
        let done = (0..50).any(|_| {
            matches!(
                rx.recv_timeout(Duration::from_millis(200)),
                Ok(Ev::Status(SessionStatus::Disconnected { .. }))
            )
        });
        assert!(done, "shell should exit after close");
    }
}
