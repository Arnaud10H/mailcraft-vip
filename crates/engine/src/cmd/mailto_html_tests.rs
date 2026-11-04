// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! A `mailto:` link that carries a message written in HTML elsewhere (`html`): the new message
//! starts with it as `compose.html` would put it in, and is sent as written, cleaned.

use mailcraft_time::{Date, DateTime};
use serde_json::{Value, json};

use crate::{Session, sample};

fn session() -> Session {
    let now = DateTime::new(Date::new(2026, 11, 4).unwrap(), 9, 0, 0).unwrap().to_timestamp(0);
    Session::at(sample::mailbox(now, 0), now, 0)
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

fn enc(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() { char::from(b).to_string() } else { format!("%{b:02X}") }).collect()
}

const HTML: &str = r#"<table width="600"><tr><td style="background:#0b3a6e;color:#fff"><h1>Faille PHP</h1><p>Bonjour Maya,</p><img src="https://radar.example/assets/mail/hero.jpg" alt="Alerte"></td></tr></table><script>alert(1)</script><img src="https://tracker.example/p.gif" width="1" height="1">"#;

#[test]
fn a_link_with_html_starts_a_message_written_in_html_and_sends_it_cleaned() {
    let mut s = session();
    run(&mut s, "signature.set", json!({"name": "Work", "text": "Avery Quinn", "default": true}));
    let link = format!("mailto:maya@lumen.example?subject=Faille%20PHP&body={}&html={}", enc("Bonjour Maya, version texte"), enc(HTML));
    let out = run(&mut s, "mail.new", json!({"mailto": link}));
    assert!(out.get("written").is_none(), "{out}");
    let c = s.compose.as_ref().unwrap();
    assert_eq!((c.to.as_str(), c.subject.as_str()), ("maya@lumen.example", "Faille PHP"));
    assert!(c.written.as_ref().is_some_and(|w| w.text.text().contains("Bonjour Maya")));
    // The plain-text stand-in gives way to the HTML; the signature stays after it.
    assert!(!c.body.text().text().contains("version texte"), "{:?}", c.body.text().text());
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let m = s.mailbox.message(id).unwrap();
    let html = m.body.html.clone().unwrap();
    assert!(html.contains("<table") && html.contains("#0b3a6e") && html.contains("https://radar.example/assets/mail/hero.jpg"), "{html}");
    assert!(!html.contains("alert(1)") && !html.contains("tracker.example"), "{html}");
    assert!(html.contains("Avery Quinn") && m.body.text.contains("Bonjour Maya"), "{html}");
}

#[test]
fn html_named_by_the_caller_comes_before_the_links() {
    let mut s = session();
    let link = format!("mailto:a@lumen.example?html={}", enc("<p>from the link</p>"));
    run(&mut s, "mail.new", json!({"mailto": link, "html": "<p>from the caller</p>"}));
    let w = s.compose.as_ref().unwrap().written.as_ref().unwrap();
    assert!(w.text.text().contains("from the caller") && !w.text.text().contains("from the link"));
}

#[test]
fn html_that_cant_go_in_leaves_the_plain_message() {
    let mut s = session();
    let link = format!("mailto:a@lumen.example?body=texte&html={}", enc(r#"<img src="data:image/png;base64,!!!">"#));
    let out = run(&mut s, "mail.new", json!({"mailto": link}));
    assert_eq!(out["written"], false, "{out}");
    let c = s.compose.as_ref().unwrap();
    assert!(c.written.is_none() && c.body.text().text().contains("texte"));
}

#[test]
fn a_link_without_html_is_a_plain_message_as_before() {
    let mut s = session();
    run(&mut s, "mail.new", json!({"mailto": "mailto:a@lumen.example?body=Bonjour"}));
    let c = s.compose.as_ref().unwrap();
    assert!(c.written.is_none() && c.body.text().text().contains("Bonjour"));
}
