// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;

#[test]
fn an_https_address_may_be_used() {
    for good in ["https://cloud.example.org", "HTTPS://Cloud.Example.org:8443/remote.php/dav/", "http://localhost:8080/dav"] {
        assert_eq!(DavServer::check_url(good), Ok(()), "{good}");
    }
}

#[test]
fn other_addresses_are_refused() {
    for bad in [
        "",
        "http://cloud.example.org/",
        "ftp://cloud.example.org/",
        "https://ada:pw@cloud.example.org/",
        "https://cloud.example.org/a b",
        "https://cloud.example.org\\x",
        "https:///dav",
        "https://cloud.example.org/\u{0}",
    ] {
        assert!(DavServer::check_url(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn user_names_are_checked_as_text_and_the_address_stands_in_for_none() {
    assert!(DavServer::check_username("ada").is_ok());
    assert!(DavServer::check_username("ada@example.org").is_ok());
    assert!(DavServer::check_username("ada:pw").is_err());
    assert!(DavServer::check_username("ada\r\nx").is_err());
    assert!(DavServer::check_username(&"x".repeat(300)).is_err());
    let server = DavServer { url: "https://cloud.example.org".into(), username: String::new() };
    assert_eq!(server.login("ada@example.org"), "ada@example.org");
    assert_eq!(DavServer { username: "ada".into(), ..server.clone() }.login("ada@example.org"), "ada");
    assert_eq!(server.host(), Some("cloud.example.org"));
    assert!(server.problem().is_none());
    assert_eq!(format!("{:?}", server), "DavServer { url: true, username: false }");
}
