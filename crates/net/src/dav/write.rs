// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Telling the server what was made, changed or deleted in the calendar here
//! ([`mailcraft_model::remote::EventsUpload`]), one change after the other, and what it said
//! to each ([`mailcraft_model::remote::EventsUploaded`]).
//!
//! A new event is written as a resource of its own (`PUT` with `If-None-Match: *`, RFC 4791
//! §5.3.2) named by its `UID`. A change to one the server has reads the resource back (`GET`),
//! makes the change to the event in it and writes it back with `If-Match` on the version that
//! was read, so that a change made on the server meanwhile is not written over: the server then
//! answers 412, the change is given up here (`Refused`), and what the server has comes down. A
//! deletion is a `DELETE`. A change to one day of a series, an answer to an invitation and a
//! forwarding are not made for this kind of server yet: the server is told nothing, and the
//! owner is told so.

use mailcraft_model::remote::{EventChangeKind, EventFields, EventOp, EventOutcome, EventResult, EventsUpload, EventsUploaded};
use mailcraft_model::{Attendee, Event, OnlineMeeting, Response};
use mailcraft_time::{DAY, DateTime, Timestamp, Zone};

use super::{Home, Link, refusal};
use crate::NetError;

/// Most changes carried out in one job; the rest wait for the next.
const MAX_OPS: usize = 200;
/// Longest resource read back for a change, in bytes.
const MAX_RESOURCE: usize = 4 * 1024 * 1024;

/// Every change as not made, with `why`: the server couldn't be reached at all.
pub(crate) fn untried(changes: &EventsUpload, why: &str) -> EventsUploaded {
    let outcomes = changes
        .ops
        .iter()
        .map(|op| EventOutcome { op: op.clone(), result: EventResult::Failed, error: why.to_string(), ..Default::default() })
        .collect();
    EventsUploaded { account: changes.account, email: changes.email.clone(), outcomes, token_refused: false }
}

/// Carry the changes out, oldest first, and say what came of each.
pub(crate) fn upload(link: &Link<'_>, home: &Home, changes: &EventsUpload, offset_at: &dyn Fn(Timestamp) -> i32) -> EventsUploaded {
    let zone = Some(&changes.zone).filter(|z| !z.is_empty()).and_then(|z| Zone::named(z));
    let mut outcomes = Vec::new();
    for op in changes.ops.iter().take(MAX_OPS) {
        let mut outcome = match one(link, home, op, zone, offset_at) {
            Ok(outcome) => outcome,
            Err(e) => EventOutcome { op: op.clone(), result: result_of(&e), error: e.to_string(), ..Default::default() },
        };
        outcome.tidy();
        outcomes.push(outcome);
    }
    EventsUploaded { account: changes.account, email: changes.email.clone(), outcomes, token_refused: false }
}

/// What an error means for a change.
fn result_of(error: &NetError) -> EventResult {
    match error {
        NetError::Forbidden(_) => EventResult::Denied,
        NetError::Config(text) if text.contains("nothing there") => EventResult::Gone,
        NetError::Config(text) if text.contains("changed since") => EventResult::Refused,
        _ => EventResult::Failed,
    }
}

/// One change, carried out.
fn one(link: &Link<'_>, home: &Home, op: &EventOp, zone: Option<Zone>, offset_at: &dyn Fn(Timestamp) -> i32) -> Result<EventOutcome, NetError> {
    let not_here = |what: &str| NetError::Config(format!("{what} isn't done for a calendar server yet: the change stays here"));
    if op.of_occurrence() {
        return Err(not_here("changing one day of a series"));
    }
    match op.kind {
        EventChangeKind::Create => create(link, home, op, zone, offset_at),
        EventChangeKind::Update => update(link, op, zone, offset_at),
        EventChangeKind::Delete => delete(link, op),
        EventChangeKind::Respond => Err(not_here("answering an invitation")),
        EventChangeKind::Forward => Err(not_here("forwarding a meeting")),
    }
}

/// A new event, as a resource of its own in the calendar it goes into.
fn create(link: &Link<'_>, home: &Home, op: &EventOp, zone: Option<Zone>, offset_at: &dyn Fn(Timestamp) -> i32) -> Result<EventOutcome, NetError> {
    let calendar = home.calendar(&op.calendar).ok_or_else(|| NetError::Config("the server has no such calendar".into()))?;
    if !calendar.rights.can_edit() {
        return Err(NetError::Forbidden("the calendar is read-only".into()));
    }
    let mut event = Event::default();
    let uid = fresh_uid()?;
    event.uid = uid.clone();
    put_fields(&mut event, &op.fields, offset_at);
    let text = mailcraft_ical::write_in(&[&event], &[], &mailcraft_ical::WriteOptions { method: String::new(), now: mailcraft_time::now() }, zone);
    let href = format!("{}{uid}.ics", calendar.href.trim_end_matches('/').to_string() + "/");
    let headers = [("If-None-Match", "*"), ("Content-Type", "text/calendar; charset=utf-8")];
    let reply = link.call("PUT", &href, &headers, Some(text.as_bytes()))?;
    if !matches!(reply.status, 200 | 201 | 204) {
        return Err(refusal("making the appointment", &reply));
    }
    let change_key = match reply.etag.as_deref() {
        Some(etag) => etag.trim_matches('"').to_string(),
        None => etag_of(link, &href).unwrap_or_default(),
    };
    Ok(EventOutcome { op: op.clone(), result: EventResult::Done, remote: href, change_key, uid, status: reply.status, ..Default::default() })
}

/// A change to an event the server has: read back, changed, written back on the version read.
fn update(link: &Link<'_>, op: &EventOp, zone: Option<Zone>, offset_at: &dyn Fn(Timestamp) -> i32) -> Result<EventOutcome, NetError> {
    if op.remote.is_empty() {
        return Err(NetError::Config("the server doesn't know the appointment".into()));
    }
    let reply = link.call("GET", &op.remote, &[], None)?;
    if reply.status != 200 {
        return Err(refusal("reading the appointment back", &reply));
    }
    if reply.body.len() > MAX_RESOURCE {
        return Err(NetError::Config("the appointment on the server is too large to change here".into()));
    }
    let had = String::from_utf8_lossy(&reply.body).to_string();
    let file = mailcraft_ical::parse(&had, offset_at);
    let mut events = file.events;
    if events.iter().any(|e| e.uid.contains('#')) {
        return Err(NetError::Config(
            "the series has days that were changed on their own, which can't be changed here yet: the change stays here".into(),
        ));
    }
    let Some(mut event) = events.pop() else { return Err(NetError::Config("the server's copy holds no appointment".into())) };
    if event.uid.is_empty() {
        event.uid = fresh_uid()?;
    }
    put_fields(&mut event, &op.fields, offset_at);
    let text = mailcraft_ical::write_in(&[&event], &[], &mailcraft_ical::WriteOptions { method: String::new(), now: mailcraft_time::now() }, zone);
    let etag = reply.etag.clone().unwrap_or_default();
    let mut headers: Vec<(&str, &str)> = vec![("Content-Type", "text/calendar; charset=utf-8")];
    if !etag.is_empty() {
        headers.push(("If-Match", etag.as_str()));
    }
    let reply = link.call("PUT", &op.remote, &headers, Some(text.as_bytes()))?;
    if !matches!(reply.status, 200 | 201 | 204) {
        return Err(refusal("changing the appointment", &reply));
    }
    let change_key = match reply.etag.as_deref() {
        Some(etag) => etag.trim_matches('"').to_string(),
        None => etag_of(link, &op.remote).unwrap_or_default(),
    };
    Ok(EventOutcome {
        op: op.clone(),
        result: EventResult::Done,
        remote: op.remote.clone(),
        change_key,
        uid: event.uid,
        status: reply.status,
        ..Default::default()
    })
}

fn delete(link: &Link<'_>, op: &EventOp) -> Result<EventOutcome, NetError> {
    if op.remote.is_empty() {
        return Err(NetError::Config("the server doesn't know the appointment".into()));
    }
    let reply = link.call("DELETE", &op.remote, &[], None)?;
    match reply.status {
        200 | 202 | 204 => {
            Ok(EventOutcome { op: op.clone(), result: EventResult::Done, remote: op.remote.clone(), status: reply.status, ..Default::default() })
        }
        404 | 410 => {
            Ok(EventOutcome { op: op.clone(), result: EventResult::Gone, remote: op.remote.clone(), status: reply.status, ..Default::default() })
        }
        _ => Err(refusal("deleting the appointment", &reply)),
    }
}

/// The `getetag` of a resource, asked for by itself (a server that doesn't say it on a `PUT`).
fn etag_of(link: &Link<'_>, href: &str) -> Option<String> {
    let body = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:getetag/></d:prop></d:propfind>"#;
    let reply = link.call("PROPFIND", href, &[("Depth", "0"), ("Content-Type", "application/xml; charset=utf-8")], Some(body)).ok()?;
    if reply.status != 207 {
        return None;
    }
    let responses = super::multistatus(&String::from_utf8_lossy(&reply.body))?;
    responses.first().map(|r| r.etag()).filter(|etag| !etag.is_empty())
}

/// A `UID` nobody else has: random, with this program's name after it.
fn fresh_uid() -> Result<String, NetError> {
    let mut buf = [0u8; 16];
    use ring::rand::SecureRandom;
    ring::rand::SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| NetError::Config("this computer couldn't make the random numbers an appointment is named with".into()))?;
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{hex}@mailcraft"))
}

/// Make `event` as the owner told the server it is: the fields that were sent, with times as
/// they read here (the engine's own rule, kept in step with it).
pub(crate) fn put_fields(event: &mut Event, fields: &EventFields, offset_at: &dyn Fn(Timestamp) -> i32) {
    if let Some(title) = &fields.title {
        event.title = title.clone();
    }
    if let Some(place) = &fields.location {
        event.location = place.clone();
    }
    if let Some(words) = &fields.description {
        event.description = words.clone();
    }
    if let Some(time) = fields.time {
        event.all_day = time.all_day;
        (event.start_repeated, event.end_repeated) = (false, false);
        if time.all_day {
            // The days the server names: midnight (UTC) of the first, and of the day after the last.
            event.start = DateTime::at_midnight(DateTime::from_timestamp(time.start, 0).date);
            let last = DateTime::from_timestamp(time.end.max(time.start.saturating_add(DAY)), 0).date;
            event.end = DateTime::at_midnight(last);
            (event.start_zone, event.end_zone) = (String::new(), String::new());
        } else {
            let zones = fields.zones.clone().unwrap_or_default();
            (event.start_zone, event.end_zone) = (zones.start.clone(), if zones.end == zones.start { String::new() } else { zones.end });
            event.set_instants(time.start, time.end, offset_at);
        }
    }
    if let Some(show_as) = fields.show_as {
        event.show_as = show_as;
    }
    if let Some(private) = fields.private {
        event.private = private;
    }
    if let Some(reminder) = fields.reminder {
        event.reminder_minutes = reminder.on.then_some(reminder.minutes);
    }
    if let Some(repeat) = &fields.repeat {
        event.recurrence = repeat.rule.clone();
        if repeat.rule.is_none() {
            event.exceptions.clear();
        }
    }
    if let Some(people) = &fields.attendees {
        let answered =
            |who: &Attendee| event.attendees.iter().find(|old| old.address.same_mailbox(&who.address)).map_or(Response::None, |old| old.response);
        event.attendees = people.iter().map(|who| Attendee { response: answered(who), ..who.clone() }).collect();
    }
    if let Some(categories) = &fields.categories {
        event.categories = categories.clone();
    }
    if let Some(importance) = fields.importance {
        event.importance = importance;
    }
    if let Some(asked) = fields.request_responses {
        event.response_options.request_responses = asked;
    }
    if let Some(allowed) = fields.allow_proposals {
        event.response_options.allow_proposals = allowed;
    }
    if let Some(provider) = fields.online
        && event.online.is_none()
    {
        event.online = Some(OnlineMeeting { provider, join_url: String::new() });
    }
}
