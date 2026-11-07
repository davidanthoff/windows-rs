use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TextControl {
    Text,
    Suggest,
    Rich,
}

/// Uppercases every edit so the native control receives a different value than the user typed.
/// The header is editable so the caret tracking must find the input's editor, not the header's.
pub(super) struct TransformFixture {
    text: Rc<str>,
    calls: u32,
    header: bool,
}

#[derive(Clone)]
pub(super) enum TransformMessage {
    Changed(Rc<str>),
    ToggleHeader,
}

impl Component for TransformFixture {
    type Input = TextControl;
    type Message = TransformMessage;

    fn create(_: &TextControl, _: &ComponentContext<Self>) -> Self {
        Self {
            text: Rc::from(""),
            calls: 0,
            header: true,
        }
    }

    fn update(&mut self, message: TransformMessage, _: &ComponentContext<Self>) {
        match message {
            TransformMessage::Changed(text) => {
                self.text = Rc::from(text.to_uppercase());
                self.calls += 1;
            }
            TransformMessage::ToggleHeader => self.header = !self.header,
        }
    }

    fn view(&self, control: &TextControl, context: &mut ViewContext<Self>) -> View {
        let header = self
            .header
            .then(|| TextBox::new().automation_id("header").text("Header"));
        let input: View = match control {
            TextControl::Text => {
                let mut input = TextBox::new()
                    .automation_id("input")
                    .text(self.text.clone())
                    .on_text_changed(context.callback(TransformMessage::Changed));
                if let Some(header) = header {
                    input = input.header(header);
                }
                input.into()
            }
            TextControl::Suggest => {
                let mut input = AutoSuggestBox::new()
                    .automation_id("input")
                    .text(self.text.clone())
                    .on_text_changed(context.callback(TransformMessage::Changed));
                if let Some(header) = header {
                    input = input.header(header);
                }
                input.into()
            }
            TextControl::Rich => {
                let mut input = RichEditBox::new()
                    .automation_id("input")
                    .text(self.text.clone())
                    .on_text_changed(context.callback(TransformMessage::Changed));
                if let Some(header) = header {
                    input = input.header(header);
                }
                input.into()
            }
        };
        let title = match control {
            TextControl::Text => "Reactor UI fixture: text-transform",
            TextControl::Suggest => "Reactor UI fixture: suggest-transform",
            TextControl::Rich => "Reactor UI fixture: rich-transform",
        };
        context.window_frame(
            title,
            StackPanel::new().margin(20.0).spacing(12.0).children((
                input,
                TextBlock::new()
                    .automation_id("echo")
                    .text(format!("Text {}; calls {}", self.text, self.calls)),
                Button::new()
                    .automation_id("toggle-header")
                    .content("Toggle header")
                    .on_click(context.message(TransformMessage::ToggleHeader)),
            )),
        )
    }
}

pub(super) const MULTILINE_TEXT: &str = "First line\rSecond line";

/// One TextBox declared multi-line with its text, and one that becomes multi-line in the same
/// update in which its text gains a second line. The echo counts the text changes the app is
/// told about: setting declared text must not report any.
pub(super) struct MultilineFixture {
    expanded: bool,
    changes: u32,
}

#[derive(Clone)]
pub(super) enum MultilineMessage {
    Changed,
    Expand,
}

impl Component for MultilineFixture {
    type Input = ();
    type Message = MultilineMessage;

    fn create(_: &(), _: &ComponentContext<Self>) -> Self {
        Self {
            expanded: false,
            changes: 0,
        }
    }

    fn update(&mut self, message: MultilineMessage, _: &ComponentContext<Self>) {
        match message {
            MultilineMessage::Changed => self.changes += 1,
            MultilineMessage::Expand => self.expanded = true,
        }
    }

    fn view(&self, _: &(), context: &mut ViewContext<Self>) -> View {
        let expanding = TextBox::new().automation_id("expanding");
        let expanding = if self.expanded {
            expanding.text(MULTILINE_TEXT).accepts_return(true)
        } else {
            expanding.text("One line")
        };
        context.window_frame(
            "Reactor UI fixture: text-multiline",
            StackPanel::new().margin(20.0).spacing(12.0).children((
                TextBox::new()
                    .automation_id("initial")
                    .text(MULTILINE_TEXT)
                    .accepts_return(true)
                    .on_text_changed(context.callback(|_: Rc<str>| MultilineMessage::Changed)),
                expanding.on_text_changed(context.callback(|_: Rc<str>| MultilineMessage::Changed)),
                TextBlock::new()
                    .automation_id("echo")
                    .text(format!("Changes {}", self.changes)),
                Button::new()
                    .automation_id("expand")
                    .content("Expand")
                    .on_click(context.message(MultilineMessage::Expand)),
            )),
        )
    }
}
