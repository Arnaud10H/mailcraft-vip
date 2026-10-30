//! What VIP Mail brought: VIP senders, snoozing, follow-ups, delayed delivery, templates,
//! automatic replies on any server and leaving mailing lists.

use mailcraft_model::time::Timestamp;
use mailcraft_model::vip::{MAX_REPLIED, MAX_TEMPLATES, MAX_TEXT, MAX_VIPS, Template, Unsubscribe, VIP_CATEGORY};
use mailcraft_model::{Address, FlagStatus, FolderKind, Id, Importance, Mailbox, Message, Rgb, clean_name, normalize_subject};
use serde_json::{Value, json};

use super::{bad, fail, message_summary, needs_compose, needs_message, targets};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const DAY: i64 = 86_400;
/// Furthest ahead a snooze, a delay or a follow-up may go.
const MAX_AHEAD: i64 = 400 * DAY;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vip.add", "Add VIP", "Home › Tags", add)
            .params(r#"{"email"?: string, "id"?: number} (default: the sender of the selected message) — mail from them is marked VIP (category and high importance), now and as it arrives"#),
        CommandSpec::new("vip.remove", "Remove VIP", "Home › Tags", remove).params(r#"{"email"?: string, "id"?: number}"#),
        CommandSpec::new("vip.list", "VIP Senders", "Home › Tags", list).pure(),
        CommandSpec::new("mail.snooze", "Snooze", "Home › Tags", snooze)
            .params(r#"{"ids"?: [number], "id"?: number, "until"?: "2026-10-20T08:00", "minutes"?: number} — out of the list until then, back as unread and on top; find snoozed mail with `is:snoozed`"#)
            .selection(needs_message),
        CommandSpec::new("mail.unsnooze", "Unsnooze", "Home › Tags", unsnooze).params(r#"{"ids"?: [number], "id"?: number}"#).selection(needs_message),
        CommandSpec::new("compose.delay", "Delay Delivery", "Options › More Options", delay)
            .pure()
            .params(r#"{"until"?: "2026-10-20T08:00", "minutes"?: number} — kept in the Outbox until then; neither clears it"#)
            .when(needs_compose),
        CommandSpec::new("compose.followUp", "Follow Up If No Reply", "Message › Tags", follow_up)
            .pure()
            .params(r#"{"days": number} — flagged, due in that many days, until a reply comes in; 0 clears it"#)
            .when(needs_compose),
        CommandSpec::new("followup.list", "Awaiting Reply", "Home › Tags", followups).pure(),
        CommandSpec::new("template.save", "Save as Template", "Insert › Text", template_save)
            .params(r#"{"name": string, "subject"?: string, "body"?: string} (default: the subject and text of the message being written); a template of that name is replaced"#),
        CommandSpec::new("template.remove", "Delete Template", "Insert › Text", template_remove).params(r#"{"name": string}"#),
        CommandSpec::new("template.list", "Templates", "Insert › Text", template_list).pure(),
        CommandSpec::new("insert.quickParts", "Saved Text", "Insert › Text", insert_template)
            .pure()
            .params(r#"{"name": string} — the template's text at the cursor, and its subject when there is none yet"#)
            .when(needs_compose),
        CommandSpec::new("account.automaticReplies", "Automatic Replies", "File › Backstage", auto_reply).params(
            r#"{"enabled"?: bool, "message"?: string, "subject"?: string, "start"?: "2026-10-20T08:00"|null, "end"?: same} — answers once each sender of new Inbox mail while on (never lists, bulk or automatic mail); no params: what is set"#,
        ),
        CommandSpec::new("mail.unsubscribe", "Unsubscribe", "Home › Delete", unsubscribe)
            .pure()
            .params(r#"{"id"?: number} — the list's ways out (List-Unsubscribe): {"web", "mailto", "oneClick"}; a `mailto` one is opened as a new message to send"#)
            .selection(needs_message),
    ]
}

/// A time ahead: `until` (wall clock here, or with an offset) or `minutes` from now.
fn ahead(s: &Session, v: &Value) -> Result<Option<Timestamp>, CmdError> {
    let now = s.now();
    let t = if let Some(m) = p::u64(v, "minutes") {
        now.saturating_add(i64::try_from(m.min(MAX_AHEAD as u64 / 60)).unwrap_or(0).saturating_mul(60))
    } else {
        match p::str(v, "until").map(str::trim).filter(|t| !t.is_empty()) {
            None => return Ok(None),
            Some(t) => mailcraft_model::time::iso_to_timestamp(t, s.offset_at(now)).ok_or_else(|| bad("`until` looks like 2026-10-20T08:00"))?,
        }
    };
    if t <= now || t > now.saturating_add(MAX_AHEAD) {
        return Err(bad("that time is in the past or more than 400 days ahead"));
    }
    Ok(Some(t))
}

// ---- VIP senders ----

fn sender(s: &Session, v: &Value) -> Result<String, CmdError> {
    let email = match p::str(v, "email") {
        Some(e) => e.to_string(),
        None => {
            let id = p::id(v, "id").or(s.sel.focus()).ok_or_else(|| bad("`email` or a selected message is needed"))?;
            s.mailbox.message(id).ok_or_else(|| fail(format!("no message with id {id}")))?.from.email.clone()
        }
    };
    let email = email.trim().to_lowercase();
    if !(Address { name: String::new(), email: email.clone() }).is_valid() {
        return Err(bad(format!("\"{email}\" isn't an e-mail address")));
    }
    Ok(email)
}

/// Mark (or unmark) `email`'s mail VIP: the category, and high importance.
fn tag_from(mb: &mut Mailbox, email: &str, on: bool) -> usize {
    let ids: Vec<Id> = mb.messages.values().filter(|m| m.from.email.eq_ignore_ascii_case(email)).map(|m| m.id).collect();
    let mut n = 0;
    for id in ids {
        let Ok(m) = mb.message_mut(id) else { continue };
        if on && !m.has_category(VIP_CATEGORY) {
            m.categories.push(VIP_CATEGORY.into());
            m.importance = Importance::High;
            n += 1;
        } else if !on && m.has_category(VIP_CATEGORY) {
            m.categories.retain(|c| c != VIP_CATEGORY);
            m.importance = Importance::Normal;
            n += 1;
        }
    }
    n
}

fn add(s: &mut Session, v: &Value) -> CmdResult {
    let email = sender(s, v)?;
    if !s.mailbox.vip.is_vip(&email) {
        if s.mailbox.vip.senders.len() >= MAX_VIPS {
            return Err(fail("there are too many VIP senders"));
        }
        s.mailbox.vip.senders.push(email.clone());
    }
    if s.mailbox.category(VIP_CATEGORY).is_none() {
        s.mailbox.set_category(VIP_CATEGORY, Rgb(0xC9, 0x9A, 0x06))?;
    }
    let tagged = tag_from(&mut s.mailbox, &email, true);
    Ok(json!({"email": email, "tagged": tagged}))
}

fn remove(s: &mut Session, v: &Value) -> CmdResult {
    let email = sender(s, v)?;
    let before = s.mailbox.vip.senders.len();
    s.mailbox.vip.senders.retain(|e| *e != email);
    if s.mailbox.vip.senders.len() == before {
        return Err(fail(format!("{email} isn't a VIP sender")));
    }
    let untagged = tag_from(&mut s.mailbox, &email, false);
    Ok(json!({"email": email, "untagged": untagged}))
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    Ok(json!({"senders": s.mailbox.vip.senders}))
}

// ---- snoozing ----

fn snooze(s: &mut Session, v: &Value) -> CmdResult {
    let until = ahead(s, v)?.ok_or_else(|| bad("`until` or `minutes` is required"))?;
    let ids = targets(s, v)?;
    for id in &ids {
        let m = s.mailbox.message_mut(*id)?;
        m.snoozed_until = Some(until);
        m.read = false;
    }
    s.sel.messages.retain(|id| !ids.contains(id));
    Ok(json!({"snoozed": ids, "until": s.local(until).iso()}))
}

fn unsnooze(s: &mut Session, v: &Value) -> CmdResult {
    let ids = targets(s, v)?;
    for id in &ids {
        s.mailbox.message_mut(*id)?.snoozed_until = None;
    }
    Ok(json!({"unsnoozed": ids}))
}

// ---- delayed delivery and follow-ups ----

fn delay(s: &mut Session, v: &Value) -> CmdResult {
    let at = ahead(s, v)?;
    let iso = at.map(|t| s.local(t).iso());
    let c = s.compose.as_mut().ok_or_else(|| fail("no message is being written"))?;
    c.send_at = at;
    c.dirty = true;
    Ok(json!({"sendAt": iso}))
}

fn follow_up(s: &mut Session, v: &Value) -> CmdResult {
    let days = p::u64(v, "days").ok_or_else(|| bad("`days` (number) is required"))?;
    if days > 365 {
        return Err(bad("`days` is at most 365"));
    }
    let c = s.compose.as_mut().ok_or_else(|| fail("no message is being written"))?;
    c.follow_up_days = days as u32;
    c.dirty = true;
    Ok(json!({"days": days}))
}

/// What `compose.send` does with a message sent with a follow-up.
pub(crate) fn mark_follow_up(m: &mut Message, days: u32, now: Timestamp) {
    if days > 0 {
        m.awaiting_reply = true;
        m.flag = FlagStatus::Flagged;
        m.flag_due = Some(now.saturating_add(i64::from(days).saturating_mul(DAY)));
    }
}

fn followups(s: &mut Session, _: &Value) -> CmdResult {
    let now = s.now();
    let list: Vec<Value> = s
        .mailbox
        .messages
        .values()
        .filter(|m| m.awaiting_reply)
        .map(|m| {
            let mut j = message_summary(s, m);
            if let Some(o) = j.as_object_mut() {
                o.insert("due".into(), json!(m.flag_due.map(|t| s.local(t).iso())));
                o.insert("overdue".into(), json!(m.flag_due.is_some_and(|t| t <= now)));
            }
            j
        })
        .collect();
    Ok(json!({"awaiting": list}))
}

// ---- templates ----

fn template_save(s: &mut Session, v: &Value) -> CmdResult {
    let name = clean_name(p::req_str(v, "name")?).ok_or_else(|| bad("`name` is empty"))?;
    let (subject, body) = match (p::str(v, "subject"), p::str(v, "body"), s.compose.as_ref()) {
        (subj, Some(b), _) => (subj.unwrap_or("").to_string(), b.to_string()),
        (subj, None, Some(c)) => (subj.map_or_else(|| c.subject.clone(), str::to_string), c.body.plain()),
        (_, None, None) => return Err(bad("`body` is required when no message is being written")),
    };
    let body: String = body.chars().take(MAX_TEXT).collect();
    let subject: String = subject.replace(['\r', '\n'], " ").chars().take(1_000).collect();
    let list = &mut s.mailbox.vip.templates;
    if let Some(t) = list.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&name)) {
        t.subject = subject;
        t.body = body;
        return Ok(json!({"id": t.id, "replaced": true}));
    }
    if list.len() >= MAX_TEMPLATES {
        return Err(fail("there are too many templates"));
    }
    let id = s.mailbox.alloc_id();
    s.mailbox.vip.templates.push(Template { id, name, subject, body });
    Ok(json!({"id": id, "replaced": false}))
}

fn find_template<'a>(s: &'a Session, v: &Value) -> Result<&'a Template, CmdError> {
    let name = p::req_str(v, "name")?.trim();
    s.mailbox.vip.templates.iter().find(|t| t.name.eq_ignore_ascii_case(name)).ok_or_else(|| fail(format!("no template named \"{name}\"")))
}

fn template_remove(s: &mut Session, v: &Value) -> CmdResult {
    let id = find_template(s, v)?.id;
    s.mailbox.vip.templates.retain(|t| t.id != id);
    Ok(json!({"deleted": id}))
}

fn template_list(s: &mut Session, _: &Value) -> CmdResult {
    Ok(json!({"templates": s.mailbox.vip.templates}))
}

fn insert_template(s: &mut Session, v: &Value) -> CmdResult {
    let t = find_template(s, v)?.clone();
    let c = s.compose.as_mut().ok_or_else(|| fail("no message is being written"))?;
    if c.subject.trim().is_empty() {
        c.subject = t.subject;
    }
    c.body.type_text(&t.body).map_err(|e| fail(e.to_string()))?;
    c.dirty = true;
    Ok(json!({"inserted": t.id}))
}

// ---- automatic replies ----

fn auto_reply(s: &mut Session, v: &Value) -> CmdResult {
    let now = s.now();
    let offset = s.offset_at(now);
    let time = |k: &str| -> Result<Option<Option<Timestamp>>, CmdError> {
        match v.get(k) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(t)) if t.trim().is_empty() => Ok(Some(None)),
            Some(Value::String(t)) => mailcraft_model::time::iso_to_timestamp(t, offset)
                .map(|t| Some(Some(t)))
                .ok_or_else(|| bad(format!("`{k}` looks like 2026-10-20T08:00"))),
            Some(_) => Err(bad(format!("`{k}` looks like 2026-10-20T08:00"))),
        }
    };
    let (start, end) = (time("start")?, time("end")?);
    let r = &mut s.mailbox.vip.auto_reply;
    if let Some(m) = p::str(v, "message") {
        r.message = m.chars().take(MAX_TEXT).collect();
    }
    if let Some(subj) = p::str(v, "subject") {
        r.subject = subj.replace(['\r', '\n'], " ").chars().take(1_000).collect();
    }
    if let Some(t) = start {
        r.start = t;
    }
    if let Some(t) = end {
        r.end = t;
    }
    if let Some(on) = p::bool(v, "enabled") {
        if on && r.message.trim().is_empty() {
            return Err(bad("write the `message` to send first"));
        }
        if on && !r.enabled {
            r.since = now;
            r.replied.clear();
        }
        r.enabled = on;
    }
    let r = s.mailbox.vip.auto_reply.clone();
    Ok(json!({
        "enabled": r.enabled,
        "subject": r.subject,
        "message": r.message,
        "start": r.start.map(|t| s.local(t).iso()),
        "end": r.end.map(|t| s.local(t).iso()),
        "replied": r.replied.len(),
    }))
}

// ---- mailing lists ----

fn unsubscribe(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::id(v, "id").or(s.sel.focus()).ok_or_else(|| fail("select a message first"))?;
    let m = s.mailbox.message(id).ok_or_else(|| fail(format!("no message with id {id}")))?;
    let u = Unsubscribe::of(m).ok_or_else(|| fail("this message doesn't say how to leave its list"))?;
    let account = m.account;
    if u.web.is_none()
        && let Some(to) = &u.mailto
    {
        s.run("mail.new", &json!({"account": account, "mailto": format!("mailto:{to}")}))?;
    }
    Ok(serde_json::to_value(&u).unwrap_or_default())
}

// ---- mail arriving ----

/// What happens to a message that just came into an Inbox, before the rules: VIP marking, the
/// end of a follow-up it answers, and the automatic reply.
pub fn arrived(mb: &mut Mailbox, id: Id) {
    let Some(m) = mb.message(id).cloned() else { return };
    if mb.vip.is_vip(&m.from.email)
        && let Ok(x) = mb.message_mut(id)
        && !x.has_category(VIP_CATEGORY)
    {
        x.categories.push(VIP_CATEGORY.into());
        x.importance = Importance::High;
    }
    end_follow_up(mb, &m);
    auto_reply_to(mb, &m);
}

/// A reply ends the follow-up of what it answers: by its references, or failing them by the
/// same topic from someone the message went to.
fn end_follow_up(mb: &mut Mailbox, reply: &Message) {
    let topic = normalize_subject(&reply.subject).to_lowercase();
    let from = reply.from.email.to_lowercase();
    let answered: Vec<Id> = mb
        .messages
        .values()
        .filter(|o| o.awaiting_reply && o.id != reply.id)
        .filter(|o| {
            let referenced = !o.message_id.is_empty() && (reply.in_reply_to == o.message_id || reply.references.contains(&o.message_id));
            referenced
                || (!topic.is_empty()
                    && normalize_subject(&o.subject).to_lowercase() == topic
                    && o.recipients().any(|r| r.email.to_lowercase() == from))
        })
        .map(|o| o.id)
        .collect();
    for oid in answered {
        if let Ok(o) = mb.message_mut(oid) {
            o.awaiting_reply = false;
            o.flag = FlagStatus::Complete;
        }
    }
}

fn auto_reply_to(mb: &mut Mailbox, m: &Message) {
    let Some(account) = mb.account(m.account).cloned() else { return };
    if !account.kind.has_server() || !mb.vip.auto_reply.answers(m, &account.email) {
        return;
    }
    let Some(outbox) = mb.folder_of_kind(account.id, FolderKind::Outbox) else { return };
    let r = &mb.vip.auto_reply;
    let subject = if r.subject.trim().is_empty() { format!("Re: {}", normalize_subject(&m.subject)) } else { r.subject.clone() };
    let now = m.received;
    let domain = account.email.rsplit_once('@').map_or("", |(_, d)| d);
    let message_id = mailcraft_mime::new_message_id(domain, now, m.id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ now as u64);
    let mut references = m.references.clone();
    if !m.message_id.is_empty() {
        references.push(m.message_id.clone());
    }
    let reply = Message {
        account: account.id,
        folder: outbox,
        message_id,
        in_reply_to: m.message_id.clone(),
        references,
        from: account.address(),
        to: vec![m.reply_to.first().cloned().unwrap_or_else(|| m.from.clone())],
        subject,
        date: now,
        received: now,
        body: mailcraft_model::Body { text: r.message.clone(), html: None },
        read: true,
        headers: vec![("Auto-Submitted".into(), "auto-replied".into())],
        ..Default::default()
    };
    if mb.add_message(reply).is_ok() {
        let replied = &mut mb.vip.auto_reply.replied;
        if replied.len() >= MAX_REPLIED {
            replied.remove(0);
        }
        replied.push(m.from.email.trim().to_lowercase());
    }
}
