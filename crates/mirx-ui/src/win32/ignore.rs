//! mIRC's `/ignore` over Matrix's account-wide ignore list
//! (`m.ignored_user_list`). The server stops sending an ignored user's
//! events; the client only shows and edits the list, which it learns from
//! the server's echo.
//!
//! Pure code: the lines `/ignore` and a list change print, and the local
//! refusals. `nick_of` gives a user's nick where a known room has them as
//! a member with a display name.

use std::collections::BTreeSet;

/// `Bob (@bob:example)`, or the bare ID for a user with no known nick.
pub(super) fn user_label(user_id: &str, nick_of: &impl Fn(&str) -> Option<String>) -> String {
    match nick_of(user_id) {
        Some(nick) => format!("{nick} ({user_id})"),
        None => user_id.to_owned(),
    }
}

/// The name a refusal uses: the nick, else the ID.
pub(super) fn user_name(user_id: &str, nick_of: &impl Fn(&str) -> Option<String>) -> String {
    nick_of(user_id).unwrap_or_else(|| user_id.to_owned())
}

const NOT_RECEIVED: &str = "the ignore list has not been received yet";

/// `/ignore` without a nick: the list, or why it cannot be shown.
pub(super) fn list_lines(
    ignored: Option<&[String]>,
    nick_of: impl Fn(&str) -> Option<String>,
) -> Result<Vec<String>, String> {
    let ignored = ignored.ok_or(NOT_RECEIVED)?;
    if ignored.is_empty() {
        return Ok(vec!["* Ignore list is empty".to_owned()]);
    }
    Ok(std::iter::once("* Ignore list:".to_owned())
        .chain(
            ignored
                .iter()
                .map(|user_id| format!("*   {}", user_label(user_id, &nick_of))),
        )
        .collect())
}

/// What a new list adds and removes, by user ID: the lines that confirm
/// an `/ignore` or `/unignore` when the server's echo arrives.
pub(super) fn change_lines(
    old: &[String],
    new: &[String],
    nick_of: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let old = old.iter().collect::<BTreeSet<_>>();
    let new = new.iter().collect::<BTreeSet<_>>();
    let added = new
        .difference(&old)
        .map(|user_id| format!("* Added {} to ignore list", user_label(user_id, &nick_of)));
    let removed = old.difference(&new).map(|user_id| {
        format!(
            "* Removed {} from ignore list",
            user_label(user_id, &nick_of)
        )
    });
    added.chain(removed).collect()
}

/// Why `/ignore` would change nothing, if it would not.
pub(super) fn check_ignore(
    ignored: Option<&[String]>,
    own_user_id: Option<&str>,
    user_id: &str,
    nick_of: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let ignored = ignored.ok_or(NOT_RECEIVED)?;
    if own_user_id == Some(user_id) {
        return Err("you cannot ignore yourself".to_owned());
    }
    if ignored.iter().any(|ignored| ignored == user_id) {
        return Err(format!(
            "{} is already ignored",
            user_name(user_id, &nick_of)
        ));
    }
    Ok(())
}

/// Why `/unignore` would change nothing, if it would not.
pub(super) fn check_unignore(
    ignored: Option<&[String]>,
    user_id: &str,
    nick_of: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let ignored = ignored.ok_or(NOT_RECEIVED)?;
    if !ignored.iter().any(|ignored| ignored == user_id) {
        return Err(format!("{} is not ignored", user_name(user_id, &nick_of)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{change_lines, check_ignore, check_unignore, list_lines};

    fn nick_of(user_id: &str) -> Option<String> {
        (user_id == "@bob:example").then(|| "Bob".to_owned())
    }

    fn ids(user_ids: &[&str]) -> Vec<String> {
        user_ids
            .iter()
            .map(|user_id| (*user_id).to_owned())
            .collect()
    }

    #[test]
    fn list_lines_show_the_list_or_why_not() {
        assert_eq!(
            list_lines(None, nick_of),
            Err("the ignore list has not been received yet".to_owned())
        );
        assert_eq!(
            list_lines(Some(&[]), nick_of),
            Ok(vec!["* Ignore list is empty".to_owned()])
        );
        assert_eq!(
            list_lines(Some(&ids(&["@bob:example", "@spam:remote"])), nick_of),
            Ok(vec![
                "* Ignore list:".to_owned(),
                "*   Bob (@bob:example)".to_owned(),
                "*   @spam:remote".to_owned(),
            ])
        );
    }

    #[test]
    fn change_lines_name_who_was_added_and_removed() {
        let old = ids(&["@bob:example", "@old:remote"]);
        assert_eq!(change_lines(&old, &old, nick_of), Vec::<String>::new());
        assert_eq!(
            change_lines(&ids(&[]), &ids(&["@bob:example"]), nick_of),
            vec!["* Added Bob (@bob:example) to ignore list"]
        );
        assert_eq!(
            change_lines(&ids(&["@bob:example"]), &ids(&[]), nick_of),
            vec!["* Removed Bob (@bob:example) from ignore list"]
        );
        assert_eq!(
            change_lines(
                &old,
                &ids(&["@bob:example", "@a:remote", "@new:remote"]),
                nick_of
            ),
            vec![
                "* Added @a:remote to ignore list",
                "* Added @new:remote to ignore list",
                "* Removed @old:remote from ignore list",
            ]
        );
    }

    #[test]
    fn ignore_refuses_yourself_and_who_is_already_ignored() {
        let ignored = ids(&["@bob:example"]);
        assert_eq!(
            check_ignore(None, Some("@me:example"), "@x:remote", nick_of),
            Err("the ignore list has not been received yet".to_owned())
        );
        assert_eq!(
            check_ignore(Some(&ignored), Some("@me:example"), "@me:example", nick_of),
            Err("you cannot ignore yourself".to_owned())
        );
        assert_eq!(
            check_ignore(Some(&ignored), Some("@me:example"), "@bob:example", nick_of),
            Err("Bob is already ignored".to_owned())
        );
        assert_eq!(
            check_ignore(Some(&ignored), Some("@me:example"), "@x:remote", nick_of),
            Ok(())
        );
    }

    #[test]
    fn unignore_refuses_who_is_not_ignored() {
        let ignored = ids(&["@bob:example"]);
        assert_eq!(
            check_unignore(None, "@bob:example", nick_of),
            Err("the ignore list has not been received yet".to_owned())
        );
        assert_eq!(
            check_unignore(Some(&ignored), "@x:remote", nick_of),
            Err("@x:remote is not ignored".to_owned())
        );
        assert_eq!(
            check_unignore(Some(&ignored), "@bob:example", nick_of),
            Ok(())
        );
    }
}
