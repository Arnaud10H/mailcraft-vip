//! Automatic Replies: the out-of-office answer, sent once to each sender while it is on.

use egui::Ui;
use mailcraft_engine::Session;
use serde_json::{Value, json};

use super::kit::{body_over, error, error_room, field_id, footer, multiline, row, tall_row};
use crate::MailApp;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoReplyForm {
    enabled: bool,
    subject: String,
    message: String,
    /// `2026-10-20T08:00`, or empty for no limit.
    start: String,
    end: String,
    error: String,
}

impl AutoReplyForm {
    pub fn new(s: &Session) -> AutoReplyForm {
        let r = &s.mailbox.vip.auto_reply;
        let when = |t: Option<i64>| t.map(|t| s.local(t).iso().chars().take(16).collect()).unwrap_or_default();
        AutoReplyForm {
            enabled: r.enabled,
            subject: r.subject.clone(),
            message: r.message.clone(),
            start: when(r.start),
            end: when(r.end),
            error: String::new(),
        }
    }

    /// The command OK runs.
    pub fn command(&self) -> (&'static str, Value) {
        let time = |t: &str| if t.trim().is_empty() { Value::Null } else { json!(t.trim()) };
        (
            "vip.automaticReplies",
            json!({"enabled": self.enabled, "subject": self.subject, "message": self.message, "start": time(&self.start), "end": time(&self.end)}),
        )
    }

    /// Lay out the form. Returns `true` when the dialog should close.
    pub fn show(&mut self, app: &mut MailApp, ui: &mut Ui) -> bool {
        body_over(ui, error_room(ui, &self.error), |ui| {
            ui.checkbox(&mut self.enabled, tl!("Send automatic replies"));
            ui.add_space(6.0);
            for (label, value, key) in [(tl!("From"), &mut self.start, "start"), (tl!("Until"), &mut self.end, "end")] {
                row(ui, label, |ui| {
                    ui.add(egui::TextEdit::singleline(value).id(field_id(("autoReply", key))).hint_text("2026-10-20T08:00").desired_width(180.0));
                });
            }
            row(ui, tl!("Subject"), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.subject)
                        .id(field_id(("autoReply", "subject")))
                        .hint_text(tl!("Re: the subject"))
                        .desired_width(f32::INFINITY),
                );
            });
            tall_row(ui, tl!("Message"), |ui| {
                multiline(ui, ("autoReply", "message"), &mut self.message, 6);
            });
            ui.label(tl!("Each sender is answered once; mailing lists and automatic mail never are."));
            error(ui, &self.error);
        });
        let f = footer(ui, tl!("OK"), true, false);
        if f.cancel {
            return true;
        }
        if f.ok {
            let (id, params) = self.command();
            match app.run(id, params) {
                Ok(_) => return true,
                Err(e) => self.error = e.to_string(),
            }
        }
        false
    }
}
