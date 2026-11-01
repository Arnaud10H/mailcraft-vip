// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The whole way of a calendar change for an account whose calendars and contacts a CalDAV/
//! CardDAV server keeps: the engine, the window's calendar timer, this host's job and
//! `mailcraft-net`, against a stand-in server on a loopback port
//! (`mailcraft_net::dav::standin`). Nothing here leaves this machine, and nobody is signed in.

use mailcraft_engine::Session;
use mailcraft_model::{Event, Id, Mailbox};
use mailcraft_net::dav::standin::{PASSWORD, StandIn};
use mailcraft_time::{Date, DateTime};
use mailcraft_ui_egui::{DavRequest, MailApp, Services};
use serde_json::{Value, json};

/// Wednesday 2026-10-14, 12:00 UTC; the computer's clock is UTC.
const NOW: i64 = 1_791_979_200;
const SECOND: f64 = 1_000.0;

/// An instant, by the clock in UTC.
fn utc(month: u8, day: u8, hour: u8, minute: u8) -> i64 {
    DateTime::new(Date::new(2026, month, day).unwrap(), hour, minute, 0).unwrap().to_timestamp(0)
}

struct Flow {
    app: MailApp,
    server: StandIn,
    account: Id,
    at: f64,
}

impl Flow {
    /// The app with a calendar and contacts account on the stand-in, the calendar in front, and
    /// "Budget review" on the server, Monday the 12th at 13:00 UTC, brought down by the first look.
    fn start() -> Flow {
        let server = StandIn::start().unwrap();
        server.state().put(
            "personal",
            "budget.ics",
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:budget@example.org\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261012T130000Z\r\nDTEND:20261012T140000Z\r\nSUMMARY:Budget review\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        server.state().put(
            "contacts",
            "robin.vcf",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:robin\r\nFN:Robin Example\r\nN:Example;Robin;;;\r\nEMAIL:robin@cloud.example\r\nEND:VCARD\r\n",
        );
        let mut app = MailApp::new(Session::at(Mailbox::with_local_account("", ""), NOW, 0), Services::default());
        app.autosave = false;
        app.services.dav = Some(Box::new(|request: DavRequest| {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = tx.send(super::dav_in(request, &|_| 0));
            rx
        }));
        let added = app.execute("account.add", json!({"email": "ada@example.org", "kind": "dav", "dav": {"url": server.url(), "username": "ada"}}));
        let account = added.unwrap()["id"].as_u64().unwrap();
        // The password, as the Send/Receive dialog gives it; the calendar is asked for at once.
        let started = app.execute("ui.sync", json!({"account": account, "password": PASSWORD})).unwrap();
        assert_eq!(started["started"], true);
        app.execute("view.calendar", json!({})).unwrap();
        let mut flow = Flow { app, server, account, at: mailcraft_ui_egui::now_ms() };
        assert!(flow.look(), "the calendar is asked for to begin with");
        flow.take_in();
        flow
    }

    /// Look at the calendar half a minute on: a job starts when one is due. Whether one did.
    fn look(&mut self) -> bool {
        self.at += 31.0 * SECOND;
        self.app.calendar_tick_at(true, self.at)
    }

    fn take_in(&mut self) {
        for _ in 0..4 {
            self.app.poll_calendar();
        }
    }

    fn sync(&mut self) {
        self.look();
        self.take_in();
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.app.execute(id, params.clone()).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
    }

    fn all_here(&self, title: &str) -> Vec<Event> {
        let mut events: Vec<Event> = self.app.session.mailbox.events.values().filter(|e| e.title == title).map(|e| e.as_ref().clone()).collect();
        events.sort_by_key(|e| (e.start, e.id));
        events
    }

    fn here(&self, title: &str) -> Event {
        self.all_here(title).into_iter().next().unwrap_or_else(|| panic!("no event \"{title}\" here"))
    }

    /// The text of the resource on the server that holds the event called `title` here.
    fn there(&self, title: &str) -> Option<String> {
        let here = self.here(title);
        self.server.state().text(&here.origin.id).map(str::to_string)
    }

    fn settled(&self) -> bool {
        let mailbox = &self.app.session.mailbox;
        mailbox.event_changes.is_empty() && mailbox.event_checks.is_empty() && !self.app.has_sync_problems()
    }
}

#[test]
fn the_calendars_and_contacts_of_a_caldav_server_come_down_and_an_appointment_goes_up_and_away() {
    let mut flow = Flow::start();
    // The server's calendar is the account's, with its name and colour; its event is here.
    let s = &flow.app.session;
    let own = s.mailbox.calendars.iter().find(|c| c.origin.account == flow.account && c.origin.id.is_empty()).cloned().unwrap();
    assert_eq!((own.name.as_str(), own.rights.can_edit(), own.visible), ("Personal", true, true));
    let review = flow.here("Budget review");
    assert_eq!(
        (review.calendar, review.origin.id.as_str(), review.origin.change_key.as_str()),
        (own.id, "/remote.php/dav/calendars/ada/personal/budget.ics", "\"e1\"".trim_matches('"'))
    );
    assert_eq!(review.start, DateTime::from_timestamp(utc(10, 12, 13, 0), 0));
    assert!(s.mailbox.contacts.values().any(|c| c.display_name == "Robin Example" && c.origin.account == flow.account));
    assert!(flow.settled());

    // Made here: on the server at once, as a resource of its own, and here once, under the
    // server's name for it.
    let lunch = flow.run("calendar.new", json!({"title": "Lunch", "start": "2026-10-15T12:00", "location": "Canteen"}))["id"].as_u64().unwrap();
    assert_eq!(flow.app.session.mailbox.event_changes.len(), 1);
    flow.sync();
    let here = flow.here("Lunch");
    assert_eq!((here.id, here.origin.account, flow.all_here("Lunch").len()), (lunch, flow.account, 1));
    assert!(here.origin.id.starts_with("/remote.php/dav/calendars/ada/personal/") && here.origin.id.ends_with(".ics"), "{}", here.origin.id);
    let text = flow.there("Lunch").unwrap();
    assert!(text.contains("SUMMARY:Lunch") && text.contains("LOCATION:Canteen") && text.contains("DTSTART:20261015T120000Z"), "{text}");
    assert!(flow.settled(), "{:?}", flow.app.session.mailbox.event_changes);

    // Moved and retitled: the server's copy follows, on the version it had.
    flow.run("calendar.move", json!({"id": lunch, "start": "2026-10-16T09:00", "end": "2026-10-16T10:30"}));
    flow.run("calendar.edit", json!({"id": lunch, "title": "Long lunch"}));
    flow.sync();
    let text = flow.there("Long lunch").unwrap();
    assert!(text.contains("SUMMARY:Long lunch") && text.contains("DTSTART:20261016T090000Z") && text.contains("DTEND:20261016T103000Z"), "{text}");
    assert!(flow.settled(), "{:?}", flow.app.session.mailbox.event_changes);
    {
        let state = flow.server.state();
        let puts = state.asked("PUT");
        assert_eq!(puts.len(), 2, "one to make it, one to change it");
        assert_eq!(puts[0].header("If-None-Match"), "*");
        assert_eq!(puts[1].header("If-Match"), "\"e3\"");
    }

    // Changed on the server meanwhile: what the server has comes down over what is here.
    let href = flow.here("Long lunch").origin.id.clone();
    let moved = flow.server.state().text(&href).unwrap().replace("SUMMARY:Long lunch", "SUMMARY:Lunch with Robin");
    let file = href.rsplit('/').next().unwrap().to_string();
    flow.server.state().put("personal", &file, &moved);
    flow.sync();
    assert_eq!(flow.all_here("Lunch with Robin").len(), 1);

    // Deleted here: gone on the server.
    flow.run("calendar.delete", json!({"id": lunch}));
    flow.sync();
    assert!(flow.server.state().text(&href).is_none() && flow.all_here("Lunch with Robin").is_empty());
    assert!(flow.settled());
    // And what the server no longer has goes from here.
    flow.server.state().calendars[0].items.remove("budget.ics");
    flow.app.execute("ui.sync", json!({"account": flow.account})).unwrap();
    flow.sync();
    assert!(flow.all_here("Budget review").is_empty());
}

#[test]
fn a_refused_password_is_forgotten_and_asked_for_again() {
    let mut flow = Flow::start();
    flow.server.state().refuse_all = Some(401);
    // The next look asks the server, which refuses: the password is dropped, and nothing more is
    // asked until the owner gives one.
    flow.app.execute("ui.sync", json!({"account": flow.account})).unwrap();
    flow.sync();
    assert!(flow.app.status_msg.as_ref().is_some_and(|(text, _)| text.contains("refused the password")), "{:?}", flow.app.status_msg);
    flow.at += 3_600.0 * SECOND;
    assert!(!flow.app.calendar_tick_at(true, flow.at), "no password, no job");
    let asked = flow.app.execute("ui.sync", json!({"account": flow.account})).unwrap();
    assert_eq!(asked["pending"], "password");
    flow.server.state().refuse_all = None;
    let again = flow.app.execute("ui.sync", json!({"account": flow.account, "password": PASSWORD})).unwrap();
    assert_eq!(again["started"], true);
    assert!(flow.look());
    flow.take_in();
    // The password is kept again: the next job starts without asking.
    flow.at += 3_600.0 * SECOND;
    assert!(flow.app.calendar_tick_at(true, flow.at));
    flow.take_in();
    assert_eq!(flow.server.state().asked("PROPFIND").iter().filter(|a| a.status == 401).count(), 1, "the refused password was tried once");
}
