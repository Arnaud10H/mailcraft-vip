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
    // Shown whole below an empty text to write in, the signature after it; nothing of it in the text.
    let c = s.compose.as_ref().unwrap();
    assert!(c.written.as_ref().is_some_and(|w| w.text.text().contains("Bonjour Maya")));
    assert!(c.body.text().text().trim().is_empty() || c.body.text().signature().is_some_and(|sig| sig.text().contains("Avery Quinn")), "{:?}", c.body.text().text());
    run(&mut s, "compose.select", json!({"from": 0}));
    run(&mut s, "compose.type", json!({"text": "Voici notre offre."}));
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
    // The owner's words first, then the HTML.
    let (mine, theirs) = (html.find("Voici notre offre").unwrap(), html.find("Offre de novembre").unwrap());
    assert!(mine < theirs, "{html}");
}

#[test]
fn turned_into_text_it_is_sent_as_that_text() {
    let mut s = session();
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Offre"}));
    run(&mut s, "compose.html", json!({"html": newsletter()}));
    run(&mut s, "compose.html", json!({"html": null}));
    let c = s.compose.as_ref().unwrap();
    assert!(c.written.is_none() && c.body.text().text().contains("Bonjour Maya") && c.body.text().pictures().len() == 1);
    run(&mut s, "compose.select", json!({"from": 0}));
    run(&mut s, "compose.type", json!({"text": "PS "}));
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let html = s.mailbox.message(id).unwrap().body.html.clone().unwrap();
    assert!(html.contains("PS") && html.contains("Bonjour Maya") && !html.contains("#293c67"), "{html}");
}

#[test]
fn null_lets_the_html_go_and_junk_changes_nothing() {
    let mut s = session();
    run(&mut s, "mail.new", json!({}));
    assert!(s.run("compose.html", &json!({"html": r#"<img src="data:image/png;base64,!!!">"#})).is_err());
    assert!(s.compose.as_ref().unwrap().written.is_none() && s.compose.as_ref().unwrap().pictures.is_empty());
    run(&mut s, "compose.html", json!({"html": "<p><b>Hi</b></p>"}));
    assert!(s.compose.as_ref().unwrap().written.is_some());
    run(&mut s, "compose.html", json!({"html": null, "remove": true}));
    assert!(s.compose.as_ref().unwrap().written.is_none() && !s.compose.as_ref().unwrap().body.text().text().contains("Hi"));
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
    assert_eq!(c.written.as_ref().unwrap().text.text().matches("Avery Quinn").count(), 1);
    assert_eq!(c.pictures[0].width, Some(15));
    // Asked to, it keeps it.
    run(&mut s, "mail.new", json!({"subject": "Offre"}));
    run(&mut s, "compose.html", json!({"html": html, "signature": true}));
    assert!(s.compose.as_ref().unwrap().body.text().signature().is_some());
}

#[test]
fn a_draft_written_elsewhere_is_sent_as_written() {
    use mailcraft_model::{Body, FolderKind, Message};
    let mut s = session();
    let account = s.mailbox.default_account().unwrap().id;
    let drafts = s.mailbox.folder_of_kind(account, FolderKind::Drafts).unwrap();
    // An .eml brought in: its HTML laid out with tables, as no rich text writes it.
    let html = r#"<!doctype html><html><head><meta charset="utf-8"></head><body><table style="background:#16202e"><tr><td style="border-radius:14px">Ce qui presse</td></tr></table></body></html>"#;
    let draft = Message { account, folder: drafts, draft: true, subject: "Feuille de route".into(), body: Body { text: "Ce qui presse".into(), html: Some(html.into()) }, ..Default::default() };
    let draft = s.mailbox.add_message(draft).unwrap();
    run(&mut s, "mail.editDraft", json!({"id": draft}));
    assert!(s.compose.as_ref().unwrap().written.is_some() && !s.compose.as_ref().unwrap().dirty);
    run(&mut s, "compose.set", json!({"to": "stan@example.org"}));
    let id = run(&mut s, "compose.send", json!({}))["id"].as_u64().unwrap();
    let sent = s.mailbox.message(id).unwrap().body.html.clone().unwrap();
    assert!(sent.contains("<table") && sent.contains("#16202e") && sent.contains("border-radius"), "{sent}");
    // A draft written here still opens as text to edit.
    run(&mut s, "mail.new", json!({"to": "maya@lumen.example", "subject": "Ici"}));
    run(&mut s, "compose.type", json!({"text": "Bonjour"}));
    let ours = run(&mut s, "compose.saveDraft", json!({}))["draft"].as_u64().unwrap();
    run(&mut s, "compose.close", json!({}));
    run(&mut s, "mail.editDraft", json!({"id": ours}));
    assert!(s.compose.as_ref().unwrap().written.is_none());
}
