// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The `html` field of a `mailto:` link: a message written in HTML elsewhere, its plain-text
//! stand-in in `body` for the programs that don't read it.

use crate::parse_mailto;

#[test]
fn the_html_of_a_link_is_read_beside_its_body() {
    let m = parse_mailto("mailto:a@example.org?subject=Offre&body=Bonjour%2C%20texte&html=%3Ctable%20width%3D%22600%22%3E%3Ctr%3E%3Ctd%3EBonjour%20%C3%A0%20vous%3C%2Ftd%3E%3C%2Ftr%3E%3C%2Ftable%3E").unwrap();
    assert_eq!(m.body, "Bonjour, texte");
    assert_eq!(m.html, "<table width=\"600\"><tr><td>Bonjour \u{e0} vous</td></tr></table>");
}

#[test]
fn only_the_first_html_counts_and_none_is_empty() {
    let m = parse_mailto("mailto:a@example.org?html=%3Cp%3Eone%3C%2Fp%3E&HTML=%3Cp%3Etwo%3C%2Fp%3E").unwrap();
    assert_eq!(m.html, "<p>one</p>");
    assert!(parse_mailto("mailto:a@example.org?body=hi").unwrap().html.is_empty());
}

#[test]
fn the_html_keeps_its_lines_and_no_other_control_character() {
    let m = parse_mailto("mailto:?html=%3Cp%3Ea%3C%2Fp%3E%0D%0A%3Cp%3Eb%00%1B%3C%2Fp%3E%09").unwrap();
    assert_eq!(m.html, "<p>a</p>\n<p>b</p>\t");
}

#[test]
fn an_html_past_the_link_cap_is_cut_and_nothing_breaks() {
    let link = format!("mailto:a@example.org?html={}", "%C3%A9".repeat(40_000));
    let m = parse_mailto(&link).unwrap();
    assert!(m.html.len() < 64 * 1024 && m.html.chars().all(|c| c == '\u{e9}' || c == '\u{fffd}'), "{}", m.html.len());
}
