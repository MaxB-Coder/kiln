//! A small single-line text field for search and the palette.
//!
//! Typed text arrives through the platform input method (so dead keys and IMEs work); editing
//! keys are handled here. Enter, Escape and the arrow keys are reported to the owner.

use std::ops::Range;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels,
    Point, Render, SharedString, Styled, UTF16Selection, Window, canvas, div, px,
};

use crate::grid_paint::FONT_FAMILY;
use crate::theme::{self, palette};

pub enum TextInputEvent {
    Changed,
    Submit { shift: bool },
    Cancel,
    Up,
    Down,
}

impl EventEmitter<TextInputEvent> for TextInput {}

pub struct TextInput {
    focus: FocusHandle,
    text: String,
    /// Byte offset of the cursor.
    cursor: usize,
    marked: Option<String>,
    placeholder: SharedString,
    size: Pixels,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            cursor: 0,
            marked: None,
            placeholder: placeholder.into(),
            size: px(13.),
        }
    }

    /// A smaller field (the hex values in Settings).
    pub fn with_size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Replaces the text without reporting a change (the owner set it).
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.text != text {
            self.text = text.to_string();
            self.cursor = self.text.len();
            cx.notify();
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        // One line only: pasted newlines become spaces.
        let text = text.replace(['\r', '\n'], " ");
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn prev_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .char_indices()
            .last()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, from: usize) -> usize {
        self.text[from..]
            .chars()
            .next()
            .map_or(from, |c| from + c.len_utf8())
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let word = ks.modifiers.alt || (cfg!(not(target_os = "macos")) && ks.modifiers.control);
        let line = ks.modifiers.platform;
        let handled = match ks.key.as_str() {
            "enter" => {
                cx.emit(TextInputEvent::Submit {
                    shift: ks.modifiers.shift,
                });
                true
            }
            "escape" => {
                cx.emit(TextInputEvent::Cancel);
                true
            }
            "up" => {
                cx.emit(TextInputEvent::Up);
                true
            }
            "down" => {
                cx.emit(TextInputEvent::Down);
                true
            }
            "backspace" if self.cursor > 0 => {
                let start = if line {
                    0
                } else if word {
                    self.word_start(self.cursor)
                } else {
                    self.prev_boundary(self.cursor)
                };
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
                cx.emit(TextInputEvent::Changed);
                true
            }
            "delete" if self.cursor < self.text.len() => {
                let end = self.next_boundary(self.cursor);
                self.text.replace_range(self.cursor..end, "");
                cx.emit(TextInputEvent::Changed);
                true
            }
            "left" => {
                self.cursor = if line {
                    0
                } else if word {
                    self.word_start(self.cursor)
                } else {
                    self.prev_boundary(self.cursor)
                };
                true
            }
            "right" => {
                self.cursor = if line {
                    self.text.len()
                } else {
                    self.next_boundary(self.cursor)
                };
                true
            }
            "home" => {
                self.cursor = 0;
                true
            }
            "end" => {
                self.cursor = self.text.len();
                true
            }
            "v" if ks.modifiers.platform || (ks.modifiers.control && ks.modifiers.shift) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.insert(&text, cx);
                }
                true
            }
            "c" if ks.modifiers.platform || (ks.modifiers.control && ks.modifiers.shift) => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.text.clone()));
                true
            }
            "a" if ks.modifiers.control && !ks.modifiers.shift => {
                self.cursor = 0;
                true
            }
            "e" if ks.modifiers.control && !ks.modifiers.shift => {
                self.cursor = self.text.len();
                true
            }
            "u" if ks.modifiers.control && !ks.modifiers.shift => {
                self.text.replace_range(..self.cursor, "");
                self.cursor = 0;
                cx.emit(TextInputEvent::Changed);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn word_start(&self, from: usize) -> usize {
        let before = &self.text[..from];
        let trimmed = before.trim_end();
        trimmed
            .rfind(|c: char| c.is_whitespace() || "/-_.".contains(c))
            .map_or(0, |i| i + 1)
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let focused = self.focus.is_focused(window);
        let caret = self.size * 1.25;
        let entity = cx.entity();
        let focus = self.focus.clone();
        let (before, after) = self.text.split_at(self.cursor);
        let marked = self.marked.clone();
        let empty = self.text.is_empty() && marked.is_none();
        let input_handler = canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
            },
        )
        .absolute()
        .size_full();
        div()
            .key_context("TextInput")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(FONT_FAMILY)
            .text_size(self.size)
            .text_color(theme::hsla(p.fg))
            .child(input_handler)
            .when(empty, |d| {
                d.when(focused, |d| {
                    d.child(div().w(px(2.)).h(caret).bg(theme::hsla(p.accent)))
                })
                .child(
                    div()
                        .pl(px(2.))
                        .text_color(theme::hsla(p.muted))
                        .child(self.placeholder.clone()),
                )
            })
            .when(!empty, |d| {
                d.child(before.to_string())
                    .when_some(marked, |d, marked| {
                        d.child(
                            div()
                                .text_color(theme::hsla(p.modified))
                                .underline()
                                .child(marked),
                        )
                    })
                    .when(focused, |d| {
                        d.child(div().w(px(2.)).h(caret).bg(theme::hsla(p.accent)))
                    })
                    .child(after.to_string())
            })
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let at = self.text[..self.cursor].encode_utf16().count();
        Some(UTF16Selection {
            range: at..at,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        self.insert(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
