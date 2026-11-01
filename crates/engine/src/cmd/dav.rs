//! A CalDAV server (Nextcloud) read with an account's Send/Receive: `account.nextcloud` says
//! where it is, the host brings its calendar down with the run
//! ([`mailcraft_model::vip::DavDownload`]), and [`take_in`] merges it like a Microsoft 365
//! account's ([`super::remote`]): what the server no longer has goes, nothing made here is touched.
//! Contacts are not read: MailCraft VIP is a client to follow the integrated branch, not a
//! synchronisation of the address book.
//!
//! For reading: the calendar is read-only here. ponytail: read-only; send changes back
//! (PUT/DELETE with If-Match) when needed.

use mailcraft_model::Id;
use mailcraft_model::remote::{EventsDownload, RemoteEvent};
use mailcraft_model::vip::{Dav, DavDownload};
use serde_json::{Value, json};

use super::{fail, remote};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

/// What the calendar is called here, and what the server's calendar is called in its origin.
const CALENDAR: &str = "Nextcloud";
const REMOTE: &str = "dav";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("account.nextcloud", "Nextcloud Calendar", "File › Info", link).params(
            r#"{"account"?: number, "url": "https://cloud.example.com" ("" unlinks and removes what came from it), "username"?: string, "calendar"?: path} — read with each Send/Receive; the password is asked once and kept in the system keyring"#,
        ),
    ]
}

fn link(s: &mut Session, v: &Value) -> CmdResult {
    let account = match p::id(v, "account") {
        Some(id) => s.mailbox.account(id).map(|a| a.id).ok_or_else(|| fail(format!("no account with id {id}")))?,
        None => s.mailbox.default_account().map(|a| a.id).ok_or_else(|| fail("add an account first"))?,
    };
    let url = p::str(v, "url").unwrap_or("").trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        s.mailbox.account_mut(account)?.dav = None;
        return remote::forget(s, account);
    }
    if !url.starts_with("https://") || url.len() > 500 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(fail("the address must start with https://"));
    }
    let username = p::req_str(v, "username")?.trim().to_string();
    let path = |k: &str| -> Result<String, CmdError> {
        let path = p::str(v, k).unwrap_or("").trim().to_string();
        if path.is_empty() || (path.starts_with('/') && path.len() < 500 && !path.chars().any(|c| c.is_whitespace() || c.is_control())) {
            Ok(path)
        } else {
            Err(fail(format!("`{k}` must be a path on the server, like /remote.php/dav/…")))
        }
    };
    if username.is_empty() || username.len() > 200 || username.chars().any(|c| c.is_control() || c == ':') {
        return Err(fail("`username` is required"));
    }
    let dav = Dav { url, username, calendar: path("calendar")? };
    let out = json!({"account": account, "calendar": dav.calendar_path()});
    s.mailbox.account_mut(account)?.dav = Some(dav);
    Ok(out)
}

/// Take in what a Send/Receive brought down from the account's server.
pub fn take_in(s: &mut Session, account: Id, download: &DavDownload) -> Result<(), CmdError> {
    let Some(owner) = s.mailbox.account(account).cloned() else { return Ok(()) };
    let offset_at = remote::offset_of(s);
    if let Some(items) = &download.events {
        let mut events = Vec::new();
        for item in items {
            for e in mailcraft_ical::parse(&item.data, &offset_at).events {
                let time = remote::server_time(&e, &offset_at);
                events.push(RemoteEvent {
                    // One resource holds a series' occurrences (the server expanded them): told apart by their start.
                    id: format!("{}#{}", item.href, time.start),
                    change_key: item.etag.clone(),
                    uid: e.uid,
                    title: e.title,
                    location: e.location,
                    description: e.description,
                    start: time.start,
                    end: time.end,
                    all_day: time.all_day,
                    start_zone: e.start_zone,
                    end_zone: e.end_zone,
                    organizer: e.organizer,
                    attendees: e.attendees,
                    show_as: e.show_as,
                    private: e.private,
                    cancelled: e.cancelled,
                    reminder_minutes: e.reminder_minutes,
                    categories: e.categories,
                    importance: e.importance,
                    ..Default::default()
                });
            }
        }
        for e in &mut events {
            e.tidy();
        }
        remote::read_only_calendar(&mut s.mailbox, account, REMOTE, CALENDAR)?;
        let events = EventsDownload {
            calendar: REMOTE.into(),
            from: download.from,
            to: download.to,
            complete: true,
            has_categories: true,
            events,
            ..Default::default()
        }
        .for_account(&owner);
        remote::take_in_events(s, &events)?;
    }
    Ok(())
}
