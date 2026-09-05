use std::{ops::Range, time::Duration};

use gpui::{
    App, Bounds, Context, CursorStyle, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, InteractiveElement, KeyBinding, LayoutId, PaintQuad, Pixels, Render,
    ShapedLine, SharedString, Subscription, Task, TextRun, UTF16Selection, Window, actions, div,
    fill, hsla, point, prelude::*, px, relative, rgb, size,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    aviqtl_gui_lab,
    [Backspace, Delete, MoveLeft, MoveRight, MoveHome, MoveEnd]
);

pub fn install_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("AviQtlTextInput")),
        KeyBinding::new("delete", Delete, Some("AviQtlTextInput")),
        KeyBinding::new("left", MoveLeft, Some("AviQtlTextInput")),
        KeyBinding::new("right", MoveRight, Some("AviQtlTextInput")),
        KeyBinding::new("home", MoveHome, Some("AviQtlTextInput")),
        KeyBinding::new("end", MoveEnd, Some("AviQtlTextInput")),
    ]);
}

pub struct TextInputEditor {
    value: String,
    pub focus_handle: FocusHandle,
    cursor: usize,
    cursor_visible: bool,
    blink_task: Task<()>,
    _focus_subscriptions: Vec<Subscription>,
}

impl TextInputEditor {
    pub fn new(value: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let value = value.into();
        let cursor = value.len();
        let focus_handle = cx.focus_handle();
        let focus_subscription = cx.on_focus(&focus_handle, window, |this, _, cx| {
            this.start_blink(cx);
        });
        let blur_subscription = cx.on_blur(&focus_handle, window, |this, _, cx| {
            this.stop_blink(cx);
        });
        Self {
            value,
            focus_handle,
            cursor,
            cursor_visible: false,
            blink_task: Task::ready(()),
            _focus_subscriptions: vec![focus_subscription, blur_subscription],
        }
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self.blink_task = Self::spawn_blink_task(cx);
        cx.notify();
    }

    fn stop_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = false;
        self.blink_task = Task::ready(());
        cx.notify();
    }

    fn spawn_blink_task(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if this
                    .update(cx, |editor, cx| {
                        editor.cursor_visible = !editor.cursor_visible;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn reset_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self.blink_task = Self::spawn_blink_task(cx);
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = previous_boundary(&self.value, self.cursor);
        self.reset_blink(cx);
        cx.notify();
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = next_boundary(&self.value, self.cursor);
        self.reset_blink(cx);
        cx.notify();
    }

    fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = 0;
        self.reset_blink(cx);
        cx.notify();
    }

    fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = self.value.len();
        self.reset_blink(cx);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor > 0 {
            let previous = previous_boundary(&self.value, self.cursor);
            self.value.drain(previous..self.cursor);
            self.cursor = previous;
        }
        self.reset_blink(cx);
        cx.notify();
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor < self.value.len() {
            let next = next_boundary(&self.value, self.cursor);
            self.value.drain(self.cursor..next);
        }
        self.reset_blink(cx);
        cx.notify();
    }
}

impl Focusable for TextInputEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInputEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = range_from_utf16(&self.value, &range_utf16);
        actual_range.replace(range_to_utf16(&self.value, &range));
        Some(self.value[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let cursor = offset_to_utf16(&self.value, self.cursor);
        Some(UTF16Selection {
            range: cursor..cursor,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&self.value, range))
            .unwrap_or(self.cursor..self.cursor);
        self.value.replace_range(range.clone(), new_text);
        self.cursor = range.start + new_text.len();
        self.reset_blink(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TextInputEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TextInputText {
            editor: cx.entity(),
        }
    }
}

struct TextInputText {
    editor: Entity<TextInputEditor>,
}

struct TextInputPrepaint {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
}

impl IntoElement for TextInputText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for TextInputText {
    type RequestLayoutState = ();
    type PrepaintState = TextInputPrepaint;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let editor = self.editor.read(cx);
        let content = editor.value.clone();
        let cursor_offset = editor.cursor;
        let style = window.text_style();
        let text: SharedString = if content.is_empty() {
            "日本語 / 中文 input".into()
        } else {
            content.into()
        };
        let color = if editor.value.is_empty() {
            hsla(0.0, 0.0, 0.55, 1.0)
        } else {
            style.color
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
            letter_spacing: None,
        };
        let line = window.text_system().shape_line(
            text,
            style.font_size.to_pixels(window.rem_size()),
            &[run],
            None,
        );
        let cursor = if editor.focus_handle.is_focused(window) && editor.cursor_visible {
            let x = if editor.value.is_empty() {
                px(0.0)
            } else {
                line.x_for_index(cursor_offset)
            };
            Some(fill(
                Bounds::new(
                    point(bounds.left() + x, bounds.top()),
                    size(px(1.5), window.line_height()),
                ),
                style.color,
            ))
        } else {
            None
        };
        TextInputPrepaint { line, cursor }
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        prepaint
            .line
            .paint(
                bounds.origin,
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .expect("paint text input");
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
    }
}

#[derive(IntoElement)]
pub struct TextInput {
    editor: Entity<TextInputEditor>,
}

impl TextInput {
    pub fn new(editor: Entity<TextInputEditor>) -> Self {
        Self { editor }
    }
}

impl gpui::View for TextInput {
    fn entity_id(&self) -> Option<gpui::EntityId> {
        Some(self.editor.entity_id())
    }

    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        let focused = focus_handle.is_focused(window);
        div()
            .key_context("AviQtlTextInput")
            .track_focus(&focus_handle)
            .cursor(CursorStyle::IBeam)
            .map(standard_actions(self.editor.clone()))
            .w_full()
            .h(px(34.0))
            .px_2()
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded_sm()
            .bg(rgb(0x151820))
            .border_1()
            .border_color(if focused {
                rgb(0x5da8ff)
            } else {
                rgb(0x3a404d)
            })
            .text_color(rgb(0xe4e8f0))
            .child(
                self.editor
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
    }
}

fn standard_actions<E: InteractiveElement>(editor: Entity<TextInputEditor>) -> impl FnOnce(E) -> E {
    move |element| {
        element
            .on_action({
                let editor = editor.clone();
                move |action: &MoveLeft, window, cx| {
                    editor.update(cx, |editor, cx| editor.move_left(action, window, cx));
                }
            })
            .on_action({
                let editor = editor.clone();
                move |action: &MoveRight, window, cx| {
                    editor.update(cx, |editor, cx| editor.move_right(action, window, cx));
                }
            })
            .on_action({
                let editor = editor.clone();
                move |action: &MoveHome, window, cx| {
                    editor.update(cx, |editor, cx| editor.move_home(action, window, cx));
                }
            })
            .on_action({
                let editor = editor.clone();
                move |action: &MoveEnd, window, cx| {
                    editor.update(cx, |editor, cx| editor.move_end(action, window, cx));
                }
            })
            .on_action({
                let editor = editor.clone();
                move |action: &Backspace, window, cx| {
                    editor.update(cx, |editor, cx| editor.backspace(action, window, cx));
                }
            })
            .on_action(move |action: &Delete, window, cx| {
                editor.update(cx, |editor, cx| editor.delete(action, window, cx));
            })
    }
}

fn previous_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

fn next_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(content.len())
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in content.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for character in content.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn range_from_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_from_utf16(content, range.start)..offset_from_utf16(content, range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_round_trip_cjk_and_supplementary_characters() {
        let content = "A日本語😀中";
        for byte_offset in content
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(content.len()))
        {
            assert_eq!(
                offset_from_utf16(content, offset_to_utf16(content, byte_offset)),
                byte_offset
            );
        }
    }

    #[test]
    fn cursor_navigation_respects_grapheme_boundaries() {
        let content = "Ame\u{301}日";
        let after_combining_character = "Ame\u{301}".len();
        assert_eq!(
            previous_boundary(content, after_combining_character),
            "Am".len()
        );
        assert_eq!(
            next_boundary(content, "Am".len()),
            after_combining_character
        );
    }
}
