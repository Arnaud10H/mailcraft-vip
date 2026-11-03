// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! A message written in HTML elsewhere (`compose.html`): sent as written while unedited, its
//! pictures written in going with it.

use mailcraft_time::{Date, DateTime};
use serde_json::{Value, json};

use crate::{Session, sample};

fn session() -> Session {
    let now = DateTime::new(Date::new(2026, 10, 14).unwrap(), 16, 0, 0).unwrap().to_timestamp(0);
    Session::at(sample::mailbox(now, 0), now, 0)
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// A newsletter as a tool makes one: a two-column table, a logo written in, a picture on the web.
fn newsletter() -> String {
    let png = mailcraft_charset::base64::STANDARD.encode(b"\x89PNG\r\n\x1a\nIHDR....");
    format!(
        r#"<html><head><style>p {{ color: red }}</style></head><body><table width="600"><tr><td style="background:#293c67"><img src="data:image/png;base64,{png}" alt="Logo"></td><td><h1 style="color:#1d71b8">Offre de novembre</h1><p>Bonjour Maya,</p><img src="https://branding.example/banner.png"></td></tr></table><script>alert(1)</script></body></html>"#
    )
}

#[test]
fn html_written_elsewhere_is_sent_as_written_with_its_pictures() {
    let mut s = session();
    run(&mut s, "signature.set", json!({"name": "Work", "text": "Avery Quinn", "default": true}));
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Offre"}));
    let out = run(&mut s, "compose.html", json!({"html": newsletter()}));
    assert_eq!(out["pictures"], 1);
    // The window shows its words and its picture, then the signature.
    let c = s.compose.as_ref().unwrap();
    let text = c.body.text();
    assert_eq!(text.pictures().len(), 1);
    assert!(text.text().contains("Offre de novembre") && text.text().contains("Bonjour Maya"), "{:?}", text.text());
    assert!(text.signature().is_some_and(|sig| sig.text().contains("Avery Quinn")));
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let m = s.mailbox.message(id).unwrap();
    let html = m.body.html.clone().unwrap();
    // As written: the table and its colours, the picture on the web; nothing that runs.
    assert!(html.contains("<table") && html.contains("#293c67") && html.contains("https://branding.example/banner.png"), "{html}");
    assert!(!html.contains("alert(1)") && !html.contains("data:") && !html.contains(crate::WRITTEN_PICTURE_CID), "{html}");
    // The logo a part of the message, which the HTML names; the signature after it.
    assert_eq!(m.attachments.len(), 1);
    let part = &m.attachments[0];
    assert!(part.inline && part.mime == "image/png", "{part:?}");
    assert!(html.contains(&format!("cid:{}", part.content_id)) && html.contains("Avery Quinn"), "{html}");
    assert!(m.body.text.contains("Bonjour Maya"), "{:?}", m.body.text);
}

#[test]
fn edited_here_it_is_sent_as_the_text_the_window_shows() {
    let mut s = session();
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Offre"}));
    run(&mut s, "compose.html", json!({"html": newsletter()}));
    run(&mut s, "compose.select", json!({"from": 0}));
    run(&mut s, "compose.type", json!({"text": "PS "}));
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let html = s.mailbox.message(id).unwrap().body.html.clone().unwrap();
    assert!(html.contains("PS ") && !html.contains("#293c67"), "{html}");
}

#[test]
fn null_lets_the_html_go_and_junk_changes_nothing() {
    let mut s = session();
    run(&mut s, "mail.new", json!({}));
    assert!(s.run("compose.html", &json!({"html": r#"<img src="data:image/png;base64,!!!">"#})).is_err());
    assert!(s.compose.as_ref().unwrap().written.is_none() && s.compose.as_ref().unwrap().pictures.is_empty());
    run(&mut s, "compose.html", json!({"html": "<p><b>Hi</b></p>"}));
    assert!(s.compose.as_ref().unwrap().written.is_some());
    run(&mut s, "compose.html", json!({"html": null}));
    assert!(s.compose.as_ref().unwrap().written.is_none());
    assert!(s.run("compose.html", &json!({})).is_err());
}

#[test]
fn its_draft_opens_again_as_written() {
    let mut s = session();
    run(&mut s, "signature.set", json!({"name": "Work", "text": "Avery Quinn", "default": true}));
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Offre"}));
    run(&mut s, "compose.html", json!({"html": newsletter()}));
    let draft = run(&mut s, "compose.saveDraft", json!({}))["draft"].as_u64().unwrap();
    run(&mut s, "compose.close", json!({}));
    run(&mut s, "mail.editDraft", json!({"id": draft}));
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let m = s.mailbox.message(id).unwrap();
    let html = m.body.html.clone().unwrap();
    assert!(html.contains("#293c67") && html.contains("<table") && html.contains("Avery Quinn"), "{html}");
    assert!(!html.contains(crate::WRITTEN_PICTURE_CID) && html.matches("<body").count() == 1, "{html}");
    let inline: Vec<_> = m.attachments.iter().filter(|a| a.inline).collect();
    assert_eq!(inline.len(), 1, "{:?}", m.attachments);
    assert!(html.contains(&format!("cid:{}", inline[0].content_id)), "{html}");
}

#[test]
fn html_that_ends_with_the_signature_takes_its_place_and_pictures_keep_their_width() {
    let mut s = session();
    run(&mut s, "signature.set", json!({"name": "Work", "text": "Avery Quinn\nFounder", "default": true}));
    run(&mut s, "mail.new", json!({"subject": "Offre"}));
    let png = mailcraft_charset::base64::STANDARD.encode(b"\x89PNG\r\n\x1a\nIHDR....");
    let html = format!(r#"<p>Hello</p><table><tr><td><img src="data:image/png;base64,{png}" width="15"></td><td><b>Avery Quinn</b><br>Founder</td></tr></table>"#);
    run(&mut s, "compose.html", json!({"html": html}));
    let c = s.compose.as_ref().unwrap();
    assert!(c.signature.is_none() && c.body.text().signature().is_none());
    assert_eq!(c.body.text().text().matches("Avery Quinn").count(), 1);
    assert_eq!(c.pictures[0].width, Some(15));
    // Asked to, it keeps it.
    run(&mut s, "mail.new", json!({"subject": "Offre"}));
    run(&mut s, "compose.html", json!({"html": html, "signature": true}));
    assert!(s.compose.as_ref().unwrap().body.text().signature().is_some());
}
