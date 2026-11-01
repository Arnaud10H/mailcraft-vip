// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reading what a WebDAV server answers (`multistatus`, RFC 4918 §13), and the contacts of an
//! address book (`addressbook-query`, RFC 6352 §8.6).
//!
//! The XML is read as a stream that takes no DTD; element names are matched by their local
//! part, whatever prefix the server gave the namespace. Every field, the number of responses
//! and the depth are capped, and an answer that can't be read is no answer at all (`None`),
//! never a crash.

use quick_xml::Reader;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::Event;

use mailcraft_model::remote::RemoteContact;

use super::{Collection, Link, MAX_ITEMS, refusal};
use crate::NetError;
use crate::http;

/// Longest text kept of one field (bytes), deepest nesting, most responses.
const MAX_FIELD: usize = 1 << 20;
const MAX_DEPTH: usize = 32;
const MAX_RESPONSES: usize = 20_000;
/// Most children named of one property (the privileges, the component kinds).
const MAX_CHILDREN: usize = 64;

/// One property of a response: its text, the names of the elements in it (a `resourcetype`'s
/// kinds, the privileges of a `current-user-privilege-set`), the `href`s in it (a principal, a
/// home), and the component kinds of a `supported-calendar-component-set`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prop {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) children: Vec<String>,
    pub(crate) hrefs: Vec<String>,
    pub(crate) components: Vec<String>,
}

/// One `response` of a `multistatus` answer: what it is about and its properties (those of a
/// `propstat` whose status is 200, and those without a status).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) href: String,
    pub(crate) props: Vec<Prop>,
}

impl Response {
    pub(crate) fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    /// The `getetag`, without its quotes.
    pub(crate) fn etag(&self) -> String {
        self.prop("getetag").map(|p| p.text.trim().trim_matches('"').to_string()).unwrap_or_default()
    }
}

/// Where the reader is in a response.
#[derive(Default)]
struct Cursor {
    response: Option<Response>,
    /// The property being read, and the depth it started at.
    prop: Option<(Prop, usize)>,
    /// Inside `propstat`, the status of the `propstat` (the text of its `status`), and the
    /// properties read under it, which count only when that status is 200.
    propstat: Option<(Vec<Prop>, String)>,
    /// A text element being read (`href`, `status`, or the text of a property): its name and
    /// the depth it started at.
    text: Option<(String, usize, String)>,
}

/// The responses of a `multistatus` answer. `None`: not readable.
pub(crate) fn multistatus(xml: &str) -> Option<Vec<Response>> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    (config.check_end_names, config.allow_unmatched_ends) = (true, false);
    let mut out: Vec<Response> = Vec::new();
    let mut at = Cursor::default();
    let mut depth = 0usize;
    for _ in 0..=xml.len() {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return None;
                }
                let local = local_name(&e);
                component(&mut at, &local, &e);
                open(&mut at, &local, depth);
            }
            Event::Empty(e) => {
                // An element with nothing in it: a kind, a privilege, an empty property.
                let local = local_name(&e);
                component(&mut at, &local, &e);
                open(&mut at, &local, depth + 1);
                close(&mut at, &mut out, &local, depth + 1)?;
            }
            Event::End(e) => {
                let local = e.local_name().as_ref().to_string();
                close(&mut at, &mut out, &local, depth)?;
                depth = depth.checked_sub(1)?;
            }
            Event::Text(t) => push(&mut at, &t.xml10_content()),
            Event::CData(t) => push(&mut at, &t.xml10_content()),
            Event::GeneralRef(r) => {
                let mut one = [0u8; 4];
                match r.resolve_char_ref() {
                    Ok(Some(c)) => push(&mut at, c.encode_utf8(&mut one)),
                    _ => push(&mut at, resolve_xml_entity(&r)?),
                }
            }
            Event::DocType(_) => return None,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    Some(out)
}

fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    e.local_name().as_ref().to_string()
}

/// A `comp` of a `supported-calendar-component-set` names its kind in its `name` attribute.
fn component(at: &mut Cursor, local: &str, e: &quick_xml::events::BytesStart<'_>) {
    if local != "comp" {
        return;
    }
    let Some((prop, _)) = &mut at.prop else { return };
    for attribute in e.attributes().flatten() {
        if attribute.key.local_name().as_ref() == "name" && prop.components.len() < MAX_CHILDREN {
            prop.components.push(attribute.value.trim().to_string());
        }
    }
}

/// `name` opened at `depth`.
fn open(at: &mut Cursor, name: &str, depth: usize) {
    match name {
        "response" if at.response.is_none() => at.response = Some(Response::default()),
        "propstat" if at.response.is_some() && at.propstat.is_none() => at.propstat = Some((Vec::new(), String::new())),
        "prop" => {}
        _ if at.response.is_some() => {
            if let Some((prop, _)) = &mut at.prop {
                // Inside a property: a kind, a privilege, a component, an href, or text.
                match name {
                    "href" => at.text = Some(("href".into(), depth, String::new())),
                    "privilege" => {}
                    "comp" => {}
                    _ if prop.children.len() < MAX_CHILDREN => prop.children.push(name.to_string()),
                    _ => {}
                }
            } else if matches!(name, "href" | "status") {
                at.text = Some((name.to_string(), depth, String::new()));
            } else if at.text.is_none() {
                at.prop = Some((Prop { name: name.to_string(), ..Default::default() }, depth));
            }
        }
        _ => {}
    }
}

/// `name` closed at `depth`.
fn close(at: &mut Cursor, out: &mut Vec<Response>, name: &str, depth: usize) -> Option<()> {
    if let Some((what, started, text)) = at.text.take_if(|(_, started, _)| *started == depth) {
        match (what.as_str(), &mut at.prop, &mut at.response, &mut at.propstat) {
            ("href", Some((prop, _)), _, _) => {
                if prop.hrefs.len() < MAX_CHILDREN {
                    prop.hrefs.push(http::decode(text.trim()));
                }
            }
            ("href", None, Some(response), _) => {
                if response.href.is_empty() {
                    response.href = http::decode(text.trim());
                }
            }
            ("status", None, _, Some((_, status))) => *status = text.trim().to_string(),
            _ => {}
        }
        let _ = started;
    }
    if let Some((prop, _)) = at.prop.take_if(|(_, started)| *started == depth) {
        match &mut at.propstat {
            Some((props, _)) => props.push(prop),
            None => {
                if let Some(response) = &mut at.response {
                    response.props.push(prop);
                }
            }
        }
        return Some(());
    }
    match name {
        "propstat" => {
            if let (Some((props, status)), Some(response)) = (at.propstat.take(), &mut at.response)
                && (status.is_empty() || status.contains(" 200 "))
            {
                response.props.extend(props);
            }
        }
        "response" => {
            if let Some(response) = at.response.take() {
                if out.len() >= MAX_RESPONSES {
                    return None;
                }
                out.push(response);
            }
        }
        _ => {}
    }
    Some(())
}

fn push(at: &mut Cursor, text: &str) {
    if let Some((_, _, have)) = &mut at.text {
        if have.len().saturating_add(text.len()) <= MAX_FIELD {
            have.push_str(text);
        }
    } else if let Some((prop, _)) = &mut at.prop
        && prop.text.len().saturating_add(text.len()) <= MAX_FIELD
    {
        prop.text.push_str(text);
    }
}

/// The contacts of an address book: one `addressbook-query` for every card.
pub(crate) fn contacts(link: &Link<'_>, book: &Collection) -> Result<Vec<RemoteContact>, NetError> {
    let body = br#"<?xml version="1.0" encoding="utf-8"?><card:addressbook-query xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:prop><d:getetag/><card:address-data/></d:prop></card:addressbook-query>"#;
    let reply = link.call("REPORT", &book.href, &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")], Some(body))?;
    if reply.status != 207 {
        return Err(refusal("reading the contacts", &reply));
    }
    let responses =
        multistatus(&String::from_utf8_lossy(&reply.body)).ok_or_else(|| NetError::Config("the server's answer couldn't be read".into()))?;
    let mut out = Vec::new();
    for r in responses.iter().take(MAX_ITEMS) {
        let Some(data) = r.prop("address-data").filter(|p| !p.text.trim().is_empty()) else { continue };
        let href = link.url_of(&r.href).map(|u| u.path).unwrap_or_default();
        if href.is_empty() {
            continue;
        }
        let etag = r.etag();
        // One card to a resource, as the standard has it; a resource with more gives the first.
        if let Some(card) = mailcraft_vcard::parse(&data.text).into_iter().next() {
            out.push(super::contact_of(&href, &etag, &card));
        }
    }
    Ok(out)
}
