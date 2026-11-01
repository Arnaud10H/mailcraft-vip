// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! A stand-in for a CalDAV/CardDAV server on a loopback port, for tests (nothing here leaves
//! this machine): the principal and the homes of one owner found as Nextcloud lays them out
//! (`/.well-known/caldav` → `/remote.php/dav/`, the principal, the homes), calendars and
//! address books kept in memory, `PROPFIND`, `REPORT`, `GET`, `PUT` and `DELETE` as the
//! standards have them, and the requests it was sent, for a test to look at.
//!
//! It takes the owner `ada` with the password [`PASSWORD`] and nobody else. The `REPORT`s'
//! filters are not read: every resource of the collection is answered (a `time-range` is the
//! client's to apply again, which the engine does), which is what the tests want to see.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};

/// The app password the stand-in takes for `ada`.
pub const PASSWORD: &str = "standin-app-pass-word-4321";
/// Largest request it reads.
const MAX_REQUEST: usize = 5 * 1024 * 1024;
/// Most requests kept for a test to look at.
const MAX_KEPT: usize = 2_000;
const PRINCIPAL: &str = "/remote.php/dav/principals/users/ada/";
const CALENDARS: &str = "/remote.php/dav/calendars/ada/";
const BOOKS: &str = "/remote.php/dav/addressbooks/users/ada/";

/// One request the stand-in was sent.
#[derive(Clone, Debug, Default)]
pub struct Asked {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub status: u16,
}

impl Asked {
    pub fn header(&self, name: &str) -> &str {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
    }
}

/// A calendar or an address book, with its resources (name → (etag, text)).
#[derive(Clone, Debug, Default)]
pub struct Collection {
    pub name: String,
    pub display: String,
    pub color: String,
    pub read_only: bool,
    /// For a calendar: the kinds it takes (`VEVENT`, `VTODO`); empty says nothing.
    pub components: Vec<String>,
    pub items: BTreeMap<String, (String, String)>,
}

/// What the stand-in keeps.
#[derive(Debug, Default)]
pub struct State {
    pub calendars: Vec<Collection>,
    pub addressbooks: Vec<Collection>,
    pub requests: Vec<Asked>,
    /// The next `PUT` and `DELETE` answer this instead (a server that won't).
    pub refuse_writes: Option<u16>,
    /// Every answer is this status (a server that is down).
    pub refuse_all: Option<u16>,
    /// A `PUT` answers without an `ETag` (a server that doesn't say).
    pub mute_etag: bool,
    etags: u64,
}

impl State {
    /// A calendar with nothing in it; `personal` is what stands for the account's own.
    pub fn add_calendar(&mut self, name: &str, display: &str, read_only: bool) {
        self.calendars.push(Collection {
            name: name.into(),
            display: display.into(),
            color: "#1D71B8FF".into(),
            read_only,
            components: vec!["VEVENT".into()],
            ..Default::default()
        });
    }

    pub fn add_addressbook(&mut self, name: &str, display: &str) {
        self.addressbooks.push(Collection { name: name.into(), display: display.into(), ..Default::default() });
    }

    /// A resource put into the calendar `calendar` by hand: its path, and its etag.
    pub fn put(&mut self, calendar: &str, file: &str, text: &str) -> (String, String) {
        self.etags += 1;
        let etag = format!("\"e{}\"", self.etags);
        if let Some(c) = self.calendars.iter_mut().chain(self.addressbooks.iter_mut()).find(|c| c.name == calendar) {
            c.items.insert(file.to_string(), (etag.clone(), text.to_string()));
        }
        let home = if self.calendars.iter().any(|c| c.name == calendar) { CALENDARS } else { BOOKS };
        (format!("{home}{calendar}/{file}"), etag)
    }

    /// The text of a resource, when the stand-in has it.
    pub fn text(&self, path: &str) -> Option<&str> {
        let (collection, file) = self.locate(path)?;
        let c = self.calendars.iter().chain(self.addressbooks.iter()).find(|c| c.name == collection)?;
        c.items.get(file).map(|(_, text)| text.as_str())
    }

    /// The requests of one method, in order.
    pub fn asked(&self, method: &str) -> Vec<&Asked> {
        self.requests.iter().filter(|a| a.method == method).collect()
    }

    fn locate<'a>(&self, path: &'a str) -> Option<(&'a str, &'a str)> {
        let rest = path.strip_prefix(CALENDARS).or_else(|| path.strip_prefix(BOOKS))?;
        let (collection, file) = rest.split_once('/')?;
        Some((collection, file))
    }

    fn answer(&mut self, asked: &Asked) -> (u16, Option<(String, String)>, Vec<(String, String)>) {
        if let Some(status) = self.refuse_all {
            return (status, None, vec![]);
        }
        let expected = format!("Basic {}", mailcraft_charset::base64::STANDARD.encode(format!("ada:{PASSWORD}")));
        if asked.header("Authorization") != expected {
            return (401, None, vec![]);
        }
        let path = asked.path.as_str();
        match asked.method.as_str() {
            "PROPFIND" => self.propfind(path, asked.header("Depth"), &asked.body),
            "REPORT" => self.report(path),
            "GET" => match self.locate(path).and_then(|(c, f)| self.item(c, f)) {
                Some((etag, text)) => (200, Some(("text/calendar; charset=utf-8".into(), text)), vec![("ETag".into(), etag)]),
                None => (404, None, vec![]),
            },
            "PUT" => self.put_request(path, asked),
            "DELETE" => {
                if let Some(status) = self.refuse_writes.take() {
                    return (status, None, vec![]);
                }
                let Some((collection, file)) = self.locate(path) else { return (404, None, vec![]) };
                let (collection, file) = (collection.to_string(), file.to_string());
                let Some(c) = self.calendars.iter_mut().chain(self.addressbooks.iter_mut()).find(|c| c.name == collection) else {
                    return (404, None, vec![]);
                };
                if c.read_only {
                    return (403, None, vec![]);
                }
                if c.items.remove(&file).is_some() { (204, None, vec![]) } else { (404, None, vec![]) }
            }
            _ => (405, None, vec![]),
        }
    }

    fn item(&self, collection: &str, file: &str) -> Option<(String, String)> {
        let c = self.calendars.iter().chain(self.addressbooks.iter()).find(|c| c.name == collection)?;
        c.items.get(file).cloned()
    }

    fn put_request(&mut self, path: &str, asked: &Asked) -> (u16, Option<(String, String)>, Vec<(String, String)>) {
        if let Some(status) = self.refuse_writes.take() {
            return (status, None, vec![]);
        }
        let Some((collection, file)) = self.locate(path) else { return (404, None, vec![]) };
        let (collection, file) = (collection.to_string(), file.to_string());
        let existing = self.item(&collection, &file);
        if asked.header("If-None-Match") == "*" && existing.is_some() {
            return (412, None, vec![]);
        }
        let if_match = asked.header("If-Match");
        if !if_match.is_empty() && existing.as_ref().is_none_or(|(etag, _)| etag != if_match) {
            return (412, None, vec![]);
        }
        let Some(c) = self.calendars.iter().chain(self.addressbooks.iter()).find(|c| c.name == collection) else { return (404, None, vec![]) };
        if c.read_only {
            return (403, None, vec![]);
        }
        let created = existing.is_none();
        let (_, etag) = self.put(&collection, &file, &asked.body);
        let headers = if self.mute_etag { vec![] } else { vec![("ETag".into(), etag)] };
        (if created { 201 } else { 204 }, None, headers)
    }

    fn propfind(&self, path: &str, depth: &str, body: &str) -> (u16, Option<(String, String)>, Vec<(String, String)>) {
        let xml = |responses: String| {
            (
                207,
                Some((
                    "application/xml; charset=utf-8".into(),
                    format!(
                        r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/" xmlns:oc="http://owncloud.org/ns" xmlns:x1="http://apple.com/ns/ical/">{responses}</d:multistatus>"#
                    ),
                )),
                vec![],
            )
        };
        let ok = |href: &str, props: &str| {
            format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop>{props}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            )
        };
        match path {
            "/.well-known/caldav" | "/.well-known/carddav" => (301, None, vec![("Location".into(), "/remote.php/dav/".into())]),
            "/remote.php/dav/" | "/remote.php/dav" | "/" => {
                if body.contains("current-user-principal") {
                    xml(ok(path, &format!("<d:current-user-principal><d:href>{PRINCIPAL}</d:href></d:current-user-principal>")))
                } else {
                    (403, None, vec![])
                }
            }
            p if p == PRINCIPAL => xml(ok(
                PRINCIPAL,
                &format!(
                    "<cal:calendar-home-set><d:href>{CALENDARS}</d:href></cal:calendar-home-set><card:addressbook-home-set><d:href>{BOOKS}</d:href></card:addressbook-home-set><d:displayname>Ada</d:displayname>"
                ),
            )),
            p if p == CALENDARS || p == BOOKS => {
                let calendars = p == CALENDARS;
                let mut out = ok(p, "<d:resourcetype><d:collection/></d:resourcetype>");
                if depth != "0" {
                    let list = if calendars { &self.calendars } else { &self.addressbooks };
                    for c in list {
                        let kind = if calendars { "<cal:calendar/>" } else { "<card:addressbook/>" };
                        let privileges = if c.read_only {
                            "<d:privilege><d:read/></d:privilege>"
                        } else {
                            "<d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege><d:privilege><d:write-content/></d:privilege>"
                        };
                        let comps: String = c.components.iter().map(|k| format!(r#"<cal:comp name="{k}"/>"#)).collect();
                        let color = if c.color.is_empty() { String::new() } else { format!("<x1:calendar-color>{}</x1:calendar-color>", c.color) };
                        let more = if calendars {
                            format!("{color}<cal:supported-calendar-component-set>{comps}</cal:supported-calendar-component-set>")
                        } else {
                            String::new()
                        };
                        let href = format!("{p}{}/", c.name);
                        out.push_str(&ok(
                            &href,
                            &format!("<d:resourcetype><d:collection/>{kind}</d:resourcetype><d:displayname>{}</d:displayname><cs:getctag>ctag-{}</cs:getctag><d:current-user-privilege-set>{privileges}</d:current-user-privilege-set>{more}", escape(&c.display), c.items.len()),
                        ));
                    }
                }
                xml(out)
            }
            p => match self.locate(p).and_then(|(c, f)| self.item(c, f)) {
                Some((etag, _)) => xml(ok(p, &format!("<d:getetag>{}</d:getetag>", escape(&etag)))),
                None => (404, None, vec![]),
            },
        }
    }

    fn report(&self, path: &str) -> (u16, Option<(String, String)>, Vec<(String, String)>) {
        let calendars = path.starts_with(CALENDARS);
        let Some(name) = path.strip_prefix(CALENDARS).or_else(|| path.strip_prefix(BOOKS)).map(|rest| rest.trim_end_matches('/')) else {
            return (404, None, vec![]);
        };
        let list = if calendars { &self.calendars } else { &self.addressbooks };
        let Some(c) = list.iter().find(|c| c.name == name) else { return (404, None, vec![]) };
        let data = if calendars { "cal:calendar-data" } else { "card:address-data" };
        let mut out = String::new();
        for (file, (etag, text)) in &c.items {
            out.push_str(&format!(
                "<d:response><d:href>{path}{file}</d:href><d:propstat><d:prop><d:getetag>{}</d:getetag><{data}>{}</{data}></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                escape(etag),
                escape(text)
            ));
        }
        (
            207,
            Some((
                "application/xml; charset=utf-8".into(),
                format!(
                    r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav">{out}</d:multistatus>"#
                ),
            )),
            vec![],
        )
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn read_request(stream: &TcpStream) -> Option<Asked> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok().filter(|n| *n > 0)?;
    let mut words = line.split_whitespace();
    let (method, target) = (words.next()?.to_string(), words.next()?.to_string());
    let (mut headers, mut length) = (Vec::new(), 0usize);
    for _ in 0..200 {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 || line.trim().is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        let (name, value) = (name.trim().to_string(), value.trim().to_string());
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().ok().filter(|n| *n <= MAX_REQUEST)?;
        }
        headers.push((name, value));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    let path = target.split('?').next().unwrap_or("").to_string();
    Some(Asked { method, path, headers, body: String::from_utf8_lossy(&body).into_owned(), status: 0 })
}

/// A stand-in on a loopback port, with what it keeps.
pub struct StandIn {
    pub port: u16,
    state: Arc<Mutex<State>>,
}

impl StandIn {
    /// Start one: the calendar `personal` ("Personal") and the address book `contacts`
    /// ("Contacts"), both empty, for `ada` with [`PASSWORD`].
    pub fn start() -> std::io::Result<StandIn> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let mut state = State::default();
        state.add_calendar("personal", "Personal", false);
        state.add_addressbook("contacts", "Contacts");
        let state = Arc::new(Mutex::new(state));
        let shared = state.clone();
        std::thread::Builder::new().name("dav stand-in".into()).spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let Some(mut asked) = read_request(&stream) else { continue };
                let mut state = shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let (status, body, headers) = state.answer(&asked);
                asked.status = status;
                if state.requests.len() >= MAX_KEPT {
                    state.requests.drain(..MAX_KEPT / 2);
                }
                state.requests.push(asked);
                drop(state);
                let reason = match status {
                    200 => "OK",
                    201 => "Created",
                    204 => "No Content",
                    207 => "Multi-Status",
                    301 => "Moved Permanently",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    404 => "Not Found",
                    405 => "Method Not Allowed",
                    412 => "Precondition Failed",
                    _ => "Other",
                };
                let extra: String = headers.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect();
                let answer = match body {
                    Some((kind, text)) => format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
                        text.len()
                    ),
                    None => format!("HTTP/1.1 {status} {reason}\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n"),
                };
                let _ = stream.write_all(answer.as_bytes());
            }
        })?;
        Ok(StandIn { port, state })
    }

    /// `http://127.0.0.1:{port}`: what the owner types as the server's address.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// What it keeps, to set up and to look at.
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
