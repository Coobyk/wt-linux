//! `vte::ansi::Handler` shim: forwards every call to alacritty's `Term`, intercepting
//! SGR to track blink (5/6/25), which alacritty drops.
//!
//! Method list generated from vte 0.15's `Handler` trait; re-check when bumping vte so no
//! new method silently falls back to the trait's no-op default.

#![allow(clippy::all)]

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::*;

use crate::{BLINK_FLAG, Listener, Palette};

pub(crate) struct Shim<'a> {
    pub term: &'a mut Term<Listener>,
    pub listener: &'a Listener,
    pub palette: &'a Palette,
    /// Set while a DECSCUSR is in force; [`Terminal::cursor`] then reports the app's shape.
    pub shape_set: &'a mut bool,
}

impl Shim<'_> {
    /// Absolute line of the scroll region's top in origin mode, else 0.
    ///
    /// Alacritty's relative moves pass the absolute cursor line to `goto`, which adds the
    /// origin offset again; callers subtract this first. The region isn't public, so probe
    /// it with `goto(0, _)` and restore the cursor.
    fn origin_top(&mut self) -> i32 {
        if !self.term.mode().contains(TermMode::ORIGIN) {
            return 0;
        }
        let cursor = &self.term.grid().cursor;
        let (point, wrap) = (cursor.point, cursor.input_needs_wrap);
        Handler::goto(self.term, 0, point.column.0);
        let top = self.term.grid().cursor.point.line.0;
        let cursor = &mut self.term.grid_mut().cursor;
        cursor.point = point;
        cursor.input_needs_wrap = wrap;
        top
    }

    /// Move to a line relative to the current one, as `goto` (origin-relative) expects.
    fn goto_relative(&mut self, delta: i32, col: Option<usize>) {
        let top = self.origin_top();
        let point = self.term.grid().cursor.point;
        let line = (point.line.0 - top + delta).max(0);
        Handler::goto(self.term, line, col.unwrap_or(point.column.0));
    }
}

impl Handler for Shim<'_> {
    fn terminal_attribute(&mut self, attr: Attr) {
        let template = &mut self.term.grid_mut().cursor.template.flags;
        match attr {
            Attr::BlinkSlow | Attr::BlinkFast => template.insert(BLINK_FLAG),
            Attr::CancelBlink => template.remove(BLINK_FLAG),
            _ => {
                let keep_blink = template.contains(BLINK_FLAG) && !matches!(attr, Attr::Reset);
                Handler::terminal_attribute(self.term, attr);
                if keep_blink {
                    self.term
                        .grid_mut()
                        .cursor
                        .template
                        .flags
                        .insert(BLINK_FLAG);
                }
            }
        }
    }

    fn set_title(&mut self, a0: Option<String>) {
        Handler::set_title(self.term, a0)
    }

    /// DECSCUSR: `Some` takes over the drawn shape until `None` restores the profile default.
    fn set_cursor_style(&mut self, a0: Option<CursorStyle>) {
        *self.shape_set = a0.is_some();
        Handler::set_cursor_style(self.term, a0)
    }

    fn set_cursor_shape(&mut self, a0: CursorShape) {
        *self.shape_set = true;
        Handler::set_cursor_shape(self.term, a0)
    }

    fn input(&mut self, a0: char) {
        Handler::input(self.term, a0)
    }

    fn goto(&mut self, a0: i32, a1: usize) {
        Handler::goto(self.term, a0, a1)
    }

    fn goto_line(&mut self, a0: i32) {
        Handler::goto_line(self.term, a0)
    }

    fn goto_col(&mut self, a0: usize) {
        self.goto_relative(0, Some(a0))
    }

    fn insert_blank(&mut self, a0: usize) {
        Handler::insert_blank(self.term, a0)
    }

    fn move_up(&mut self, a0: usize) {
        self.goto_relative(-(a0.min(i32::MAX as usize) as i32), None)
    }

    fn move_down(&mut self, a0: usize) {
        self.goto_relative(a0.min(i32::MAX as usize) as i32, None)
    }

    fn identify_terminal(&mut self, a0: Option<char>) {
        Handler::identify_terminal(self.term, a0)
    }

    fn device_status(&mut self, a0: usize) {
        Handler::device_status(self.term, a0)
    }

    fn move_forward(&mut self, a0: usize) {
        Handler::move_forward(self.term, a0)
    }

    fn move_backward(&mut self, a0: usize) {
        Handler::move_backward(self.term, a0)
    }

    fn move_down_and_cr(&mut self, a0: usize) {
        self.goto_relative(a0.min(i32::MAX as usize) as i32, Some(0))
    }

    fn move_up_and_cr(&mut self, a0: usize) {
        self.goto_relative(-(a0.min(i32::MAX as usize) as i32), Some(0))
    }

    fn put_tab(&mut self, a0: u16) {
        Handler::put_tab(self.term, a0)
    }

    fn backspace(&mut self) {
        Handler::backspace(self.term)
    }

    fn carriage_return(&mut self) {
        Handler::carriage_return(self.term)
    }

    fn linefeed(&mut self) {
        Handler::linefeed(self.term)
    }

    fn bell(&mut self) {
        Handler::bell(self.term)
    }

    fn substitute(&mut self) {
        Handler::substitute(self.term)
    }

    fn newline(&mut self) {
        Handler::newline(self.term)
    }

    fn set_horizontal_tabstop(&mut self) {
        Handler::set_horizontal_tabstop(self.term)
    }

    fn scroll_up(&mut self, a0: usize) {
        Handler::scroll_up(self.term, a0)
    }

    fn scroll_down(&mut self, a0: usize) {
        Handler::scroll_down(self.term, a0)
    }

    fn insert_blank_lines(&mut self, a0: usize) {
        Handler::insert_blank_lines(self.term, a0)
    }

    fn delete_lines(&mut self, a0: usize) {
        Handler::delete_lines(self.term, a0)
    }

    fn erase_chars(&mut self, a0: usize) {
        Handler::erase_chars(self.term, a0)
    }

    fn delete_chars(&mut self, a0: usize) {
        // Alacritty clamps to the row width, not to the cells right of the cursor, and
        // would then clear cells left of the cursor.
        let remaining = self.term.columns() - self.term.grid().cursor.point.column.0;
        Handler::delete_chars(self.term, a0.min(remaining))
    }

    fn move_backward_tabs(&mut self, a0: u16) {
        Handler::move_backward_tabs(self.term, a0)
    }

    fn move_forward_tabs(&mut self, a0: u16) {
        Handler::move_forward_tabs(self.term, a0)
    }

    fn save_cursor_position(&mut self) {
        Handler::save_cursor_position(self.term)
    }

    fn restore_cursor_position(&mut self) {
        Handler::restore_cursor_position(self.term)
    }

    fn clear_line(&mut self, a0: LineClearMode) {
        Handler::clear_line(self.term, a0)
    }

    fn clear_screen(&mut self, a0: ClearMode) {
        Handler::clear_screen(self.term, a0)
    }

    fn clear_tabs(&mut self, a0: TabulationClearMode) {
        Handler::clear_tabs(self.term, a0)
    }

    fn set_tabs(&mut self, a0: u16) {
        Handler::set_tabs(self.term, a0)
    }

    fn reset_state(&mut self) {
        Handler::reset_state(self.term);
        self.listener.send_event(Event::ResetTitle);
    }

    fn reverse_index(&mut self) {
        Handler::reverse_index(self.term)
    }

    fn set_mode(&mut self, a0: Mode) {
        Handler::set_mode(self.term, a0)
    }

    fn unset_mode(&mut self, a0: Mode) {
        Handler::unset_mode(self.term, a0)
    }

    fn report_mode(&mut self, a0: Mode) {
        Handler::report_mode(self.term, a0)
    }

    fn set_private_mode(&mut self, a0: PrivateMode) {
        Handler::set_private_mode(self.term, a0)
    }

    fn unset_private_mode(&mut self, a0: PrivateMode) {
        Handler::unset_private_mode(self.term, a0)
    }

    fn report_private_mode(&mut self, a0: PrivateMode) {
        Handler::report_private_mode(self.term, a0)
    }

    fn set_scrolling_region(&mut self, a0: usize, a1: Option<usize>) {
        Handler::set_scrolling_region(self.term, a0, a1)
    }

    fn set_keypad_application_mode(&mut self) {
        Handler::set_keypad_application_mode(self.term)
    }

    fn unset_keypad_application_mode(&mut self) {
        Handler::unset_keypad_application_mode(self.term)
    }

    fn set_active_charset(&mut self, a0: CharsetIndex) {
        Handler::set_active_charset(self.term, a0)
    }

    fn configure_charset(&mut self, a0: CharsetIndex, a1: StandardCharset) {
        Handler::configure_charset(self.term, a0, a1)
    }

    fn set_color(&mut self, a0: usize, a1: Rgb) {
        Handler::set_color(self.term, a0, a1)
    }

    /// Answered here rather than via alacritty's `ColorRequest` event so the reply
    /// reflects the colors at this point in the stream, not after the whole chunk.
    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        let p = crate::effective_palette(self.term, self.palette);
        let rgb = match index {
            0..=255 => p.colors[index],
            i if i == NamedColor::Foreground as usize => p.foreground,
            i if i == NamedColor::Background as usize => p.background,
            i if i == NamedColor::Cursor as usize => p.cursor,
            _ => return,
        };
        let (r, g, b) = (rgb.r, rgb.g, rgb.b);
        self.listener.send_event(Event::PtyWrite(format!(
            "\x1b]{prefix};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{terminator}"
        )));
    }

    fn reset_color(&mut self, a0: usize) {
        Handler::reset_color(self.term, a0)
    }

    fn clipboard_store(&mut self, a0: u8, a1: &[u8]) {
        Handler::clipboard_store(self.term, a0, a1)
    }

    fn clipboard_load(&mut self, a0: u8, a1: &str) {
        Handler::clipboard_load(self.term, a0, a1)
    }

    fn decaln(&mut self) {
        Handler::decaln(self.term)
    }

    fn push_title(&mut self) {
        Handler::push_title(self.term)
    }

    fn pop_title(&mut self) {
        Handler::pop_title(self.term)
    }

    fn text_area_size_pixels(&mut self) {
        Handler::text_area_size_pixels(self.term)
    }

    fn text_area_size_chars(&mut self) {
        Handler::text_area_size_chars(self.term)
    }

    fn set_hyperlink(&mut self, a0: Option<Hyperlink>) {
        Handler::set_hyperlink(self.term, a0)
    }

    fn set_mouse_cursor_icon(&mut self, a0: cursor_icon::CursorIcon) {
        Handler::set_mouse_cursor_icon(self.term, a0)
    }

    fn report_keyboard_mode(&mut self) {
        Handler::report_keyboard_mode(self.term)
    }

    fn push_keyboard_mode(&mut self, a0: KeyboardModes) {
        Handler::push_keyboard_mode(self.term, a0)
    }

    fn pop_keyboard_modes(&mut self, a0: u16) {
        Handler::pop_keyboard_modes(self.term, a0)
    }

    fn set_keyboard_mode(&mut self, a0: KeyboardModes, a1: KeyboardModesApplyBehavior) {
        Handler::set_keyboard_mode(self.term, a0, a1)
    }

    fn set_modify_other_keys(&mut self, a0: ModifyOtherKeys) {
        Handler::set_modify_other_keys(self.term, a0)
    }

    fn report_modify_other_keys(&mut self) {
        Handler::report_modify_other_keys(self.term)
    }

    fn set_scp(&mut self, a0: ScpCharPath, a1: ScpUpdateMode) {
        Handler::set_scp(self.term, a0, a1)
    }
}
