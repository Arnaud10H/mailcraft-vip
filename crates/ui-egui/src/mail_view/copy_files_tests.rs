// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Ctrl+C on the message list: the selected messages go on the clipboard as .eml files.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use egui::Event;
use serde_json::json;

use crate::RichClipboard;
use crate::people_view::testing;

#[derive(Clone, Default)]
struct Board(Rc<RefCell<Vec<Vec<PathBuf>>>>);

impl RichClipboard for Board {
    fn put(&self, _html: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn html(&self) -> Option<String> {
        None
    }
    fn files(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.0.borrow_mut().push(paths.to_vec());
        Ok(())
    }
}

#[test]
fn ctrl_c_on_the_list_puts_the_selected_messages_on_the_clipboard_as_files() {
    let mut w = testing::window("view.mail");
    w.state_mut().execute("ui.dismissReminder", json!({"all": true})).unwrap();
    let board = Board::default();
    w.state_mut().services.clipboard = Some(Box::new(board.clone()));

    // Nothing selected: nothing copied.
    w.state_mut().session.sel.messages.clear();
    w.input_mut().events.push(Event::Copy);
    w.run_steps(2);
    assert!(board.0.borrow().is_empty());

    let ids: Vec<_> = w.state_mut().session.rows().iter().take(3).map(|r| r.id).collect();
    w.state_mut().session.sel.messages = ids;
    w.input_mut().events.push(Event::Copy);
    w.run_steps(2);
    let copies = board.0.borrow();
    let [files] = copies.as_slice() else { panic!("one copy: {copies:?}") };
    assert_eq!(files.len(), 3, "{files:?}");
    for f in files {
        assert!(f.extension().is_some_and(|e| e == "eml") && std::fs::read_to_string(f).unwrap().contains("Subject:"), "{f:?}");
    }
    let _ = std::fs::remove_dir_all(files[0].parent().unwrap());
}
