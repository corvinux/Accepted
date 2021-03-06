use std::borrow::Cow;
use std::cmp::{max, min};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthChar;

use crate::compiler::CompileId;
use crate::compiler::CompileResult;
use crate::compiler::Compiler;
use crate::config;
use crate::config::types::keys;
use crate::core::Core;
use crate::core::Cursor;
use crate::core::CursorRange;
use crate::core::Id;
use crate::draw;
use crate::draw::{styles, CharStyle, LinenumView, TermView};
use crate::draw_cache::DrawCache;
use crate::formatter;
use crate::lsp::LSPClient;
use crate::ropey_util::RopeExt;
use crate::storage::Storage;
use crate::syntax;

pub struct Yank {
    pub insert_newline: bool,
    pub content: String,
}

impl Default for Yank {
    fn default() -> Self {
        Yank {
            insert_newline: false,
            content: String::new(),
        }
    }
}

fn get_rows(s: &str, width: usize) -> usize {
    let mut x = 0;
    let mut y = 1;

    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if x + w < width {
            x += w;
        } else {
            y += 1;
            x = w;
        }
    }
    y
}

enum ShowCursor {
    None,
    Show,
    ShowMiddle,
}

pub struct Buffer<'a> {
    storage: Option<Box<dyn Storage>>,
    pub core: Core,
    pub search: Vec<char>,
    syntax_parent: &'a syntax::SyntaxParent,
    config: &'a config::ConfigWithDefault,
    syntax: syntax::Syntax<'a>,
    pub snippet: BTreeMap<String, String>,
    pub yank: Yank,
    last_save: Id,
    pub lsp: Option<LSPClient>,
    compiler: Option<Compiler<'a>>,
    row_offset: usize,
    last_compiler_result: Option<CompileResult>,
    cache: DrawCache<'a>,
    buffer_update: Id,
    last_compiler_submit: CompileId,
    last_compiler_compiled: CompileId,
    show_cursor_on_draw: ShowCursor,
}

impl<'a> Buffer<'a> {
    pub fn new(
        syntax_parent: &'a syntax::SyntaxParent,
        config: &'a config::ConfigWithDefault,
    ) -> Self {
        let syntax = syntax_parent.load_syntax_or_txt("txt");

        let mut res = Self {
            storage: None,
            core: Core::default(),
            search: Vec::new(),
            cache: DrawCache::new(&syntax),
            syntax,
            snippet: BTreeMap::new(),
            yank: Yank::default(),
            last_save: Id::default(),
            lsp: None,
            compiler: config.get::<keys::Compiler>(None).map(Compiler::new),
            row_offset: 0,
            last_compiler_result: None,
            syntax_parent,
            config,
            buffer_update: Id::default(),
            last_compiler_submit: CompileId::default(),
            last_compiler_compiled: CompileId::default(),
            show_cursor_on_draw: ShowCursor::None,
        };
        res.restart_lsp();
        res.reset_snippet();
        res
    }

    pub fn path(&self) -> Option<&Path> {
        self.storage.as_ref().map(|s| s.path())
    }

    fn extension(&self) -> Option<&OsStr> {
        self.path().and_then(Path::extension)
    }

    pub fn storage(&self) -> Option<&dyn Storage> {
        self.storage.as_ref().map(AsRef::as_ref)
    }

    pub fn get_config<A: typemap::Key>(&self) -> Option<&'a A::Value> {
        self.config.get::<A>(self.path())
    }

    fn reset_snippet(&mut self) {
        self.snippet = self.config.snippets(self.path());
    }

    pub fn extend_cache_duration(&mut self, duration: std::time::Duration) {
        self.cache
            .extend_cache_duration(self.core.buffer(), duration);
    }

    pub fn indent_width(&self) -> usize {
        self.get_config::<keys::IndentWidth>().cloned().unwrap_or(4)
    }

    pub fn restart_lsp(&mut self) {
        let ext = self
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.lsp = self
            .config
            .get::<keys::LSP>(self.path())
            .and_then(|c| LSPClient::start(c.command(), ext).ok());
    }

    fn set_syntax(&mut self, extension: &str) {
        self.syntax = self.syntax_parent.load_syntax_or_txt(extension);
        self.cache = DrawCache::new(&self.syntax);
    }

    pub fn set_language(&mut self) {
        self.compiler = self.get_config::<keys::Compiler>().map(Compiler::new);
        self.restart_lsp();
    }

    pub fn indent(&mut self) {
        self.core.indent(self.indent_width());
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.storage = Some(Box::new(path));
        self.set_language();
    }

    pub fn open<S: Storage + 'static>(&mut self, mut storage: S) {
        self.core = storage.load();
        self.storage = Some(Box::new(storage));
        let path = self.path().unwrap();

        let syntax_extension = self
            .get_config::<keys::SyntaxExtension>()
            .cloned()
            .or_else(|| {
                path.extension()
                    .or_else(|| path.file_name())
                    .unwrap_or_default()
                    .to_str()
                    .map(String::from)
            })
            .unwrap_or_default();
        self.set_syntax(&syntax_extension);

        self.row_offset = 0;
        self.last_save = self.core.buffer_changed();
        self.set_language();
        self.cache = DrawCache::new(&self.syntax);
        self.compile(false);
        self.reset_snippet();
    }

    pub fn save(&mut self, is_optimize: bool) -> bool {
        let saved = if let Some(storage) = self.storage.as_mut() {
            storage.save(&self.core)
        } else {
            false
        };
        if saved {
            self.compile(is_optimize);
        }
        saved
    }

    pub fn show_cursor(&mut self) {
        self.show_cursor_on_draw = ShowCursor::Show;
    }

    pub fn show_cursor_middle(&mut self) {
        self.show_cursor_on_draw = ShowCursor::ShowMiddle;
    }

    fn show_cursor_(&mut self, rows: usize, cols: usize) {
        if self.row_offset >= self.core.cursor().row {
            self.row_offset = self.core.cursor().row;
        } else {
            if cols < LinenumView::prefix_width(self.core.buffer().len_lines()) {
                return;
            }
            let cols = cols - LinenumView::prefix_width(self.core.buffer().len_lines());
            let mut i = self.core.cursor().row + 1;
            let mut sum = 0;
            while i > 0 && sum + get_rows(&Cow::from(self.core.buffer().l(i - 1)), cols) <= rows {
                sum += get_rows(&Cow::from(self.core.buffer().l(i - 1)), cols);
                i -= 1;
            }
            self.row_offset = max(i, self.row_offset);
        }
    }

    fn show_cursor_middle_(&mut self, rows: usize) {
        if rows / 2 > self.core.cursor().row {
            self.row_offset = 0;
        } else {
            self.row_offset = self.core.cursor().row - rows / 2;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.row_offset < 3 {
            self.row_offset = 0;
        } else {
            self.row_offset -= 3;
        }
    }

    pub fn scroll_down(&mut self) {
        self.row_offset = min(self.row_offset + 3, self.core.buffer().len_lines() - 1);
    }

    pub fn format(&mut self) {
        let src = self.core.get_string();
        let formatter = self.config.get::<keys::Formatter>(self.path());

        if let Some(formatter) = formatter {
            if let Some(formatted) = formatter::system_format(formatter.command(), &src) {
                if formatted != self.core.get_string() {
                    self.core.set_string(formatted, false);
                }
            }
        }
    }

    pub fn compile(&mut self, is_optimize: bool) {
        if self.last_compiler_submit
            == (CompileId {
                id: self.core.buffer_changed(),
                is_optimize,
            })
        {
            return;
        }
        self.last_compiler_submit = CompileId {
            id: self.core.buffer_changed(),
            is_optimize,
        };

        if let Some(path) = self.path() {
            if let Some(compiler) = self.compiler.as_ref() {
                compiler.compile(path.to_path_buf(), self.last_compiler_submit);
            }
        }
    }

    pub fn last_compile_success(&self) -> Option<bool> {
        self.last_compiler_result.as_ref().map(|res| res.success)
    }

    fn is_annotate(&self, cursor: Cursor) -> bool {
        self.last_compiler_result
            .as_ref()
            .map(|res| res.messages.iter().any(|r| r.span.contains(cursor)))
            .unwrap_or(false)
    }

    pub fn compiler_message_on_cursor(&self) -> Option<&str> {
        let line = self.core.cursor().row;
        self.last_compiler_result.as_ref().and_then(|res| {
            res.messages
                .iter()
                .find(|r| r.line == line)
                .map(|r| r.message.as_str())
        })
    }

    pub fn poll_compile_message(&mut self) {
        if let Some(compiler) = self.compiler.as_ref() {
            while let Some((id, res)) = compiler.try_recv_compile_result() {
                self.last_compiler_compiled = id;
                self.last_compiler_result = Some(res);
            }
        }
    }

    pub fn wait_compile_message(&mut self) {
        while self.is_compiling() {
            if let Some(compiler) = self.compiler.as_ref() {
                if let Some((id, res)) = compiler.recv_compile_result() {
                    self.last_compiler_compiled = id;
                    self.last_compiler_result = Some(res);
                }
            }
        }
    }

    pub fn is_compiling(&self) -> bool {
        self.compiler
            .as_ref()
            .map(Compiler::is_compiling)
            .unwrap_or(false)
    }

    pub fn draw(&mut self, view: TermView) -> Option<Cursor> {
        self.poll_compile_message();
        self.draw_with_selected(view, None)
    }

    pub fn draw_with_selected(
        &mut self,
        mut view: TermView,
        selected: Option<CursorRange>,
    ) -> Option<Cursor> {
        match self.show_cursor_on_draw {
            ShowCursor::ShowMiddle => {
                self.show_cursor_middle_(view.height());
            }
            ShowCursor::Show => {
                self.show_cursor_(view.height(), view.width());
            }
            ShowCursor::None => {}
        }
        self.show_cursor_on_draw = ShowCursor::None;
        view.bg = self.syntax.theme.settings.background.map(Into::into);
        let v = Vec::new();
        let compiler_outputs = self
            .last_compiler_result
            .as_ref()
            .map(|res| &res.messages)
            .unwrap_or_else(|| &v);
        let mut view = LinenumView::new(
            self.row_offset,
            self.core.buffer().len_lines(),
            &compiler_outputs,
            view,
        );
        let mut cursor = None;

        if self.buffer_update != self.core.buffer_changed() {
            self.buffer_update = self.core.buffer_changed();
            self.cache.dirty_from(self.core.dirty_from);
        }

        'outer: for i in self.row_offset..self.core.buffer().len_lines() {
            self.cache.cache_line(self.core.buffer(), i);
            let line_ref = self.cache.get_line(i).unwrap();
            let mut line = Cow::Borrowed(line_ref);

            self.core.dirty_from = i;

            if !self.search.is_empty() && line.len() >= self.search.len() {
                for j in 0..=line.len() - self.search.len() {
                    let m = self
                        .search
                        .iter()
                        .zip(line[j..j + self.search.len()].iter())
                        .all(|(c1, (c2, _))| c1 == c2);
                    if m {
                        for k in j..j + self.search.len() {
                            line.to_mut()[k].1 = draw::styles::HIGHLIGHT;
                        }
                    }
                }
            }

            for (j, &c) in line.iter().enumerate() {
                let (c, mut style) = c;
                let t = Cursor { row: i, col: j };

                if self.is_annotate(t) {
                    style.modification = draw::CharModification::UnderLine;
                }

                let style = if selected.as_ref().map(|r| r.contains(t)) == Some(true) {
                    styles::SELECTED
                } else {
                    style
                };

                if self.core.cursor() == t {
                    cursor = view.put(c, style, Some(t));
                } else if view.put(c, style, Some(t)).is_none() {
                    break 'outer;
                }
            }
            let t = Cursor {
                row: i,
                col: self.core.buffer().l(i).len_chars(),
            };

            if self.core.cursor() == t {
                cursor = view.cursor();
            }

            if self.core.buffer().l(i).len_chars() == 0 {
                if let Some(col) = self.syntax.theme.settings.background {
                    view.put(' ', CharStyle::bg(col.into()), Some(t));
                } else {
                    view.put(' ', styles::DEFAULT, Some(t));
                }
            }

            if i != self.core.buffer().len_lines() - 1 {
                if let Some(col) = self.syntax.theme.settings.background {
                    while !view.cause_newline(' ') {
                        view.put(' ', CharStyle::bg(col.into()), Some(t));
                    }
                } else {
                    while !view.cause_newline(' ') {
                        view.put(' ', styles::DEFAULT, Some(t));
                    }
                }
                view.newline();
            }
        }

        cursor
    }
}
