// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The calendars and the address books of a server that speaks CalDAV (RFC 4791) and CardDAV
//! (RFC 6352) — Nextcloud, ownCloud, Radicale, Baïkal, a Synology — for an account of that kind
//! ([`mailcraft_model::AccountKind::Dav`]), both ways.
//!
//! One job ([`DavJob`], [`run`]) is what the calendar's own job asks of Microsoft Graph for a
//! Microsoft account, in the same shape ([`DavOutcome`]): tell the server what was made, changed
//! or deleted here (`PUT` with `If-None-Match: *` for a new event, `GET`, the change and `PUT`
//! with `If-Match` for one the server has, `DELETE` with `If-Match` for one that goes), then
//! bring down the calendars, the events of a stretch of each, and the contacts of every address
//! book. The collections are found from the address the owner gave each time (`.well-known`,
//! the principal, the calendar and address book homes: RFC 6764), so that a calendar made on
//! the server since turns up by itself.
//!
//! Basic authentication, inside TLS only (`http::Url` takes nothing else but this machine). The
//! password goes in the `Authorization` header and nowhere else; it is scrubbed from every
//! error. The server's answers are as hostile as any: XML is read as a stream that takes no DTD,
//! with every field, the number of items and the depth capped, and nothing in them is followed
//! off the server the owner named.
//!
//! An event comes down as its series (with its rule) and the occurrences of the stretch, as a
//! Microsoft account's do, so that the engine merges them the same way; a changed instance of a
//! series (`RECURRENCE-ID`) is an exception of it.

mod events;
mod read;
pub mod standin;
mod write;

use std::time::Duration;

use mailcraft_model::remote::{CalendarsDownload, ContactsDownload, EventsDownload, EventsUpload, EventsUploaded, RemoteCalendar, RemoteContact};
use mailcraft_model::{Account, DavServer, Rgb, Rights};
use mailcraft_time::Timestamp;

use crate::NetError;
use crate::http::{self, Request, Url};

pub(crate) use read::multistatus;

/// How long one request may take.
const PATIENCE: Duration = Duration::from_secs(60);
/// Most collections of one kind followed on one server, most items read of a collection.
const MAX_COLLECTIONS: usize = 200;
pub(crate) const MAX_ITEMS: usize = 5_000;
/// Most calendars asked for in one job (those the window asks for first).
const MAX_ASKED: usize = 50;
/// How many redirects are followed finding the principal.
const MAX_HOPS: usize = 5;

/// What the host is asked to do with a calendar and contacts server for one account, as the
/// calendar's own job asks it.
#[derive(Clone)]
pub struct DavJob {
    pub account: Account,
    /// The account's password (usually an app password). Never written anywhere.
    pub password: String,
    /// Also bring every calendar's list and the contacts of every address book down
    /// (the wide stretch: a month back, a year ahead).
    pub directory: bool,
    /// The stretch of the calendars to bring down (Unix seconds, from and up to).
    pub calendar: Option<(Timestamp, Timestamp)>,
    /// What was made, changed or deleted in the calendar here, for the server to be told
    /// before the calendars are brought down.
    pub changes: Option<EventsUpload>,
    /// The calendars to bring down, by what the server calls them (the account's main calendar
    /// is the one with no name); empty for all of them.
    pub asks: Vec<String>,
}

impl std::fmt::Debug for DavJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavJob").field("account", &self.account.id).field("directory", &self.directory).field("calendar", &self.calendar).finish()
    }
}

/// What a [`DavJob`] came to. Never an error as a whole: what could be had is here, and what
/// couldn't is said in `errors`, one line each, with nothing of the owner's in it.
#[derive(Clone, Debug, Default)]
pub struct DavOutcome {
    pub account: mailcraft_model::Id,
    /// The server refused the user name or the password: the owner has to give it again.
    pub sign_in_refused: bool,
    /// The account's calendars, when they were listed.
    pub calendars: Option<CalendarsDownload>,
    /// The stretch of each calendar that was asked for.
    pub events: Vec<EventsDownload>,
    /// Every contact of every address book, when they were asked for.
    pub contacts: Option<ContactsDownload>,
    /// What the server said to the changes it was told.
    pub uploaded: Option<EventsUploaded>,
    pub errors: Vec<String>,
}

/// A calendar or an address book on the server, as it was found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Collection {
    /// Its path on the server, with the server's own name for it (`/remote.php/dav/calendars/ada/personal/`).
    pub(crate) href: String,
    pub(crate) name: String,
    pub(crate) color: Option<Rgb>,
    /// Whether the owner may write to it, as the server says (`current-user-privilege-set`);
    /// a server that doesn't say is taken to allow it, and its refusal is what says no.
    pub(crate) rights: Rights,
    /// For a calendar: it takes events (`VEVENT`); one that takes only to-dos is passed over.
    pub(crate) events: bool,
    /// The server's mark of the collection as a whole (`getctag`), when it gave one.
    pub(crate) ctag: String,
}

/// Where the collections of a server are: the calendars and the address books of the owner.
#[derive(Clone, Debug, Default)]
pub(crate) struct Home {
    pub(crate) calendars: Vec<Collection>,
    pub(crate) addressbooks: Vec<Collection>,
}

impl Home {
    /// The calendar that stands for "the account's calendar" here: the one the server calls
    /// `personal` (Nextcloud's own name for the first), else the first by name. Stable across
    /// jobs for as long as the server has it.
    pub(crate) fn main(&self) -> Option<&Collection> {
        let events: Vec<&Collection> = self.calendars.iter().filter(|c| c.events).collect();
        events
            .iter()
            .find(|c| c.href.trim_end_matches('/').rsplit('/').next().is_some_and(|last| last.eq_ignore_ascii_case("personal")))
            .or_else(|| events.first())
            .copied()
    }

    /// The collection the engine calls `name` (empty: the main one).
    pub(crate) fn calendar(&self, name: &str) -> Option<&Collection> {
        if name.is_empty() { self.main() } else { self.calendars.iter().find(|c| c.href == name) }
    }
}

/// The connection: where the server is and who signs in, with the password kept apart.
pub(crate) struct Link<'a> {
    base: Url,
    login: String,
    password: &'a str,
}

impl<'a> Link<'a> {
    pub(crate) fn new(server: &DavServer, email: &str, password: &'a str) -> Result<Link<'a>, NetError> {
        let base = Url::parse(server.url.trim().trim_end_matches('/'))?;
        Ok(Link { base, login: server.login(email).to_string(), password })
    }

    /// A request to `target` (a path on the server, or an address on it) and its answer, read
    /// whatever its status. An address on another server is refused: nothing of the owner's
    /// goes anywhere but where the owner pointed.
    pub(crate) fn call(&self, method: &str, target: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<http::Reply, NetError> {
        let url = self.url_of(target)?;
        let auth = format!("Basic {}", mailcraft_charset::base64::STANDARD.encode(format!("{}:{}", self.login, self.password)));
        let mut all: Vec<(&str, &str)> = vec![("Authorization", auth.as_str())];
        all.extend_from_slice(headers);
        http::request(&url, &Request { method, headers: &all, body, patience: PATIENCE, max_body: http::MAX_LARGE_BODY }).map_err(|e| self.without(e))
    }

    /// The address of `target` on the server: a path, or an address that is on the same server.
    pub(crate) fn url_of(&self, target: &str) -> Result<Url, NetError> {
        let target = target.trim();
        if target.starts_with('/') {
            return Ok(Url { path: target.to_string(), ..self.base.clone() });
        }
        let url = Url::parse_within(target, http::MAX_LONG_URL)?;
        if !url.same_server(&self.base) {
            return Err(NetError::Config("the server pointed to another server, which is not followed".into()));
        }
        Ok(url)
    }

    /// `error` with the password taken out of it.
    fn without(&self, error: NetError) -> NetError {
        error.without(self.password)
    }
}

/// The path of the server's own root, where `.well-known` lives.
const WELL_KNOWN_CALDAV: &str = "/.well-known/caldav";
const WELL_KNOWN_CARDDAV: &str = "/.well-known/carddav";

/// What the server said to a request that was refused, as an error.
pub(crate) fn refusal(doing: &str, reply: &http::Reply) -> NetError {
    match reply.status {
        401 => NetError::Config(format!("{doing}: the server refused the user name or the password (most servers want an app password)")),
        403 => NetError::Forbidden(format!("{doing}: the server doesn't allow it")),
        404 => NetError::Config(format!("{doing}: the server has nothing there")),
        412 => NetError::Config(format!("{doing}: the server's copy has changed since")),
        423 => NetError::Config(format!("{doing}: the server has it locked")),
        429 | 503 => NetError::Busy { text: format!("{doing}: the server asked to try later"), retry_after: None },
        status => NetError::Config(format!("{doing}: the server answered {status}")),
    }
}

/// Check that the server takes the sign-in: the owner's principal is asked for.
pub fn check(server: &DavServer, email: &str, password: &str) -> Result<(), NetError> {
    let link = Link::new(server, email, password)?;
    principal(&link).map(|_| ())
}

/// The path of the owner's principal: asked of the address the owner gave, then of
/// `.well-known/caldav` and `.well-known/carddav` on the server's root, each followed through
/// the redirects it answers with (on this server only).
pub(crate) fn principal(link: &Link<'_>) -> Result<String, NetError> {
    let starts = [link.base.path.clone(), WELL_KNOWN_CALDAV.to_string(), WELL_KNOWN_CARDDAV.to_string()];
    let mut last = NetError::Config("the server names no principal".into());
    for start in starts {
        match principal_from(link, &start) {
            Ok(path) => return Ok(path),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn principal_from(link: &Link<'_>, start: &str) -> Result<String, NetError> {
    let mut at = start.to_string();
    let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
    for _ in 0..MAX_HOPS {
        let reply = link.call("PROPFIND", &at, &[("Depth", "0"), ("Content-Type", "application/xml; charset=utf-8")], Some(body))?;
        match reply.status {
            207 => {
                let responses = multistatus(&String::from_utf8_lossy(&reply.body))
                    .ok_or_else(|| NetError::Config("the server's answer couldn't be read".into()))?;
                let principal = responses.iter().find_map(|r| r.prop("current-user-principal").and_then(|p| p.hrefs.first().cloned()));
                return principal.filter(|p| !p.is_empty()).ok_or_else(|| NetError::Config("the server names no principal".into()));
            }
            301 | 302 | 307 | 308 => {
                let Some(next) = reply.location.as_deref() else { return Err(NetError::Config("the server pointed nowhere".into())) };
                at = link.url_of(next)?.path;
            }
            _ => return Err(refusal("finding the principal", &reply)),
        }
    }
    Err(NetError::Config("the server kept pointing elsewhere".into()))
}

/// The owner's calendars and address books: the homes from the principal, then what each
/// holds (`PROPFIND` with `Depth: 1`).
pub(crate) fn discover(link: &Link<'_>) -> Result<Home, NetError> {
    let principal = principal(link)?;
    let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:prop><c:calendar-home-set/><card:addressbook-home-set/></d:prop></d:propfind>"#;
    let reply = link.call("PROPFIND", &principal, &[("Depth", "0"), ("Content-Type", "application/xml; charset=utf-8")], Some(body))?;
    if reply.status != 207 {
        return Err(refusal("reading the principal", &reply));
    }
    let responses =
        multistatus(&String::from_utf8_lossy(&reply.body)).ok_or_else(|| NetError::Config("the server's answer couldn't be read".into()))?;
    let home_of = |name: &str| responses.iter().find_map(|r| r.prop(name).and_then(|p| p.hrefs.first().cloned())).filter(|h| !h.is_empty());
    let mut home = Home::default();
    if let Some(calendars) = home_of("calendar-home-set") {
        home.calendars = collections(link, &calendars, true)?;
    }
    if let Some(books) = home_of("addressbook-home-set") {
        home.addressbooks = collections(link, &books, false)?;
    }
    Ok(home)
}

/// The collections under `home` that are calendars (`calendars`) or address books.
fn collections(link: &Link<'_>, home: &str, calendars: bool) -> Result<Vec<Collection>, NetError> {
    let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:a="http://apple.com/ns/ical/"><d:prop><d:resourcetype/><d:displayname/><d:current-user-privilege-set/><cs:getctag/><a:calendar-color/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;
    let home_path = link.url_of(home)?.path;
    let reply = link.call("PROPFIND", &home_path, &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")], Some(body))?;
    if reply.status != 207 {
        return Err(refusal(if calendars { "listing the calendars" } else { "listing the address books" }, &reply));
    }
    let responses =
        multistatus(&String::from_utf8_lossy(&reply.body)).ok_or_else(|| NetError::Config("the server's answer couldn't be read".into()))?;
    let wanted = if calendars { "calendar" } else { "addressbook" };
    let mut out = Vec::new();
    for r in responses.iter().take(MAX_COLLECTIONS.saturating_mul(4)) {
        let Some(kind) = r.prop("resourcetype") else { continue };
        if !kind.children.iter().any(|c| c == wanted) {
            continue;
        }
        let href = link.url_of(&r.href).map(|u| u.path).unwrap_or_default();
        if href.is_empty() || href.trim_end_matches('/') == home_path.trim_end_matches('/') {
            continue;
        }
        let name = r
            .prop("displayname")
            .map(|p| http::tidy(&p.text))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| href.trim_end_matches('/').rsplit('/').next().map(|last| http::decode(last)).unwrap_or_default());
        let color = r.prop("calendar-color").and_then(|p| Rgb::parse(p.text.trim().get(..7).unwrap_or(p.text.trim())));
        // A privilege set that is given and lacks `write` (or `write-content`, `all`) is read-only.
        let rights = match r.prop("current-user-privilege-set") {
            Some(p) if !p.children.is_empty() => {
                Rights::from_read_only(!p.children.iter().any(|c| matches!(c.as_str(), "write" | "write-content" | "all")))
            }
            _ => Rights::Edit,
        };
        let events = match r.prop("supported-calendar-component-set") {
            Some(p) if !p.components.is_empty() => p.components.iter().any(|c| c.eq_ignore_ascii_case("VEVENT")),
            _ => true,
        };
        let ctag = r.prop("getctag").map(|p| p.text.trim().trim_matches('"').to_string()).unwrap_or_default();
        out.push(Collection { href, name, color, rights, events, ctag });
        if out.len() >= MAX_COLLECTIONS {
            break;
        }
    }
    out.sort_by(|a, b| a.href.cmp(&b.href));
    Ok(out)
}

/// The account's calendars as the engine lists them: every calendar that takes events, with
/// the one that stands for the account's own marked as such.
pub(crate) fn calendar_list(home: &Home) -> CalendarsDownload {
    let main = home.main().map(|c| c.href.clone()).unwrap_or_default();
    let calendars = home
        .calendars
        .iter()
        .filter(|c| c.events)
        .map(|c| RemoteCalendar {
            id: c.href.clone(),
            name: c.name.clone(),
            color: c.color,
            group: String::new(),
            rights: c.rights,
            owner: None,
            main: c.href == main,
            online_providers: Some(Vec::new()),
        })
        .collect();
    CalendarsDownload { complete: true, calendars, ..Default::default() }
}

/// Do the job: what waits goes up first, then the calendars, their stretch and the contacts
/// come down. `offset_at` is the clock of the calendar the events are shown in; `now` is for
/// the wide stretch.
pub fn run(job: &DavJob, now: Timestamp, offset_at: &dyn Fn(Timestamp) -> i32) -> DavOutcome {
    let mut out = DavOutcome { account: job.account.id, ..Default::default() };
    let Some(server) = job.account.dav.as_ref() else {
        out.errors.push("the account has no calendar and contacts server".into());
        return out;
    };
    if let Some(why) = server.problem() {
        out.errors.push(format!("the server's address {why}"));
        return out;
    }
    let link = match Link::new(server, &job.account.email, &job.password) {
        Ok(link) => link,
        Err(e) => {
            out.errors.push(e.to_string());
            return out;
        }
    };
    let home = match discover(&link) {
        Ok(home) => home,
        Err(e) => {
            out.sign_in_refused = matches!(&e, NetError::Config(text) if text.contains("refused the user name"));
            out.errors.push(e.to_string());
            if let Some(changes) = &job.changes {
                // Nothing was sent: every change is still to be made, and says so.
                out.uploaded = Some(write::untried(changes, &e.to_string()));
            }
            return out;
        }
    };
    if let Some(changes) = &job.changes {
        out.uploaded = Some(write::upload(&link, &home, changes, offset_at));
    }
    if job.directory {
        out.calendars = Some(calendar_list(&home).for_account(&job.account));
    }
    // The stretch: the one asked for, or the wide one that comes with the directory.
    let stretch = job.calendar.or_else(|| job.directory.then(|| (now - events::WIDE_BEHIND, now + events::WIDE_AHEAD)));
    if let Some((from, to)) = stretch.filter(|(from, to)| from < to) {
        let asked: Vec<&Collection> = if job.asks.is_empty() {
            home.calendars.iter().filter(|c| c.events).collect()
        } else {
            job.asks.iter().take(MAX_ASKED).filter_map(|name| home.calendar(name)).collect()
        };
        let main = home.main().map(|c| c.href.clone()).unwrap_or_default();
        for calendar in asked {
            match events::download(&link, calendar, from, to, &job.account.email, offset_at) {
                Ok(mut download) => {
                    download.calendar = if calendar.href == main { String::new() } else { calendar.href.clone() };
                    out.events.push(download.for_account(&job.account));
                }
                Err(e) => out.errors.push(format!("{}: {e}", calendar.name)),
            }
        }
    }
    if job.directory {
        let mut contacts = Vec::new();
        let mut complete = true;
        for book in &home.addressbooks {
            match read::contacts(&link, book) {
                Ok(found) => contacts.extend(found),
                Err(e) => {
                    complete = false;
                    out.errors.push(format!("{}: {e}", book.name));
                }
            }
        }
        contacts.truncate(MAX_ITEMS);
        out.contacts = Some(ContactsDownload { complete, contacts, ..Default::default() }.for_account(&job.account));
    }
    out
}

/// A contact as the server has it, from its vCard text.
pub(crate) fn contact_of(href: &str, etag: &str, card: &mailcraft_model::Contact) -> RemoteContact {
    let mut r = RemoteContact {
        id: href.to_string(),
        change_key: etag.to_string(),
        display_name: card.display_name.clone(),
        first: card.first.clone(),
        middle: card.middle.clone(),
        last: card.last.clone(),
        nickname: card.nickname.clone(),
        company: card.company.clone(),
        title: card.title.clone(),
        department: card.department.clone(),
        emails: card.emails.clone(),
        phones: card.phones.clone(),
        addresses: card.addresses.clone(),
        websites: card.websites.clone(),
        birthday: card.birthday,
        notes: card.notes.clone(),
        modified: 0,
    };
    r.tidy();
    r
}

#[cfg(test)]
mod tests;
