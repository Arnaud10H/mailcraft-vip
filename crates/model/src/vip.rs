//! What VIP Mail brought to MailCraft: senders who matter (VIP), message templates, automatic
//! replies for any server, and reading a newsletter's unsubscribe links. The data lives here; the
//! commands are `vip.*`, `template.*`, `insert.quickParts`, `account.automaticReplies` and
//! `mail.unsubscribe` in the engine.

use mailcraft_time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::{Id, Message};

/// Most VIP senders, templates and remembered auto-replied senders kept.
pub const MAX_VIPS: usize = 1_000;
pub const MAX_TEMPLATES: usize = 200;
pub const MAX_REPLIED: usize = 5_000;
/// Longest template or automatic reply text kept.
pub const MAX_TEXT: usize = 50_000;

/// The category VIP mail is shown with.
pub const VIP_CATEGORY: &str = "VIP";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Vip {
    /// VIP senders' addresses, lower case.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub senders: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    #[serde(skip_serializing_if = "AutoReply::is_off")]
    pub auto_reply: AutoReply,
}

impl Vip {
    pub fn is_empty(&self) -> bool {
        self.senders.is_empty() && self.templates.is_empty() && self.auto_reply.is_off()
    }

    /// Whether mail from `email` is VIP.
    pub fn is_vip(&self, email: &str) -> bool {
        let e = email.trim().to_lowercase();
        !e.is_empty() && self.senders.contains(&e)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Template {
    pub id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    pub body: String,
}

/// The out-of-office reply, sent once to each sender while it is on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoReply {
    pub enabled: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    pub message: String,
    /// Only between these times, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<Timestamp>,
    /// When it was turned on: mail that came before is never answered.
    pub since: Timestamp,
    /// Who was answered since it was turned on, lower case.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replied: Vec<String>,
}

impl AutoReply {
    pub fn is_off(&self) -> bool {
        !self.enabled && self.message.is_empty()
    }

    /// Whether `m`, which just arrived for `me`, gets the reply (RFC 3834's rules: never to
    /// automatic mail, lists, bulk mail or anyone answered already).
    pub fn answers(&self, m: &Message, me: &str) -> bool {
        let at = m.received;
        let from = m.from.email.trim().to_lowercase();
        let local = from.split('@').next().unwrap_or("");
        let header = |name: &str| m.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_lowercase());
        self.enabled
            && !self.message.trim().is_empty()
            && at >= self.since
            && self.start.is_none_or(|s| at >= s)
            && self.end.is_none_or(|e| at < e)
            // Old mail that a first download brings is not news.
            && m.date.saturating_add(3 * 86_400) >= at
            && m.from.is_valid()
            && from != me.trim().to_lowercase()
            && !self.replied.contains(&from)
            && !["noreply", "no-reply", "donotreply", "do-not-reply", "mailer-daemon", "postmaster", "bounce", "notifications"]
                .iter()
                .any(|n| local.contains(n))
            && header("auto-submitted").is_none_or(|v| v == "no")
            && header("precedence").is_none_or(|v| !matches!(v.as_str(), "bulk" | "list" | "junk"))
            && header("list-id").is_none()
            && header("list-unsubscribe").is_none()
    }
}

/// How to leave a mailing list, from `List-Unsubscribe` (RFC 2369) and `List-Unsubscribe-Post`
/// (RFC 8058).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unsubscribe {
    /// An `https:` address to open (or to POST to, when `one_click`).
    pub web: Option<String>,
    /// A `mailto:` address, without the scheme.
    pub mailto: Option<String>,
    pub one_click: bool,
}

impl Unsubscribe {
    pub fn of(m: &Message) -> Option<Unsubscribe> {
        let value = m.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("list-unsubscribe")).map(|(_, v)| v.as_str())?;
        let mut u = Unsubscribe::default();
        for part in value.split(',').take(10) {
            let link = part.trim().trim_start_matches('<').trim_end_matches('>').trim();
            if link.chars().any(|c| c.is_whitespace() || c.is_control()) || link.len() > 2_000 {
                continue;
            }
            let lower = link.to_ascii_lowercase();
            if u.web.is_none() && lower.starts_with("https://") {
                u.web = Some(link.to_string());
            } else if u.mailto.is_none() && lower.starts_with("mailto:") {
                u.mailto = link.get(7..).map(str::to_string).filter(|a| !a.is_empty());
            }
        }
        u.one_click = u.web.is_some()
            && m.headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("list-unsubscribe-post") && v.trim().eq_ignore_ascii_case("List-Unsubscribe=One-Click"));
        (u.web.is_some() || u.mailto.is_some()).then_some(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Address;

    fn mail(from: &str, headers: &[(&str, &str)]) -> Message {
        Message {
            from: Address { name: String::new(), email: from.into() },
            date: 1_000_000,
            received: 1_000_100,
            headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn auto_reply_skips_automatic_and_list_mail() {
        let r = AutoReply { enabled: true, message: "Away".into(), since: 1_000_000, ..Default::default() };
        assert!(r.answers(&mail("ada@example.org", &[]), "me@example.org"));
        assert!(!r.answers(&mail("me@example.org", &[]), "me@example.org"));
        assert!(!r.answers(&mail("no-reply@example.org", &[]), "me@example.org"));
        assert!(!r.answers(&mail("ada@example.org", &[("Auto-Submitted", "auto-replied")]), "me@example.org"));
        assert!(!r.answers(&mail("ada@example.org", &[("Precedence", "bulk")]), "me@example.org"));
        assert!(!r.answers(&mail("ada@example.org", &[("List-Id", "<news.example.org>")]), "me@example.org"));
        let done = AutoReply { replied: vec!["ada@example.org".into()], ..r.clone() };
        assert!(!done.answers(&mail("ada@example.org", &[]), "me@example.org"));
        let later = AutoReply { since: 2_000_000, ..r.clone() };
        assert!(!later.answers(&mail("ada@example.org", &[]), "me@example.org"));
        let ended = AutoReply { end: Some(1_000_050), ..r };
        assert!(!ended.answers(&mail("ada@example.org", &[]), "me@example.org"));
    }

    #[test]
    fn unsubscribe_links() {
        let m = mail(
            "news@example.org",
            &[
                ("List-Unsubscribe", "<mailto:leave@example.org?subject=x>, <https://example.org/u?id=1>"),
                ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
            ],
        );
        let u = Unsubscribe::of(&m).unwrap();
        assert_eq!(u.web.as_deref(), Some("https://example.org/u?id=1"));
        assert_eq!(u.mailto.as_deref(), Some("leave@example.org?subject=x"));
        assert!(u.one_click);
        assert!(Unsubscribe::of(&mail("a@example.org", &[("List-Unsubscribe", "<http://example.org/u>")])).is_none());
        assert!(Unsubscribe::of(&mail("a@example.org", &[])).is_none());
    }

    #[test]
    fn vip_is_case_blind() {
        let v = Vip { senders: vec!["ada@example.org".into()], ..Default::default() };
        assert!(v.is_vip(" Ada@Example.org "));
        assert!(!v.is_vip(""));
    }
}

/// A CalDAV server whose calendar an account reads ([`crate::Account::dav`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Dav {
    /// `https://cloud.example.com`.
    pub url: String,
    pub username: String,
    /// The calendar, as a path on that server (Nextcloud's when empty: [`Dav::calendar_path`]).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub calendar: String,
}

impl Dav {
    pub fn calendar_path(&self) -> String {
        if self.calendar.is_empty() { format!("/remote.php/dav/calendars/{}/personal/", self.username) } else { self.calendar.clone() }
    }
}

/// One resource of a CalDAV collection: where it is, its version, its iCalendar text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DavItem {
    pub href: String,
    pub etag: String,
    pub data: String,
}

/// What a Send/Receive brought down from an account's [`Dav`] server. `None`: not asked, or it failed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DavDownload {
    /// The stretch of the calendar that was asked for (Unix seconds).
    pub from: Timestamp,
    pub to: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<DavItem>>,
}
