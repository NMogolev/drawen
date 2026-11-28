//! Shared text-editing model for single- and multi-line controls.
//!
//! The model stores UTF-8 byte offsets, moves on grapheme-cluster boundaries,
//! and keeps independent undo and redo stacks of interval edits, bounded by
//! both an entry cap and a byte budget. Widgets add layout-dependent behavior
//! such as line navigation, hit testing, and scrolling on top.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

const UNDO_LIMIT: usize = 256;
/// Byte budget for retained undo/redo edit payloads. The newest edit is always
/// kept, so a single paste larger than the budget stays undoable on its own.
const UNDO_BYTE_BUDGET: usize = 4 * 1024 * 1024;

/// One interval edit: the replaced byte range, the removed and inserted text,
/// and the selections before and after. Undo applies the inverse interval and
/// redo re-applies it, so typing into a large document stores only the typed
/// characters instead of a full copy of the buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Edit {
    range: Range<usize>,
    removed: String,
    inserted: String,
    anchor: usize,
    focus: usize,
    caret: usize,
}

impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

/// A selection or caret expressed as UTF-8 byte offsets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextEditor {
    text: String,
    anchor: usize,
    focus: usize,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    undo_bytes: usize,
    redo_bytes: usize,
    multiline: bool,
}

impl TextEditor {
    pub(crate) fn with_text(multiline: bool, text: impl Into<String>) -> Self {
        let text = normalize(multiline, &text.into());
        let focus = text.len();
        Self {
            text,
            anchor: focus,
            focus,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            redo_bytes: 0,
            multiline,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set_text(&mut self, text: impl Into<String>) {
        self.text = normalize(self.multiline, &text.into());
        self.anchor = self.text.len();
        self.focus = self.anchor;
        self.undo.clear();
        self.redo.clear();
        self.undo_bytes = 0;
        self.redo_bytes = 0;
    }

    pub(crate) const fn anchor(&self) -> usize {
        self.anchor
    }

    pub(crate) const fn focus(&self) -> usize {
        self.focus
    }

    pub(crate) fn selection(&self) -> Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.anchor != self.focus
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        let selection = self.selection();
        (!selection.is_empty()).then(|| &self.text[selection])
    }

    /// Moves both ends of the selection to `byte`, clamped to a char boundary.
    pub(crate) fn set_caret(&mut self, byte: usize) {
        let byte = self.clamp(byte);
        self.anchor = byte;
        self.focus = byte;
    }

    pub(crate) fn select(&mut self, anchor: usize, focus: usize) {
        self.anchor = self.clamp(anchor);
        self.focus = self.clamp(focus);
    }

    pub(crate) fn select_all(&mut self) {
        self.anchor = 0;
        self.focus = self.text.len();
    }

    pub(crate) fn move_horizontal(&mut self, forward: bool, by_word: bool, extend: bool) {
        let target = if by_word {
            if forward {
                self.next_word(self.focus)
            } else {
                self.previous_word(self.focus)
            }
        } else if forward {
            self.next_grapheme(self.focus)
        } else {
            self.previous_grapheme(self.focus)
        };
        self.move_to(target, extend);
    }

    pub(crate) fn move_to(&mut self, target: usize, extend: bool) {
        let target = self.clamp(target);
        if extend {
            self.focus = target;
        } else {
            self.anchor = target;
            self.focus = target;
        }
    }

    pub(crate) fn move_vertical(&mut self, target: usize, extend: bool) {
        self.move_to(target, extend);
    }

    pub(crate) fn previous_grapheme(&self, byte: usize) -> usize {
        let byte = self.clamp(byte);
        self.text[..byte]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    pub(crate) fn next_grapheme(&self, byte: usize) -> usize {
        let byte = self.clamp(byte);
        self.text[byte..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(self.text.len(), |(offset, _)| byte + offset)
    }

    fn previous_word(&self, byte: usize) -> usize {
        let mut index = self.clamp(byte);
        while index > 0 {
            let previous = self.previous_grapheme(index);
            let grapheme = &self.text[previous..index];
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index = previous;
        }
        while index > 0 {
            let previous = self.previous_grapheme(index);
            let grapheme = &self.text[previous..index];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index = previous;
        }
        index
    }

    fn next_word(&self, byte: usize) -> usize {
        let mut index = self.clamp(byte);
        while index < self.text.len() {
            let next = self.next_grapheme(index);
            let grapheme = &self.text[index..next];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index = next;
        }
        while index < self.text.len() {
            let next = self.next_grapheme(index);
            let grapheme = &self.text[index..next];
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index = next;
        }
        index
    }

    /// Replaces the selection with `input`.
    pub(crate) fn insert(&mut self, input: &str) -> bool {
        let input = normalize(self.multiline, input);
        if input.is_empty() && !self.has_selection() {
            return false;
        }
        let selection = self.selection();
        let caret = selection.start + input.len();
        self.push_undo(Edit {
            range: selection.clone(),
            removed: self.text[selection.clone()].to_owned(),
            inserted: input.clone(),
            anchor: self.anchor,
            focus: self.focus,
            caret,
        });
        self.text.replace_range(selection, &input);
        self.anchor = caret;
        self.focus = caret;
        self.clear_redo();
        true
    }

    /// Deletes the selection, or the grapheme before the caret.
    pub(crate) fn backspace(&mut self, by_word: bool) -> bool {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.delete_selection();
        }
        if self.focus == 0 {
            return false;
        }
        let start = if by_word {
            self.previous_word(self.focus)
        } else {
            self.previous_grapheme(self.focus)
        };
        self.replace_range(start..self.focus)
    }

    /// Deletes the selection, or the grapheme after the caret.
    pub(crate) fn delete(&mut self, by_word: bool) -> bool {
        let selection = self.selection();
        if !selection.is_empty() {
            return self.delete_selection();
        }
        if self.focus >= self.text.len() {
            return false;
        }
        let end = if by_word {
            self.next_word(self.focus)
        } else {
            self.next_grapheme(self.focus)
        };
        self.replace_range(self.focus..end)
    }

    pub(crate) fn delete_selection(&mut self) -> bool {
        let selection = self.selection();
        if selection.is_empty() {
            return false;
        }
        self.replace_range(selection)
    }

    fn replace_range(&mut self, range: Range<usize>) -> bool {
        self.push_undo(Edit {
            range: range.clone(),
            removed: self.text[range.clone()].to_owned(),
            inserted: String::new(),
            anchor: self.anchor,
            focus: self.focus,
            caret: range.start,
        });
        self.text.replace_range(range.clone(), "");
        self.anchor = range.start;
        self.focus = range.start;
        self.clear_redo();
        true
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.undo_bytes = self.undo_bytes.saturating_sub(edit.bytes());
        let inserted = edit.range.start..edit.range.start + edit.inserted.len();
        self.text.replace_range(inserted, &edit.removed);
        self.anchor = edit.anchor.min(self.text.len());
        self.focus = edit.focus.min(self.text.len());
        self.push_redo(edit);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.redo_bytes = self.redo_bytes.saturating_sub(edit.bytes());
        self.text.replace_range(edit.range.clone(), &edit.inserted);
        self.anchor = edit.caret.min(self.text.len());
        self.focus = self.anchor;
        self.push_undo(edit);
        true
    }

    fn push_undo(&mut self, edit: Edit) {
        self.undo_bytes = self.undo_bytes.saturating_add(edit.bytes());
        self.undo.push(edit);
        Self::trim(&mut self.undo, &mut self.undo_bytes);
    }

    fn push_redo(&mut self, edit: Edit) {
        self.redo_bytes = self.redo_bytes.saturating_add(edit.bytes());
        self.redo.push(edit);
        Self::trim(&mut self.redo, &mut self.redo_bytes);
    }

    fn clear_redo(&mut self) {
        self.redo.clear();
        self.redo_bytes = 0;
    }

    /// Evicts the oldest edits past the entry cap or byte budget, always
    /// keeping the most recent edit so a single large paste stays undoable.
    fn trim(stack: &mut Vec<Edit>, bytes: &mut usize) {
        while stack.len() > UNDO_LIMIT || (*bytes > UNDO_BYTE_BUDGET && stack.len() > 1) {
            let evicted = stack.remove(0);
            *bytes = bytes.saturating_sub(evicted.bytes());
        }
    }

    fn clamp(&self, byte: usize) -> usize {
        let mut byte = byte.min(self.text.len());
        while byte > 0 && !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }
}

pub(crate) fn normalize(multiline: bool, text: &str) -> String {
    if multiline {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.chars()
            .filter(|character| !matches!(character, '\r' | '\n'))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{TextEditor, UNDO_BYTE_BUDGET, UNDO_LIMIT};

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let mut editor = TextEditor::with_text(true, "hello world");
        editor.select(0, 5);
        assert!(editor.insert("goodbye"));
        assert_eq!(editor.text(), "goodbye world");
        assert_eq!(editor.focus(), 7);

        assert!(editor.undo());
        assert_eq!(editor.text(), "hello world");
        assert_eq!(editor.selection(), 0..5);

        assert!(editor.redo());
        assert_eq!(editor.text(), "goodbye world");
        assert_eq!(editor.focus(), 7);
        assert!(!editor.redo());
    }

    #[test]
    fn undo_restores_deleted_graphemes() {
        let mut editor = TextEditor::with_text(false, "ab");
        editor.set_caret(2);
        assert!(editor.backspace(false));
        assert_eq!(editor.text(), "a");
        assert!(editor.undo());
        assert_eq!(editor.text(), "ab");
        assert_eq!(editor.focus(), 2);
    }

    #[test]
    fn typing_history_stays_within_the_entry_cap() {
        let mut editor = TextEditor::with_text(true, String::new());
        for _ in 0..(UNDO_LIMIT + 64) {
            assert!(editor.insert("x"));
        }
        let mut undos = 0;
        while editor.undo() {
            undos += 1;
        }
        assert_eq!(undos, UNDO_LIMIT);
        assert_eq!(editor.text().len(), 64);
    }

    #[test]
    fn large_edits_evict_older_history_under_the_byte_budget() {
        let mut editor = TextEditor::with_text(true, String::new());
        let chunk = "x".repeat(UNDO_BYTE_BUDGET / 2);
        let inserts = 6;
        for _ in 0..inserts {
            assert!(editor.insert(&chunk));
        }
        let mut undos = 0;
        while editor.undo() {
            undos += 1;
        }
        assert!(undos < inserts, "oldest edits must be evicted");
        assert!(undos >= 1, "the newest edit stays undoable");
        for _ in 0..undos {
            assert!(editor.redo());
        }
        assert_eq!(editor.text().len(), inserts * chunk.len());
    }

    #[test]
    fn a_single_edit_larger_than_the_budget_stays_undoable() {
        let mut editor = TextEditor::with_text(true, String::new());
        let huge = "y".repeat(UNDO_BYTE_BUDGET + 1024);
        assert!(editor.insert(&huge));
        assert!(editor.undo());
        assert_eq!(editor.text(), "");
        assert!(editor.redo());
        assert_eq!(editor.text().len(), huge.len());
    }
}
