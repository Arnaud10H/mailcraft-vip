// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The events of a calendar on the server, a stretch at a time (`calendar-query` with a
//! `time-range`, RFC 4791 §7.8), as the engine takes a server's events in
//! ([`mailcraft_model::remote::EventsDownload`]): an event of its own as itself, a series as
//! itself (with its rule) and as its occurrences in the stretch, and a changed instance of a
//! series (`RECURRENCE-ID`) as an exception of it. What the server calls a resource (its path)
//! is what the engine calls the event; an occurrence is called by its series and its instant.

use mailcraft_model::remote::{EventsDownload, RemoteEvent, RemoteEventKind};
use mailcraft_model::{Event, Response};
use mailcraft_time::{DAY, Date, DateTime, Timestamp};

use super::{Collection, Link, MAX_ITEMS, refusal};
use crate::NetError;

/// The wide stretch: a month back, a year ahead (seconds).
pub(crate) const WIDE_BEHIND: i64 = 31 * 86_400;
pub(crate) const WIDE_AHEAD: i64 = 366 * 86_400;
/// Most occurrences made of one series in one download.
const MAX_OCCURRENCES: usize = 2_000;

/// `20261019T080000Z`.
pub(crate) fn stamp(t: Timestamp) -> String {
    let d = DateTime::from_timestamp(t, 0);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", d.date.year, d.date.month, d.date.day, d.hour, d.minute, d.second)
}

/// One resource of the calendar, with its text.
pub(crate) struct Resource {
    pub(crate) href: String,
    pub(crate) etag: String,
    pub(crate) data: String,
}

/// The resources of `calendar` that have anything in the stretch.
pub(crate) fn resources(link: &Link<'_>, calendar: &Collection, from: Timestamp, to: Timestamp) -> Result<Vec<Resource>, NetError> {
    let (start, end) = (stamp(from), stamp(to));
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{start}" end="{end}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    );
    let reply = link.call("REPORT", &calendar.href, &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")], Some(body.as_bytes()))?;
    if reply.status != 207 {
        return Err(refusal("reading the calendar", &reply));
    }
    let responses =
        super::multistatus(&String::from_utf8_lossy(&reply.body)).ok_or_else(|| NetError::Config("the server's answer couldn't be read".into()))?;
    let mut out = Vec::new();
    for r in responses.iter().take(MAX_ITEMS) {
        let Some(data) = r.prop("calendar-data").filter(|p| !p.text.trim().is_empty()) else { continue };
        let href = link.url_of(&r.href).map(|u| u.path).unwrap_or_default();
        if href.is_empty() {
            continue;
        }
        out.push(Resource { href, etag: r.etag(), data: data.text.clone() });
    }
    Ok(out)
}

/// The stretch `from`–`to` of `calendar`, as the engine takes it in.
pub(crate) fn download(
    link: &Link<'_>,
    calendar: &Collection,
    from: Timestamp,
    to: Timestamp,
    owner: &str,
    offset_at: &dyn Fn(Timestamp) -> i32,
) -> Result<EventsDownload, NetError> {
    let resources = resources(link, calendar, from, to)?;
    let mut events = Vec::new();
    for resource in &resources {
        events.extend(remote_events(resource, from, to, owner, offset_at));
        if events.len() >= MAX_ITEMS {
            break;
        }
    }
    let complete = events.len() < MAX_ITEMS;
    events.truncate(MAX_ITEMS);
    Ok(EventsDownload { from, to, complete, events, has_categories: true, has_options: true, ..Default::default() })
}

/// What the engine is told of one resource: its event, or its series with its occurrences in
/// the stretch and its changed instances.
pub(crate) fn remote_events(
    resource: &Resource,
    from: Timestamp,
    to: Timestamp,
    owner: &str,
    offset_at: &dyn Fn(Timestamp) -> i32,
) -> Vec<RemoteEvent> {
    let file = mailcraft_ical::parse(&resource.data, offset_at);
    let mut out = Vec::new();
    // The series (or the event) is the one without an instance mark; the instances name it.
    for event in &file.events {
        let instance: Option<String> = event.uid.split_once('#').map(|(_, at)| at.to_string());
        let mut remote = remote_of(event, resource, owner, offset_at);
        match (instance, event.recurrence.is_some()) {
            (Some(at), _) => {
                remote.id = format!("{}#{at}", resource.href);
                remote.kind = RemoteEventKind::Exception;
                remote.series = resource.href.clone();
                remote.uid = event.uid.split_once('#').map(|(uid, _)| uid.to_string()).unwrap_or_default();
                remote.original = Some(instance_instant(&at, event, offset_at));
                out.push(remote);
            }
            (None, true) => {
                remote.kind = RemoteEventKind::SeriesMaster;
                remote.recurrence = event.recurrence.clone();
                remote.series_day = Some(event.start.date);
                let occurrences = occurrences_of(event, &remote, from, to, offset_at);
                out.push(remote);
                out.extend(occurrences);
            }
            (None, false) => out.push(remote),
        }
    }
    out
}

/// The occurrences of `series` in the stretch, each an event of the series' own.
fn occurrences_of(series: &Event, remote: &RemoteEvent, from: Timestamp, to: Timestamp, offset_at: &dyn Fn(Timestamp) -> i32) -> Vec<RemoteEvent> {
    let day_of = |instant: Timestamp| DateTime::from_timestamp(instant, 0).date;
    let (first, last) = (day_of(from).add_days(-2), day_of(to).add_days(2));
    let (begins, ends) = series.instants(offset_at);
    let length = ends.saturating_sub(begins);
    let mut out = Vec::new();
    for occurrence in series.occurrences_in(first, last, offset_at).into_iter().take(MAX_OCCURRENCES) {
        let start =
            if series.all_day { DateTime::at_midnight(occurrence.day).to_timestamp(0) } else { series.occurrence_instant(occurrence.day, offset_at) };
        let end = if series.all_day { start.saturating_add(length.max(DAY)) } else { start.saturating_add(length) };
        if end <= from || start >= to {
            continue;
        }
        out.push(RemoteEvent {
            id: format!("{}#{start}", remote.id),
            kind: RemoteEventKind::Occurrence,
            series: remote.id.clone(),
            original: Some(start),
            start,
            end,
            recurrence: None,
            series_day: None,
            ..remote.clone()
        });
    }
    out
}

/// A `RECURRENCE-ID` as written (`20261026T140000`, `20261026T140000Z`, `20261026`), as the
/// instant the series has that instance at. A time without a zone is read by the calendar's
/// own clock, as the reader read the event's times.
fn instance_instant(written: &str, event: &Event, offset_at: &dyn Fn(Timestamp) -> i32) -> Timestamp {
    let digits = |s: &str, from: usize, len: usize| -> Option<i64> { s.get(from..from.checked_add(len)?)?.parse::<i64>().ok() };
    let text = written.trim();
    let date = (|| {
        let (year, month, day) = (digits(text, 0, 4)?, digits(text, 4, 2)?, digits(text, 6, 2)?);
        Date::new(i32::try_from(year).ok()?, u8::try_from(month).ok()?, u8::try_from(day).ok()?)
    })();
    let Some(date) = date else {
        return event.instants(offset_at).0;
    };
    if text.len() < 15 || !text.get(8..9).is_some_and(|t| t == "T") {
        return DateTime::at_midnight(date).to_timestamp(0);
    }
    let (hour, minute, second) = (digits(text, 9, 2).unwrap_or(0), digits(text, 11, 2).unwrap_or(0), digits(text, 13, 2).unwrap_or(0));
    let wall = DateTime::new(date, hour.clamp(0, 23) as u8, minute.clamp(0, 59) as u8, second.clamp(0, 59) as u8)
        .unwrap_or_else(|| DateTime::at_midnight(date));
    if text.ends_with('Z') {
        return wall.to_timestamp(0);
    }
    // By the zone the event has, else by the clock here on that day.
    match event.zone() {
        Some(zone) => zone.instant(wall),
        None => {
            let about = wall.to_timestamp(0);
            wall.to_timestamp(offset_at(about))
        }
    }
}

/// An event of the mailbox's shape as the server's: instants for its times, the resource for
/// its name.
fn remote_of(event: &Event, resource: &Resource, owner: &str, offset_at: &dyn Fn(Timestamp) -> i32) -> RemoteEvent {
    let (start, end) = if event.all_day {
        let start = event.start.to_timestamp(0);
        (start, event.end.to_timestamp(0).max(start.saturating_add(DAY)))
    } else {
        event.instants(offset_at)
    };
    // What the owner answered, as the list of people says of the owner.
    let response = event
        .attendees
        .iter()
        .find(|a| !owner.trim().is_empty() && a.address.email.trim().eq_ignore_ascii_case(owner.trim()))
        .map(|a| a.response)
        .unwrap_or(Response::None);
    let mut remote = RemoteEvent {
        id: resource.href.clone(),
        change_key: resource.etag.clone(),
        uid: event.uid.clone(),
        title: event.title.clone(),
        location: event.location.clone(),
        description: event.description.clone(),
        start,
        end,
        all_day: event.all_day,
        start_zone: event.start_zone.clone(),
        end_zone: event.end_zone.clone(),
        organizer: event.organizer.clone(),
        attendees: event.attendees.clone(),
        response,
        show_as: event.show_as,
        private: event.private,
        cancelled: event.cancelled,
        kind: RemoteEventKind::Single,
        reminder_minutes: event.reminder_minutes,
        categories: event.categories.clone(),
        importance: event.importance,
        options: event.response_options.clone(),
        online: event.online.clone(),
        attachments: event.has_attachments,
        ..Default::default()
    };
    remote.tidy();
    remote
}
