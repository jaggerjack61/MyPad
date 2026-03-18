use iced::widget::text_editor;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct EditorBuffer {
    path: Option<PathBuf>,
    content: text_editor::Content,
    saved_text: String,
    cursor: CursorLocation,
}

impl EditorBuffer {
    pub fn new(path: Option<PathBuf>, text: impl Into<String>) -> Self {
        let text = text.into();
        let content = text_editor::Content::with_text(&text);

        let mut buffer = Self {
            path,
            content,
            saved_text: text,
            cursor: CursorLocation { line: 1, column: 1 },
        };

        buffer.sync_cursor();
        buffer
    }

    pub fn open(path: PathBuf, text: String) -> Self {
        Self::new(Some(path), text)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
    }

    pub fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    pub fn file_label(&self) -> String {
        self.path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn replace_text(&mut self, text: String, cursor: CursorLocation) {
        self.content = text_editor::Content::with_text(&text);
        self.cursor = cursor;
    }

    pub fn set_from_disk(&mut self, path: Option<PathBuf>, text: String) {
        self.path = path;
        self.content = text_editor::Content::with_text(&text);
        self.saved_text = self.content.text();
        self.sync_cursor();
    }

    pub fn apply_action(&mut self, action: text_editor::Action) {
        self.content.perform(action);
        self.sync_cursor();
    }

    pub fn cursor_location(&self) -> CursorLocation {
        self.cursor
    }

    pub fn line_numbers(&self) -> Vec<String> {
        (1..=self.content.line_count().max(1))
            .map(|line| line.to_string())
            .collect()
    }

    pub fn is_dirty(&self) -> bool {
        self.content.text() != self.saved_text
    }

    pub fn mark_saved(&mut self) {
        self.saved_text = self.content.text();
    }

    fn sync_cursor(&mut self) {
        let position = self.content.cursor().position;

        self.cursor = CursorLocation {
            line: position.line.saturating_add(1),
            column: position.column.saturating_add(1),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{CursorLocation, EditorBuffer};
    use std::path::PathBuf;

    #[test]
    fn buffer_marks_dirty_after_text_change() {
        let mut buffer = EditorBuffer::new(Some(PathBuf::from("notes.md")), "hello");

        buffer.mark_saved();
        assert!(!buffer.is_dirty());

        buffer.replace_text(
            "hello\nworld".to_string(),
            CursorLocation { line: 2, column: 6 },
        );

        assert!(buffer.is_dirty());
        assert_eq!(buffer.line_numbers(), vec!["1", "2"]);
        assert_eq!(buffer.cursor_location(), CursorLocation { line: 2, column: 6 });
    }

    #[test]
    fn mark_saved_resets_dirty_state() {
        let mut buffer = EditorBuffer::new(None, "draft");

        buffer.replace_text("draft v2".to_string(), CursorLocation { line: 1, column: 9 });
        assert!(buffer.is_dirty());

        buffer.mark_saved();
        assert!(!buffer.is_dirty());
    }
}