//! The calendar of a CalDAV server (Nextcloud), for reading: one `REPORT` for the events of a
//! stretch, expanded by the server into their occurrences. Basic authentication, inside TLS only
//! (`http::Url` takes nothing else but this machine). What comes back is the server's text as it
//! is: the engine reads the iCalendar.
//!
//! The answer is as hostile as any server's: read as a stream that takes no DTD, with every field
//! and the number of items capped.

use std::time::Duration;

use mailcraft_model::vip::{Dav, DavDownload, DavItem};
use mailcraft_time::{DateTime, Timestamp};
use quick_xml::Reader;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::Event;

use crate::NetError;
use crate::http::{self, Request, Url};

/// The stretch of the calendar asked for: a month back, a year ahead (seconds).
const BEHIND: i64 = 31 * 86_400;
const AHEAD: i64 = 366 * 86_400;
/// Most items read of a collection, longest text kept of one (bytes), deepest nesting.
const MAX_ITEMS: usize = 5_000;
const MAX_FIELD: usize = 1 << 20;
const MAX_DEPTH: usize = 32;
const PATIENCE: Duration = Duration::from_secs(60);

/// Bring down the events the server has from a month back to a year ahead of `now`. With the
/// problem, in one line, that left them out.
pub fn download(dav: &Dav, password: &str, now: Timestamp) -> (DavDownload, Vec<String>) {
    let (from, to) = (now - BEHIND, now + AHEAD);
    let mut out = DavDownload { from, to, ..Default::default() };
    let mut errors = Vec::new();
    let (start, end) = (stamp(from), stamp(to));
    let calendar = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data><c:expand start="{start}" end="{end}"/></c:calendar-data></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{start}" end="{end}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    );
    match report(dav, password, &dav.calendar_path(), &calendar) {
        Ok(items) => out.events = Some(items),
        Err(e) => errors.push(format!("Nextcloud calendar: {e}")),
    }
    (out, errors)
}

/// `20261019T080000Z`.
fn stamp(t: Timestamp) -> String {
    let d = DateTime::from_timestamp(t, 0);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", d.date.year, d.date.month, d.date.day, d.hour, d.minute, d.second)
}

fn report(dav: &Dav, password: &str, path: &str, body: &str) -> Result<Vec<DavItem>, NetError> {
    let url = Url::parse(&format!("{}{}", dav.url.trim().trim_end_matches('/'), path))?;
    let auth = format!("Basic {}", mailcraft_charset::base64::STANDARD.encode(format!("{}:{password}", dav.username)));
    let headers = [("Authorization", auth.as_str()), ("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")];
    let reply = http::request(
        &url,
        &Request { method: "REPORT", headers: &headers, body: Some(body.as_bytes()), patience: PATIENCE, max_body: http::MAX_LARGE_BODY },
    )?;
    match reply.status {
        207 => multistatus(&String::from_utf8_lossy(&reply.body)).ok_or_else(|| NetError::Config("the server's answer couldn't be read".into())),
        401 | 403 => Err(NetError::Config("the server refused the user name or the password (use an app password)".into())),
        404 => Err(NetError::Config(format!("the server has nothing at {path}"))),
        status => Err(NetError::Config(format!("the server answered {status}"))),
    }
}

/// The items of a `multistatus` answer: each `response` with its `href`, `getetag` and
/// `calendar-data`. Those without data are left out. `None`: not readable.
pub(crate) fn multistatus(xml: &str) -> Option<Vec<DavItem>> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    (config.check_end_names, config.allow_unmatched_ends) = (true, false);
    let (mut items, mut item, mut depth) = (Vec::new(), DavItem::default(), 0usize);
    // The field being read, by its element's local name, and its text.
    let mut field: Option<(Vec<u8>, String)> = None;
    for _ in 0..=xml.len() {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return None;
                }
                let local = AsRef::<str>::as_ref(&e.local_name()).as_bytes().to_vec();
                match local.as_slice() {
                    b"response" => item = DavItem::default(),
                    b"href" | b"getetag" | b"calendar-data" => field = Some((local, String::new())),
                    _ => {}
                }
            }
            Event::End(e) => {
                depth = depth.checked_sub(1)?;
                let local = e.local_name();
                if let Some((name, text)) = field.take_if(|(name, _)| name.as_slice() == AsRef::<str>::as_ref(&local).as_bytes()) {
                    match name.as_slice() {
                        b"href" if item.href.is_empty() => item.href = http::decode(text.trim()),
                        b"getetag" => item.etag = text.trim().trim_matches('"').to_string(),
                        b"calendar-data" => item.data = text,
                        _ => {}
                    }
                }
                if AsRef::<str>::as_ref(&local) == "response" && !item.data.is_empty() && !item.href.is_empty() {
                    if items.len() >= MAX_ITEMS {
                        break;
                    }
                    items.push(std::mem::take(&mut item));
                }
            }
            Event::Text(t) => push(&mut field, &t.xml10_content()),
            Event::CData(t) => push(&mut field, &t.xml10_content()),
            Event::GeneralRef(r) => {
                let mut one = [0u8; 4];
                match r.resolve_char_ref() {
                    Ok(Some(c)) => push(&mut field, c.encode_utf8(&mut one)),
                    _ => push(&mut field, resolve_xml_entity(&r)?),
                }
            }
            Event::DocType(_) => return None,
            Event::Empty(_) | Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    Some(items)
}

fn push(field: &mut Option<(Vec<u8>, String)>, text: &str) {
    if let Some((_, have)) = field
        && have.len().saturating_add(text.len()) <= MAX_FIELD
    {
        have.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_multistatus() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/remote.php/dav/calendars/a/personal/x%20y.ics</d:href>
  <d:propstat><d:prop><d:getetag>&quot;e1&quot;</d:getetag>
   <cal:calendar-data>BEGIN:VCALENDAR&#13;
SUMMARY:R&amp;D
END:VCALENDAR</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/a/personal/</d:href><d:propstat><d:prop><d:getetag/></d:prop></d:propstat></d:response>
</d:multistatus>"#;
        let items = multistatus(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].href, "/remote.php/dav/calendars/a/personal/x y.ics");
        assert_eq!(items[0].etag, "e1");
        assert!(items[0].data.contains("SUMMARY:R&D"));
        assert!(multistatus("<!DOCTYPE x><x/>").is_none());
        assert_eq!(stamp(0), "19700101T000000Z");
    }
}
