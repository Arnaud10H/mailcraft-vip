use mailcraft_model::{Address, FlagStatus, FolderKind, Id, Importance, Message};
use mailcraft_time::{Date, DateTime};
use serde_json::{Value, json};

use crate::*;

/// Wednesday 2026-10-14 16:00 UTC.
fn now() -> i64 {
    DateTime::new(Date::new(2026, 10, 14).unwrap(), 16, 0, 0).unwrap().to_timestamp(0)
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// The sample mailbox with a server account, whose id is the second value.
fn session() -> (Session, Id) {
    let mut s = Session::at(sample::mailbox(now(), 0), now(), 0);
    let a = run(
        &mut s,
        "account.add",
        json!({"email": "avery@mail.example", "incoming": {"host": "imap.mail.example"}, "outgoing": {"host": "smtp.mail.example"}}),
    )["id"]
        .as_u64()
        .unwrap();
    (s, a)
}

/// `m` arrives in the Inbox of account `a`, as a synchronisation brings it.
fn arrive(s: &mut Session, a: Id, m: Message) -> Id {
    let inbox = s.mailbox.folder_of_kind(a, FolderKind::Inbox).unwrap();
    let id = s.mailbox.add_message(Message { account: a, folder: inbox, date: now(), received: now(), ..m }).unwrap();
    super::vip::arrived(&mut s.mailbox, id);
    id
}

fn from(email: &str, subject: &str) -> Message {
    Message {
        from: Address { name: String::new(), email: email.into() },
        subject: subject.into(),
        message_id: format!("{subject}@example.org"),
        ..Default::default()
    }
}

fn outbox(s: &Session, a: Id) -> Vec<Message> {
    let o = s.mailbox.folder_of_kind(a, FolderKind::Outbox).unwrap();
    s.mailbox.messages_in(o).map(|m| m.as_ref().clone()).collect()
}

#[test]
fn vip_senders_are_marked_now_and_as_mail_arrives() {
    let (mut s, a) = session();
    let some = s.mailbox.messages.values().next().unwrap().from.email.clone();
    let r = run(&mut s, "vip.add", json!({"email": some.to_uppercase()}));
    assert!(r["tagged"].as_u64().unwrap() >= 1);
    assert!(
        s.mailbox
            .messages
            .values()
            .filter(|m| m.from.email.eq_ignore_ascii_case(&some))
            .all(|m| m.has_category("VIP") && m.importance == Importance::High)
    );
    run(&mut s, "vip.add", json!({"email": "boss@example.org"}));
    let id = arrive(&mut s, a, from("Boss@Example.org", "Budget"));
    assert!(s.mailbox.message(id).unwrap().has_category("VIP"));
    assert_eq!(run(&mut s, "vip.list", json!({}))["senders"].as_array().unwrap().len(), 2);
    run(&mut s, "vip.remove", json!({"email": "boss@example.org"}));
    assert!(!s.mailbox.message(id).unwrap().has_category("VIP"));
    assert!(s.run("vip.add", &json!({"email": "not an address"})).is_err());
}

#[test]
fn snoozed_mail_comes_back_unread_on_top() {
    let (mut s, _) = session();
    let rows = s.rows();
    let last = rows.last().unwrap().id;
    run(&mut s, "mail.snooze", json!({"id": last, "minutes": 90}));
    assert!(!s.rows().iter().any(|r| r.id == last));
    s.view.search = "is:snoozed".into();
    assert_eq!(s.rows().iter().map(|r| r.id).collect::<Vec<_>>(), vec![last]);
    assert!(s.run("mail.snooze", &json!({"id": last, "until": "2020-01-01T08:00"})).is_err());
    let mut later = Session::at(s.mailbox.clone(), now() + 2 * 3600, 0);
    let top = later.rows().iter().find(|r| !s.mailbox.message(r.id).unwrap().pinned).unwrap().id;
    assert_eq!(top, last);
    assert!(!later.mailbox.message(last).unwrap().read);
}

#[test]
fn delayed_mail_waits_and_follow_ups_end_with_a_reply() {
    let (mut s, a) = session();
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Proposal", "account": a}));
    run(&mut s, "compose.delay", json!({"minutes": 120}));
    run(&mut s, "compose.followUp", json!({"days": 3}));
    run(&mut s, "compose.send", json!({}));
    assert!(cmd::account::sync_request(&s, a).unwrap().outbox.is_empty());
    let later = Session::at(s.mailbox.clone(), now() + 3 * 3600, 0);
    assert_eq!(cmd::account::sync_request(&later, a).unwrap().outbox.len(), 1);
    let sent = outbox(&s, a).into_iter().next().unwrap();
    assert!(sent.awaiting_reply && sent.flag == FlagStatus::Flagged);
    assert_eq!(sent.flag_due, Some(now() + 3 * 86_400));
    assert_eq!(run(&mut s, "followup.list", json!({}))["awaiting"].as_array().unwrap().len(), 1);
    // Someone else's mail on another topic ends nothing; the answer does.
    arrive(&mut s, a, from("other@example.org", "Lunch"));
    assert!(s.mailbox.message(sent.id).unwrap().awaiting_reply);
    arrive(&mut s, a, Message { in_reply_to: sent.message_id.clone(), ..from("maya@lumen.example", "Re: Proposal") });
    let done = s.mailbox.message(sent.id).unwrap();
    assert!(!done.awaiting_reply && done.flag == FlagStatus::Complete);
}

#[test]
fn templates_fill_a_message() {
    let (mut s, _) = session();
    assert!(s.run("template.save", &json!({"name": "Thanks"})).is_err());
    run(&mut s, "template.save", json!({"name": "Thanks", "subject": "Merci", "body": "Merci pour votre message."}));
    run(&mut s, "template.save", json!({"name": "thanks", "subject": "Merci", "body": "Merci beaucoup."}));
    assert_eq!(s.mailbox.vip.templates.len(), 1);
    run(&mut s, "mail.new", json!({}));
    run(&mut s, "insert.quickParts", json!({"name": "THANKS"}));
    let c = s.compose.as_ref().unwrap();
    assert_eq!(c.subject, "Merci");
    assert!(c.body.plain().contains("Merci beaucoup."));
    assert!(s.run("insert.quickParts", &json!({"name": "nope"})).is_err());
    run(&mut s, "template.remove", json!({"name": "Thanks"}));
    assert!(s.mailbox.vip.templates.is_empty());
}

#[test]
fn automatic_replies_answer_each_sender_once() {
    let (mut s, a) = session();
    assert!(s.run("vip.automaticReplies", &json!({"enabled": true})).is_err());
    run(&mut s, "vip.automaticReplies", json!({"enabled": true, "message": "Absent jusqu'au 3 novembre."}));
    arrive(&mut s, a, from("ada@example.org", "Question"));
    arrive(&mut s, a, from("ada@example.org", "Question 2"));
    arrive(&mut s, a, Message { headers: vec![("List-Id".into(), "<news.example.org>".into())], ..from("news@example.org", "Weekly") });
    let out = outbox(&s, a);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].to[0].email, "ada@example.org");
    assert_eq!(out[0].subject, "Re: Question");
    assert_eq!(out[0].in_reply_to, "Question@example.org");
    assert!(out[0].headers.iter().any(|(n, v)| n == "Auto-Submitted" && v == "auto-replied"));
    assert_eq!(run(&mut s, "vip.automaticReplies", json!({"enabled": false}))["enabled"], false);
}

#[test]
fn unsubscribing_by_mail_opens_a_message() {
    let (mut s, a) = session();
    let id = arrive(
        &mut s,
        a,
        Message {
            headers: vec![("List-Unsubscribe".into(), "<mailto:leave@news.example?subject=stop>".into())],
            ..from("news@news.example", "Weekly")
        },
    );
    let r = run(&mut s, "mail.unsubscribe", json!({"id": id}));
    assert_eq!(r["mailto"], "leave@news.example?subject=stop");
    let c = s.compose.as_ref().unwrap();
    assert!(c.to.contains("leave@news.example"));
    assert_eq!(c.subject, "stop");
    let plain = arrive(&mut s, a, from("ada@example.org", "Hello"));
    assert!(s.run("mail.unsubscribe", &json!({"id": plain})).is_err());
}
