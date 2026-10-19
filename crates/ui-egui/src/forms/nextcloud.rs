//! Nextcloud (CalDAV/CardDAV): where the calendar and the contacts read with Send/Receive are.

use egui::Ui;
use mailcraft_engine::Session;
use serde_json::{Value, json};

use super::kit::{body_over, error, error_room, field_id, footer, row};
use crate::MailApp;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NextcloudForm {
    account: u64,
    url: String,
    username: String,
    password: String,
    calendar: String,
    addressbook: String,
    error: String,
}

impl NextcloudForm {
    pub fn new(s: &Session) -> NextcloudForm {
        let account = s.mailbox.default_account();
        let dav = account.and_then(|a| a.dav.clone()).unwrap_or_default();
        NextcloudForm {
            account: account.map(|a| a.id).unwrap_or(0),
            url: dav.url,
            username: dav.username,
            calendar: dav.calendar,
            addressbook: dav.addressbook,
            ..Default::default()
        }
    }

    /// The command OK runs.
    pub fn command(&self) -> (&'static str, Value) {
        (
            "account.nextcloud",
            json!({"account": self.account, "url": self.url.trim(), "username": self.username.trim(), "calendar": self.calendar.trim(), "addressbook": self.addressbook.trim()}),
        )
    }

    /// Lay out the form. Returns `true` when the dialog should close.
    pub fn show(&mut self, app: &mut MailApp, ui: &mut Ui) -> bool {
        body_over(ui, error_room(ui, &self.error), |ui| {
            ui.label(tl!("The calendar and the contacts come down with each Send/Receive, for reading. Leave the address empty to stop."));
            ui.add_space(6.0);
            let fields: [(&str, &mut String, &str, &str); 4] = [
                (tl!("Address"), &mut self.url, "url", "https://cloud.example.com"),
                (tl!("User name"), &mut self.username, "user", ""),
                (tl!("Calendar"), &mut self.calendar, "calendar", "/remote.php/dav/calendars/<user>/personal/"),
                (tl!("Contacts"), &mut self.addressbook, "addressbook", "/remote.php/dav/addressbooks/users/<user>/contacts/"),
            ];
            for (label, value, key, hint) in fields {
                row(ui, label, |ui| {
                    ui.add(egui::TextEdit::singleline(value).id(field_id(("nextcloud", key))).hint_text(hint).desired_width(f32::INFINITY));
                });
            }
            row(ui, tl!("Password"), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .id(field_id(("nextcloud", "password")))
                        .password(true)
                        .hint_text(tl!("An app password (Settings › Security)"))
                        .desired_width(f32::INFINITY),
                );
            });
            error(ui, &self.error);
        });
        let f = footer(ui, tl!("OK"), true, false);
        if f.cancel {
            return true;
        }
        if f.ok {
            let (id, params) = self.command();
            match app.run(id, params) {
                Ok(_) => {
                    app.adopt_dav_password(self.account, std::mem::take(&mut self.password));
                    if !self.url.trim().is_empty() {
                        let _ = app.run("ui.sync", json!({"account": self.account}));
                    }
                    return true;
                }
                Err(e) => self.error = e.to_string(),
            }
        }
        false
    }
}
