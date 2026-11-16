//! PTY d'alacritty dont la lecture passe par [`ApcFilter`] : les commandes
//! graphiques sont exécutées avant que le parseur ne voie le reste du flux.
//! Le [`ProgressScanner`] y repère aussi la barre de progression.

use std::io::{self, Read};
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoopSender, Msg};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite, Pty};
use polling::{Event, PollMode, Poller};

use crate::graphics::{ApcFilter, Graphics};
use crate::progress::ProgressScanner;
use crate::terminal::TerminalEvent;

pub(crate) struct GraphicsPty {
    pty: Pty,
    reader: FilteredReader,
}

impl GraphicsPty {
    pub fn new(
        pty: Pty,
        graphics: Arc<Mutex<Graphics>>,
        replies: Arc<OnceLock<EventLoopSender>>,
        on_event: Arc<dyn Fn(TerminalEvent) + Send + Sync>,
    ) -> Self {
        let reader = FilteredReader {
            filter: ApcFilter::default(),
            graphics,
            replies,
            progress: ProgressScanner::default(),
            on_event,
            raw: vec![0; 0x1_0000].into_boxed_slice(),
            pending: Vec::new(),
            read_from: 0,
        };
        Self { pty, reader }
    }
}

struct FilteredReader {
    filter: ApcFilter,
    graphics: Arc<Mutex<Graphics>>,
    replies: Arc<OnceLock<EventLoopSender>>,
    progress: ProgressScanner,
    on_event: Arc<dyn Fn(TerminalEvent) + Send + Sync>,
    raw: Box<[u8]>,
    /// Octets filtrés pas encore rendus à l'appelant.
    pending: Vec<u8>,
    read_from: usize,
}

impl FilteredReader {
    fn read(&mut self, source: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.read_from < self.pending.len() {
                let available = &self.pending[self.read_from..];
                let n = available.len().min(buf.len());
                buf[..n].copy_from_slice(&available[..n]);
                self.read_from += n;
                return Ok(n);
            }
            self.pending.clear();
            self.read_from = 0;

            // `WouldBlock` et la fin du flux remontent tels quels à la boucle d'E/S.
            let got = source.read(&mut self.raw)?;
            if got == 0 {
                return Ok(0);
            }
            let Self { filter, graphics, replies, raw, pending, .. } = self;
            filter.feed(&raw[..got], pending, |command| {
                let reply = graphics.lock().unwrap().command(command);
                if let (Some(reply), Some(sender)) = (reply, replies.get()) {
                    let _ = sender.send(Msg::Input(reply.into_bytes().into()));
                }
            });
            if let Some(progress) = self.progress.scan(&self.pending) {
                (self.on_event)(TerminalEvent::Progress(progress));
            }
            // Lecture faite uniquement de commandes : on relit, jusqu'à du texte ou `WouldBlock`.
        }
    }
}

impl Read for GraphicsPty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(self.pty.reader(), buf)
    }
}

impl EventedReadWrite for GraphicsPty {
    type Reader = Self;
    type Writer = <Pty as EventedReadWrite>::Writer;

    unsafe fn register(&mut self, poll: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()> {
        unsafe { self.pty.register(poll, interest, mode) }
    }

    fn reregister(&mut self, poll: &Arc<Poller>, interest: Event, mode: PollMode) -> io::Result<()> {
        self.pty.reregister(poll, interest, mode)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        self.pty.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self::Reader {
        self
    }

    fn writer(&mut self) -> &mut Self::Writer {
        self.pty.writer()
    }
}

impl EventedPty for GraphicsPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.pty.next_child_event()
    }
}

impl OnResize for GraphicsPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.pty.on_resize(window_size);
    }
}
