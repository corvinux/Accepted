use crate::buffer::Buffer;
use crate::draw;
use crate::mode::{Mode, Normal, Transition};

pub struct BufferMode<'a> {
    buf: Buffer<'a>,
    mode: Box<Mode>,
    is_recording: bool,
    dot_macro: Vec<termion::event::Event>,
    recording_macro: Vec<termion::event::Event>,
}

impl<'a> BufferMode<'a> {
    pub fn new(buf: Buffer<'a>) -> Self {
        Self {
            buf,
            mode: Box::new(Normal::default()),
            is_recording: false,
            dot_macro: Vec::new(),
            recording_macro: Vec::new(),
        }
    }

    pub fn buf(&self) -> &Buffer {
        &self.buf
    }

    pub fn event(&mut self, event: termion::event::Event) -> bool {
        if self.is_recording {
            self.recording_macro.push(event.clone());
        }
        match self.mode.event(&mut self.buf, event.clone()) {
            Transition::Exit => {
                return true;
            }
            Transition::Trans(mut t) => {
                t.init(&mut self.buf);
                self.mode = t;
            }
            Transition::DoMacro => {
                for event in self.dot_macro.clone() {
                    self.event(event);
                }
            }
            Transition::Return(s, is_commit_macro) => {
                if self.is_recording && !self.recording_macro.is_empty() && is_commit_macro {
                    std::mem::swap(&mut self.dot_macro, &mut self.recording_macro);
                    self.recording_macro.clear();
                }
                self.is_recording = false;
                let mut t = if let Some(s) = s {
                    Box::new(Normal::with_message(s))
                } else {
                    Box::new(Normal::default())
                };
                t.init(&mut self.buf);
                self.mode = t;
            }
            Transition::RecordMacro(mut t) => {
                self.is_recording = true;
                self.recording_macro.clear();
                self.recording_macro.push(event);
                t.init(&mut self.buf);
                self.mode = t;
            }
            Transition::Nothing => {}
        }
        false
    }

    pub fn draw(&mut self, term: &mut draw::Term) {
        term.cursor = self
            .mode
            .draw(&mut self.buf, term.view((0, 0), term.height, term.width));
    }

    /// This method should be called every frame
    pub fn background_task_duration(&mut self, duration: std::time::Duration) {
        self.buf.extend_cache_duration(duration);
    }
}
