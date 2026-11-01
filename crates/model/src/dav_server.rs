// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Where a calendar and contacts server is ([`DavServer`], held by [`crate::Account::dav`]): one
//! that speaks CalDAV (RFC 4791) and CardDAV (RFC 6352), as Nextcloud, ownCloud, Radicale,
//! Baïkal, Synology Calendar and many others do. The account has no mail: its calendars and its
//! address books are what is kept in step.
//!
//! Settings are checked as text, as a [`crate::JmapServer`]'s are: that takes no network library,
//! so the engine and the browser build can do it, and a host that is about to connect checks them
//! again, since a mailbox file is hostile too. The server's collections are not settings: the
//! host finds them from the address each time it asks (`.well-known/caldav`, the principal, the
//! calendar and address book homes), and they come down as the account's calendars
//! ([`crate::remote::CalendarsDownload`]) and contacts.

use serde::{Deserialize, Serialize};

/// The server of a [`crate::AccountKind::Dav`] account. Plain settings: the password (an app
/// password, with most servers) is given by whoever holds the connection, and is never kept here.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DavServer {
    /// The address the owner gave: the server's root (`https://cloud.example.org`), or anything
    /// on it that leads to the principal (`…/remote.php/dav`). It may name the organisation's
    /// server, so it is never written to a log; [`DavServer::check_url`] says whether it may be
    /// used.
    pub url: String,
    /// The name the account signs in with; empty means the account's e-mail address.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,
}

/// The address and the name stay out of debug output, which may reach a log.
impl std::fmt::Debug for DavServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavServer").field("url", &!self.url.is_empty()).field("username", &!self.username.is_empty()).finish()
    }
}

impl DavServer {
    /// Longest user name, in characters.
    pub const MAX_USERNAME: usize = 254;

    /// Whether `address` may be the server's: `Ok`, or why not, in words that follow the
    /// setting's name. The rules of a JMAP session address: `https://` only (plain `http://` to
    /// this computer only, for tests), no control character, space or backslash, no user name in
    /// front of the server, a plausible server name and port.
    pub fn check_url(address: &str) -> Result<(), &'static str> {
        crate::jmap_server::accepted(address).map(|_| ())
    }

    /// Whether `name` may be a user name: not too long, no control character and no colon (it
    /// goes in front of one in the sign-in).
    pub fn check_username(name: &str) -> Result<(), &'static str> {
        if name.chars().take(Self::MAX_USERNAME + 1).count() > Self::MAX_USERNAME {
            return Err("is longer than 254 characters");
        }
        if name.chars().any(char::is_control) {
            return Err("has a control character in it");
        }
        if name.contains(':') {
            return Err("has a colon in it");
        }
        Ok(())
    }

    /// Why these settings can't be used, when they can't.
    pub fn problem(&self) -> Option<&'static str> {
        Self::check_url(&self.url).err().or_else(|| Self::check_username(&self.username).err())
    }

    /// The name of the server the address is on (`cloud.example.org`), when the address may be
    /// used: for telling the owner which server it is, without the rest of the address.
    pub fn host(&self) -> Option<&str> {
        crate::jmap_server::accepted(&self.url).ok()
    }

    /// The name the account signs in with: its own, else `email`.
    pub fn login<'a>(&'a self, email: &'a str) -> &'a str {
        if self.username.is_empty() { email } else { &self.username }
    }
}

#[cfg(test)]
#[path = "dav_server_tests.rs"]
mod dav_server_tests;
