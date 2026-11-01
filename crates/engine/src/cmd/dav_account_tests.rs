// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! A calendar and contacts account (CalDAV/CardDAV, Nextcloud and the like): made with its server's
//! address, no mail of its own, and its calendars, events and contacts taken in as any server's are.

use mailcraft_model::remote::{CalendarsDownload, ContactsDownload, EventsDownload, RemoteCalendar, RemoteContact, RemoteEvent};
use mailcraft_model::{AccountKind, Rights};
use serde_json::json;

use super::*;

const ME: &str = "ada@example.org";
const URL: &str = "https://cloud.example.org";
const PERSONAL: &str = "/remote.php/dav/calendars/ada/personal/";
const WORK: &str = "/remote.php/dav/calendars/ada/work/";

/// A session with one calendar and contacts account; its id.
fn session() -> (Session, Id) {
    let mut s = Session::empty();
    let added = s.run("account.add", &json!({"email": ME, "kind": "dav", "dav": {"url": URL, "username": "ada"}})).unwrap();
    (s, added["id"].as_u64().unwrap())
}

#[test]
fn a_dav_account_is_made_from_its_address_and_has_no_mail() {
    let (mut s, a) = session();
    let account = s.mailbox.account(a).unwrap().clone();
    assert_eq!(account.kind, AccountKind::Dav);
    assert_eq!(account.dav.as_ref().map(|d| (d.url.as_str(), d.username.as_str())), Some((URL, "ada")));
    assert!(account.incoming.is_none() && account.outgoing.is_none() && account.oauth.is_none());
    assert!(!account.kind.has_server() && account.kind.is_dav());
    // It is listed with its server's name and nothing more of the address.
    let listed = s.run("account.list", &json!({})).unwrap();
    let mine = listed["accounts"].as_array().unwrap().iter().find(|x| x["id"] == a).unwrap().clone();
    assert_eq!(mine["kind"], "dav");
    assert_eq!(mine["dav"], json!({"server": "cloud.example.org", "username": "ada"}));
    // No mail run is asked for it.
    assert!(s.run("sync.request", &json!({"account": a})).is_err());
    // The settings are checked as text, and nothing else is kept of the address.
    for bad in [json!({"url": "http://cloud.example.org"}), json!({"url": ""}), json!({"url": URL, "username": "a:b"}), json!("x"), json!({"url": 5})]
    {
        assert!(s.run("account.add", &json!({"email": "other@example.org", "kind": "dav", "dav": bad})).is_err(), "{bad}");
    }
    assert!(s.run("account.add", &json!({"email": "other@example.org", "kind": "dav"})).is_err(), "a server is wanted");
    // Edited: the user name changes, the address stays; another kind drops the server.
    s.run("account.edit", &json!({"id": a, "dav": {"username": "ada.lovelace"}})).unwrap();
    let edited = s.mailbox.account(a).unwrap().clone();
    assert_eq!(edited.dav.as_ref().map(|d| (d.url.as_str(), d.username.as_str())), Some((URL, "ada.lovelace")));
    assert_eq!(edited.kind, AccountKind::Dav);
    s.run("account.edit", &json!({"id": a, "kind": "local"})).unwrap();
    let local = s.mailbox.account(a).unwrap().clone();
    assert!(local.dav.is_none() && local.kind == AccountKind::Local);
    // Other names for the kind are taken too.
    let nc = s.run("account.add", &json!({"email": "nc@example.org", "kind": "nextcloud", "dav": {"url": URL}})).unwrap();
    let nc = s.mailbox.account(nc["id"].as_u64().unwrap()).unwrap().clone();
    assert_eq!(nc.kind, AccountKind::Dav);
    assert_eq!(nc.dav.as_ref().map(|d| d.login(&nc.email)), Some("nc@example.org"), "the address signs in when no name is given");
}

#[test]
fn its_calendars_events_and_contacts_come_in_as_any_servers_do() {
    let (mut s, a) = session();
    let account = s.mailbox.account(a).unwrap().clone();
    let calendars = CalendarsDownload {
        complete: true,
        calendars: vec![
            RemoteCalendar { id: PERSONAL.into(), name: "Personal".into(), rights: Rights::Edit, main: true, ..Default::default() },
            RemoteCalendar { id: WORK.into(), name: "Work".into(), rights: Rights::Read, ..Default::default() },
        ],
        ..Default::default()
    }
    .for_account(&account);
    let listed = remote::apply_calendars(&mut s, &calendars).unwrap();
    assert_eq!(listed["added"], 2, "{listed}");
    let personal = s.mailbox.calendars.iter().find(|c| c.origin.account == a && c.origin.id.is_empty()).unwrap().clone();
    let work = s.mailbox.calendars.iter().find(|c| c.origin.id == WORK).unwrap().clone();
    assert_eq!((personal.name.as_str(), personal.rights.can_edit()), ("Personal", true));
    assert_eq!((work.name.as_str(), work.rights.can_edit()), ("Work", false));
    let event = |id: &str, title: &str, start: i64| RemoteEvent {
        id: id.into(),
        change_key: "e1".into(),
        uid: format!("{title}@example.org"),
        title: title.into(),
        start,
        end: start + 3_600,
        ..Default::default()
    };
    let now = s.now();
    let events = EventsDownload {
        from: now - 86_400,
        to: now + 86_400 * 30,
        complete: true,
        has_categories: true,
        events: vec![event("/p/one.ics", "Point chantier", now + 3_600)],
        ..Default::default()
    }
    .for_account(&account);
    remote::apply_events(&mut s, &events).unwrap();
    let here = s.mailbox.events.values().find(|e| e.title == "Point chantier").unwrap().clone();
    assert_eq!((here.calendar, here.origin.account, here.origin.id.as_str(), here.origin.change_key.as_str()), (personal.id, a, "/p/one.ics", "e1"));
    // A change here to the account's own calendar is noted for the server; one to the read-only
    // calendar is refused.
    s.run("calendar.edit", &json!({"id": here.id, "title": "Point chantier (2)"})).unwrap();
    let changes = s.run("sync.eventChanges", &json!({"account": a})).unwrap();
    assert_eq!(changes["ops"].as_array().map(Vec::len), Some(1), "{changes}");
    assert_eq!(changes["ops"][0]["remote"], "/p/one.ics");
    let in_work = EventsDownload {
        calendar: WORK.into(),
        from: now - 86_400,
        to: now + 86_400 * 30,
        complete: true,
        events: vec![event("/w/two.ics", "Review", now + 7_200)],
        ..Default::default()
    }
    .for_account(&account);
    remote::apply_events(&mut s, &in_work).unwrap();
    let review = s.mailbox.events.values().find(|e| e.title == "Review").unwrap().clone();
    assert_eq!(review.calendar, work.id);
    assert!(s.run("calendar.delete", &json!({"id": review.id})).is_err(), "a read-only calendar refuses changes");
    // Contacts, with the server's name for each, and gone when the server no longer has them.
    let contact = RemoteContact { id: "/c/robin.vcf".into(), change_key: "e2".into(), display_name: "Robin Example".into(), ..Default::default() };
    remote::apply_contacts(&mut s, &ContactsDownload { complete: true, contacts: vec![contact], ..Default::default() }.for_account(&account))
        .unwrap();
    assert!(s.mailbox.contacts.values().any(|c| c.display_name == "Robin Example" && c.origin.id == "/c/robin.vcf"));
    remote::apply_contacts(&mut s, &ContactsDownload { complete: true, ..Default::default() }.for_account(&account)).unwrap();
    assert!(!s.mailbox.contacts.values().any(|c| c.display_name == "Robin Example"));
    // Removing the account takes its calendars and what came from them with it.
    s.run("account.remove", &json!({"id": a})).unwrap();
    assert!(!s.mailbox.calendars.iter().any(|c| c.origin.account == a));
    assert!(!s.mailbox.events.values().any(|e| e.origin.account == a));
}
