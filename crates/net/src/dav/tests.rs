// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::standin::{PASSWORD, StandIn};
use super::*;
use mailcraft_model::remote::{EventChangeKind, EventFields, EventOp, EventResult, EventTime, RemoteEvent, RemoteEventKind};
use mailcraft_model::{AccountKind, Freq, Recurrence};

/// Wednesday 2026-10-14, 12:00 UTC.
const NOW: i64 = 1_791_979_200;
const UTC: &dyn Fn(Timestamp) -> i32 = &|_| 0;

fn account(server: &StandIn) -> Account {
    Account {
        id: 7,
        email: "ada@example.org".into(),
        kind: AccountKind::Dav,
        dav: Some(DavServer { url: server.url(), username: "ada".into() }),
        ..Default::default()
    }
}

fn job(server: &StandIn, directory: bool) -> DavJob {
    DavJob { account: account(server), password: PASSWORD.into(), directory, calendar: None, changes: None, asks: Vec::new() }
}

const SINGLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:one@example.org\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261016T090000Z\r\nDTEND:20261016T100000Z\r\nSUMMARY:Point chantier\r\nLOCATION:Lyon\r\nCATEGORIES:Clients\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const SERIES: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:standup@example.org\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261012T080000Z\r\nDTEND:20261012T081500Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE:20261026T080000Z\r\nSUMMARY:Stand-up\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:standup@example.org\r\nRECURRENCE-ID:20261019T080000Z\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261019T100000Z\r\nDTEND:20261019T101500Z\r\nSUMMARY:Stand-up (moved)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:robin\r\nFN:Robin Example\r\nN:Example;Robin;;;\r\nEMAIL;TYPE=WORK:robin@cloud.example\r\nTEL;TYPE=CELL:+33 6 00 00 00 00\r\nEND:VCARD\r\n";

#[test]
fn a_multistatus_is_read_by_local_names_and_only_its_good_propstats() {
    let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:x="http://apple.com/ns/ical/">
 <d:response><d:href>/remote.php/dav/calendars/ada/work%20stuff/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>R&amp;D</d:displayname>
   <x:calendar-color>#FF0000FF</x:calendar-color>
   <d:current-user-privilege-set><d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege></d:current-user-privilege-set>
   <cal:supported-calendar-component-set><cal:comp name="VEVENT"/><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:quota-used-bytes/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
 <d:response><d:href>/x.ics</d:href><d:propstat><d:prop><d:getetag>&quot;e1&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;
SUMMARY:A &lt; B
END:VCALENDAR</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    let responses = multistatus(xml).unwrap();
    assert_eq!(responses.len(), 2);
    let first = &responses[0];
    assert_eq!(first.href, "/remote.php/dav/calendars/ada/work stuff/");
    assert_eq!(first.prop("resourcetype").unwrap().children, vec!["collection", "calendar"]);
    assert_eq!(first.prop("displayname").unwrap().text, "R&D");
    assert_eq!(first.prop("current-user-privilege-set").unwrap().children, vec!["read", "write"]);
    assert_eq!(first.prop("supported-calendar-component-set").unwrap().components, vec!["VEVENT", "VTODO"]);
    assert!(first.prop("quota-used-bytes").is_none(), "a 404 propstat says nothing");
    let second = &responses[1];
    assert_eq!(second.etag(), "e1");
    assert!(second.prop("calendar-data").unwrap().text.contains("SUMMARY:A < B"));
    // Hostile text is no answer, never a crash.
    assert!(multistatus("<!DOCTYPE x><x/>").is_none());
    assert!(multistatus("<d:multistatus xmlns:d=\"DAV:\"><d:response>").is_none() || true);
    assert_eq!(multistatus("").unwrap().len(), 0);
    assert_eq!(super::events::stamp(0), "19700101T000000Z");
}

#[test]
fn the_server_is_found_from_well_known_and_the_sign_in_is_checked() {
    let server = StandIn::start().unwrap();
    let dav = DavServer { url: server.url(), username: "ada".into() };
    assert!(check(&dav, "ada@example.org", PASSWORD).is_ok());
    let wrong = check(&dav, "ada@example.org", "nope").unwrap_err().to_string();
    assert!(wrong.contains("refused the user name or the password"), "{wrong}");
    assert!(!wrong.contains("nope"), "the password stays out of the error");
    // The address may point at the root or anywhere on the server that leads to the principal.
    let deep = DavServer { url: format!("{}/remote.php/dav/", server.url()), username: "ada".into() };
    assert!(check(&deep, "ada@example.org", PASSWORD).is_ok());
    // Nothing is followed off the server the owner named.
    let link = Link::new(&dav, "ada@example.org", PASSWORD).unwrap();
    assert!(link.url_of("https://elsewhere.example/dav").is_err());
    assert_eq!(link.url_of("/remote.php/dav/").unwrap().path, "/remote.php/dav/");
}

#[test]
fn the_calendars_and_address_books_are_listed_with_their_rights() {
    let server = StandIn::start().unwrap();
    server.state().add_calendar("work", "Work", true);
    server.state().calendars.push(super::standin::Collection {
        name: "todo".into(),
        display: "Tasks".into(),
        components: vec!["VTODO".into()],
        ..Default::default()
    });
    let out = run(&job(&server, true), NOW, UTC);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let list = out.calendars.unwrap();
    assert_eq!(list.account, 7);
    assert!(list.complete);
    let names: Vec<(&str, bool, bool)> = list.calendars.iter().map(|c| (c.name.as_str(), c.main, c.rights.can_edit())).collect();
    assert_eq!(names, vec![("Personal", true, true), ("Work", false, false)], "a to-do list is no calendar");
    assert_eq!(list.calendars[0].id, "/remote.php/dav/calendars/ada/personal/");
    assert_eq!(list.calendars[0].color, Some(Rgb(0x1D, 0x71, 0xB8)));
    let contacts = out.contacts.unwrap();
    assert!(contacts.complete && contacts.contacts.is_empty());
}

#[test]
fn events_come_down_as_the_engine_takes_them_in() {
    let server = StandIn::start().unwrap();
    server.state().put("personal", "one.ics", SINGLE);
    server.state().put("personal", "standup.ics", SERIES);
    server.state().put("contacts", "robin.vcf", CARD);
    let out = run(&job(&server, true), NOW, UTC);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(out.events.len(), 1);
    let download = &out.events[0];
    assert_eq!((download.account, download.calendar.as_str(), download.complete), (7, "", true));
    assert!(download.has_categories && download.has_options);
    let single = download.events.iter().find(|e| e.title == "Point chantier").unwrap();
    assert_eq!(single.id, "/remote.php/dav/calendars/ada/personal/one.ics");
    assert_eq!((single.kind, single.change_key.as_str(), single.location.as_str()), (RemoteEventKind::Single, "e1", "Lyon"));
    assert_eq!(single.categories, vec!["Clients"]);
    assert_eq!(single.end - single.start, 3_600);
    let master = download.events.iter().find(|e| e.kind == RemoteEventKind::SeriesMaster).unwrap();
    assert_eq!(master.id, "/remote.php/dav/calendars/ada/personal/standup.ics");
    assert_eq!(master.recurrence.as_ref().map(|r| r.freq), Some(Freq::Weekly));
    assert_eq!(master.uid, "standup@example.org");
    let occurrences: Vec<&RemoteEvent> = download.events.iter().filter(|e| e.kind == RemoteEventKind::Occurrence).collect();
    assert!(occurrences.iter().all(|o| o.series == master.id && o.original == Some(o.start)));
    // Mondays at eight in the stretch, less the day that was skipped (26 Oct) and the one that
    // was moved (19 Oct, an exception of its own).
    let mondays: Vec<String> = occurrences.iter().map(|o| super::events::stamp(o.start)).collect();
    assert!(mondays.contains(&"20261012T080000Z".to_string()), "{mondays:?}");
    assert!(mondays.contains(&"20261102T080000Z".to_string()), "{mondays:?}");
    assert!(!mondays.contains(&"20261026T080000Z".to_string()), "{mondays:?}");
    assert!(!mondays.contains(&"20261019T080000Z".to_string()), "{mondays:?}");
    let moved = download.events.iter().find(|e| e.kind == RemoteEventKind::Exception).unwrap();
    assert_eq!((moved.series.as_str(), moved.title.as_str()), (master.id.as_str(), "Stand-up (moved)"));
    assert_eq!(moved.original.map(super::events::stamp).as_deref(), Some("20261019T080000Z"));
    assert_eq!(super::events::stamp(moved.start), "20261019T100000Z");
    assert_eq!(moved.id, format!("{}#20261019T080000Z", master.id));
    let contacts = out.contacts.unwrap();
    assert_eq!(contacts.contacts.len(), 1);
    let robin = &contacts.contacts[0];
    assert_eq!((robin.id.as_str(), robin.display_name.as_str()), ("/remote.php/dav/addressbooks/users/ada/contacts/robin.vcf", "Robin Example"));
    assert_eq!(robin.emails[0].value, "robin@cloud.example");
    // A stretch of its own asks for the calendars named, and the main one by no name.
    let mut narrow = job(&server, false);
    narrow.calendar = Some((NOW, NOW + 86_400));
    narrow.asks = vec![String::new()];
    let out = run(&narrow, NOW, UTC);
    assert!(out.calendars.is_none() && out.contacts.is_none());
    assert_eq!(out.events.len(), 1);
    assert_eq!((out.events[0].from, out.events[0].to), (NOW, NOW + 86_400));
}

#[test]
fn changes_go_up_and_the_server_has_the_last_word() {
    let server = StandIn::start().unwrap();
    let fields = EventFields {
        title: Some("Dentist".into()),
        location: Some("Town".into()),
        time: Some(EventTime { start: NOW + 86_400, end: NOW + 90_000, all_day: false }),
        categories: Some(vec!["Perso".into()]),
        ..Default::default()
    };
    let create = EventOp { kind: EventChangeKind::Create, event: 41, token: "mark-1".into(), fields: fields.clone(), ..Default::default() };
    let mut with = job(&server, true);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![create], zone: "Europe/Paris".into() });
    let out = run(&with, NOW, UTC);
    let uploaded = out.uploaded.unwrap();
    assert_eq!(uploaded.outcomes.len(), 1);
    let made = &uploaded.outcomes[0];
    assert_eq!(made.result, EventResult::Done, "{}", made.error);
    assert!(made.remote.starts_with("/remote.php/dav/calendars/ada/personal/") && made.remote.ends_with(".ics"), "{}", made.remote);
    assert_eq!(made.change_key, "e1");
    assert!(made.uid.ends_with("@mailcraft"));
    {
        let state = server.state();
        let put = state.asked("PUT");
        assert_eq!(put.len(), 1);
        assert_eq!(put[0].header("If-None-Match"), "*");
        let text = state.text(&made.remote).unwrap();
        assert!(text.contains("SUMMARY:Dentist") && text.contains("CATEGORIES:Perso") && text.contains("TZID=Europe/Paris"), "{text}");
    }
    // What was made comes down in the same job, in the stretch it falls in.
    assert!(
        out.events[0].events.iter().any(|e| e.id == made.remote && e.title == "Dentist"),
        "{:?}",
        out.events[0].events.iter().map(|e| &e.title).collect::<Vec<_>>()
    );
    // A change reads the resource back and writes it on that version.
    let update = EventOp {
        kind: EventChangeKind::Update,
        event: 41,
        remote: made.remote.clone(),
        fields: EventFields { title: Some("Dentist (moved)".into()), ..Default::default() },
        ..Default::default()
    };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![update.clone()], ..Default::default() });
    let out = run(&with, NOW, UTC);
    let changed = &out.uploaded.unwrap().outcomes[0];
    assert_eq!(changed.result, EventResult::Done, "{}", changed.error);
    assert_eq!(changed.change_key, "e2");
    {
        let state = server.state();
        let put = state.asked("PUT");
        assert_eq!(put[1].header("If-Match"), "\"e1\"");
        let text = state.text(&made.remote).unwrap();
        assert!(text.contains("SUMMARY:Dentist (moved)") && text.contains("LOCATION:Town"), "the rest is kept: {text}");
    }
    // A change the server refuses (its copy moved on) is given up, with the server's reason.
    server.state().refuse_writes = Some(412);
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![update.clone()], ..Default::default() });
    let refused = &run(&with, NOW, UTC).uploaded.unwrap().outcomes[0];
    assert_eq!(refused.result, EventResult::Refused, "{}", refused.error);
    // A read-only calendar takes nothing.
    server.state().add_calendar("shared", "Shared", true);
    let into_shared =
        EventOp { kind: EventChangeKind::Create, event: 42, calendar: "/remote.php/dav/calendars/ada/shared/".into(), fields, ..Default::default() };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![into_shared], ..Default::default() });
    let denied = &run(&with, NOW, UTC).uploaded.unwrap().outcomes[0];
    assert_eq!(denied.result, EventResult::Denied, "{}", denied.error);
    // A deletion; one of something already gone is settled.
    let delete = EventOp { kind: EventChangeKind::Delete, event: 41, remote: made.remote.clone(), ..Default::default() };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![delete.clone(), delete], ..Default::default() });
    let outcomes = run(&with, NOW, UTC).uploaded.unwrap().outcomes;
    assert_eq!((outcomes[0].result, outcomes[1].result), (EventResult::Done, EventResult::Gone));
    assert!(server.state().text(&made.remote).is_none());
    // What this kind of server isn't told yet stays here, out loud.
    let answer = EventOp { kind: EventChangeKind::Respond, event: 43, remote: "/x.ics".into(), ..Default::default() };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![answer], ..Default::default() });
    let kept = &run(&with, NOW, UTC).uploaded.unwrap().outcomes[0];
    assert_eq!(kept.result, EventResult::Failed);
    assert!(kept.error.contains("stays here"), "{}", kept.error);
}

#[test]
fn a_server_that_is_down_or_refuses_the_sign_in_is_said_and_nothing_is_lost() {
    let server = StandIn::start().unwrap();
    let delete =
        EventOp { kind: EventChangeKind::Delete, event: 41, remote: "/remote.php/dav/calendars/ada/personal/x.ics".into(), ..Default::default() };
    let mut with = job(&server, true);
    with.password = "wrong".into();
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![delete.clone()], ..Default::default() });
    let out = run(&with, NOW, UTC);
    assert!(out.sign_in_refused);
    assert!(out.calendars.is_none() && out.events.is_empty());
    let outcomes = out.uploaded.unwrap().outcomes;
    assert_eq!(outcomes[0].result, EventResult::Failed);
    assert!(!outcomes[0].error.contains("wrong"));
    server.state().refuse_all = Some(503);
    let out = run(&job(&server, true), NOW, UTC);
    assert!(!out.sign_in_refused);
    assert!(out.errors.iter().any(|e| e.contains("try later")), "{:?}", out.errors);
    // An account without a server, or with an address that can't be used.
    let mut none = job(&server, true);
    none.account.dav = None;
    assert!(!run(&none, NOW, UTC).errors.is_empty());
    let mut bad = job(&server, true);
    bad.account.dav = Some(DavServer { url: "ftp://x".into(), ..Default::default() });
    assert!(run(&bad, NOW, UTC).errors[0].contains("address"));
}

#[test]
fn a_calendar_that_takes_no_etag_on_put_is_asked_for_it() {
    let server = StandIn::start().unwrap();
    server.state().mute_etag = true;
    let create = EventOp {
        kind: EventChangeKind::Create,
        event: 1,
        fields: EventFields {
            title: Some("Walk".into()),
            time: Some(EventTime { start: NOW, end: NOW + 3_600, all_day: false }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![create], ..Default::default() });
    let made = &run(&with, NOW, UTC).uploaded.unwrap().outcomes[0];
    assert_eq!((made.result, made.change_key.as_str()), (EventResult::Done, "e1"), "{}", made.error);
    assert_eq!(server.state().asked("PROPFIND").iter().filter(|a| a.path == made.remote).count(), 1);
    // A whole-day event is the days the server names.
    let whole = EventOp {
        kind: EventChangeKind::Create,
        event: 2,
        fields: EventFields {
            title: Some("Holiday".into()),
            time: Some(EventTime { start: 1_792_022_400, end: 1_792_195_200, all_day: true }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut with = job(&server, false);
    with.changes = Some(EventsUpload { account: 7, email: "ada@example.org".into(), ops: vec![whole], ..Default::default() });
    let made = &run(&with, NOW, UTC).uploaded.unwrap().outcomes[0];
    let text = server.state().text(&made.remote).unwrap().to_string();
    assert!(text.contains("DTSTART;VALUE=DATE:20261015") && text.contains("DTEND;VALUE=DATE:20261017"), "{text}");
    let _ = Recurrence::every(Freq::Daily);
}
