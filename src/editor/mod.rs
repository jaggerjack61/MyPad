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
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorSnapshot {
    text: String,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
        self.set_cursor(cursor);
        self.clear_history();
    }

    pub fn set_from_disk(&mut self, path: Option<PathBuf>, text: String) {
        self.path = path;
        self.content = text_editor::Content::with_text(&text);
        self.saved_text = self.content.text();
        self.sync_cursor();
        self.clear_history();
    }

    pub fn reload_from_disk(&mut self, path: Option<PathBuf>, text: String) {
        let cursor = self.cursor;

        self.path = path;
        self.content = text_editor::Content::with_text(&text);
        self.saved_text = self.content.text();
        self.set_cursor(cursor);
        self.clear_history();
    }

    pub fn apply_action(&mut self, action: text_editor::Action) {
        let prior_snapshot = action.is_edit().then(|| self.snapshot());

        self.content.perform(action);
        self.sync_cursor();

        if let Some(snapshot) = prior_snapshot {
            let current = self.snapshot();

            if current != snapshot {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }
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

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };

        let current = self.snapshot();
        self.restore_snapshot(snapshot);
        self.redo_stack.push(current);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };

        let current = self.snapshot();
        self.restore_snapshot(snapshot);
        self.undo_stack.push(current);
        true
    }

    fn set_cursor(&mut self, cursor: CursorLocation) {
        self.content.move_to(clamp_cursor(&self.content, cursor));
        self.sync_cursor();
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.content.text(),
            cursor: self.cursor,
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.content = text_editor::Content::with_text(&snapshot.text);
        self.set_cursor(snapshot.cursor);
    }

    fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn sync_cursor(&mut self) {
        let position = self.content.cursor().position;

        self.cursor = CursorLocation {
            line: position.line.saturating_add(1),
            column: position.column.saturating_add(1),
        };
    }
}

fn clamp_cursor(
    content: &text_editor::Content,
    cursor: CursorLocation,
) -> text_editor::Cursor {
    let line_index = clamp_line(content, cursor.line.saturating_sub(1));
    let max_column = content
        .line(line_index)
        .map(|line| line.text.chars().count())
        .unwrap_or(0);
    let column_index = cursor.column.saturating_sub(1).min(max_column);

    text_editor::Cursor {
        position: text_editor::Position {
            line: line_index,
            column: column_index,
        },
        selection: None,
    }
}

fn clamp_line(content: &text_editor::Content, line_index: usize) -> usize {
    let last_line = content.line_count().max(1).saturating_sub(1);
    line_index.min(last_line)
}

#[cfg(test)]
mod tests {
    use super::{CursorLocation, EditorBuffer};
    use iced::widget::text_editor;
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

    #[test]
    fn reload_from_disk_preserves_cursor_location() {
        let path = PathBuf::from("notes.md");
        let mut buffer = EditorBuffer::new(Some(path.clone()), "hello\nworld");

        buffer.replace_text(
            "hello\nworld".to_string(),
            CursorLocation { line: 2, column: 4 },
        );

        buffer.reload_from_disk(Some(path), "hello\nworld".to_string());

        assert_eq!(buffer.cursor_location(), CursorLocation { line: 2, column: 4 });
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn reload_from_disk_clamps_cursor_to_new_text_bounds() {
        let path = PathBuf::from("notes.md");
        let mut buffer = EditorBuffer::new(Some(path.clone()), "alpha\nbeta");

        buffer.replace_text(
            "alpha\nbeta".to_string(),
            CursorLocation { line: 2, column: 5 },
        );

        buffer.reload_from_disk(Some(path), "z".to_string());

        assert_eq!(buffer.cursor_location(), CursorLocation { line: 1, column: 2 });
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn undo_and_redo_restore_text_and_cursor() {
        let mut buffer = EditorBuffer::new(None, "a");

        buffer.replace_text("a".to_string(), CursorLocation { line: 1, column: 2 });
        buffer.apply_action(text_editor::Action::Edit(text_editor::Edit::Insert('b')));

        assert_eq!(buffer.text(), "ab");
        assert_eq!(buffer.cursor_location(), CursorLocation { line: 1, column: 3 });

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "a");
        assert_eq!(buffer.cursor_location(), CursorLocation { line: 1, column: 2 });

        assert!(buffer.redo());
        assert_eq!(buffer.text(), "ab");
        assert_eq!(buffer.cursor_location(), CursorLocation { line: 1, column: 3 });
    }

    #[test]
    fn new_edit_clears_redo_history() {
        let mut buffer = EditorBuffer::new(None, "a");

        buffer.replace_text("a".to_string(), CursorLocation { line: 1, column: 2 });
        buffer.apply_action(text_editor::Action::Edit(text_editor::Edit::Insert('b')));
        assert!(buffer.undo());

        buffer.apply_action(text_editor::Action::Edit(text_editor::Edit::Insert('c')));

        assert!(!buffer.redo());
        assert_eq!(buffer.text(), "ac");
    }
}