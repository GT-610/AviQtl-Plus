//! The one-line report a command leaves behind.
//!
//! A status line is user-facing text, but it is produced by the layer that owns
//! the command, not by the window that shows it: the same sentence is read by the
//! status bar, by the tests, and by the Lua host. Keeping only the English form
//! meant a Chinese or Japanese interface showed `Edit applied` in a status bar
//! that was otherwise fully translated.
//!
//! A message therefore carries the same sentence in every language the interface
//! speaks, built from one set of arguments, and the window picks the rendering it
//! needs. Putting the translations next to the command is deliberate: a table
//! keyed by an English string nobody can grep for rots silently, while a
//! `status_message!` call cannot drift away from the sentence it translates.
//!
//! Text that arrives from somewhere else - an error string from `aviqtl-core`, a
//! path, a name a script chose - has no translation and is reported as it is;
//! see [`WorkspaceModel::set_status_text`](crate::workspace::WorkspaceModel).

/// One status line, rendered in each language the interface offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusMessage {
    english: String,
    simplified_chinese: String,
    japanese: String,
}

impl StatusMessage {
    pub fn new(
        english: impl Into<String>,
        simplified_chinese: impl Into<String>,
        japanese: impl Into<String>,
    ) -> Self {
        Self {
            english: english.into(),
            simplified_chinese: simplified_chinese.into(),
            japanese: japanese.into(),
        }
    }

    /// The source language. Tests, the command line, and the Lua host read this.
    pub fn english(&self) -> &str {
        &self.english
    }

    pub fn simplified_chinese(&self) -> &str {
        &self.simplified_chinese
    }

    pub fn japanese(&self) -> &str {
        &self.japanese
    }
}

/// Builds one status line from the same arguments in every language.
///
/// The arguments are formatted three times, so a placeholder that does not match
/// its arguments fails to compile instead of showing a raw `{}` to the user.
///
/// ```
/// use aviqtl_app::status_message;
/// let message = status_message!("Layer {} is locked", "图层 {} 已锁定", "レイヤー {} はロックされています", 3);
/// assert_eq!(message.english(), "Layer 3 is locked");
/// assert_eq!(message.simplified_chinese(), "图层 3 已锁定");
/// ```
#[macro_export]
macro_rules! status_message {
    ($english:literal, $simplified_chinese:literal, $japanese:literal $(, $argument:expr)*) => {
        $crate::status::StatusMessage::new(
            format!($english $(, $argument)*),
            format!($simplified_chinese $(, $argument)*),
            format!($japanese $(, $argument)*),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::StatusMessage;

    /// Every `status_message!` call site in the crate, as (english, chinese, japanese).
    ///
    /// Reads the source rather than a registry, so a new status cannot be added
    /// without appearing here.
    fn composed_statuses(file_name: &str, source: &str) -> Vec<(String, String, String)> {
        let mut found = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find("status_message!(") {
            rest = &rest[start + "status_message!(".len()..];
            let mut literals = Vec::new();
            let mut cursor = rest;
            while literals.len() < 3 {
                let Some((before, after)) = cursor.split_once('"') else {
                    break;
                };
                // A literal argument starts its own line; anything else means the
                // call passes a variable - the layer-overflow message, for one.
                let starts_clean = before.trim().is_empty() || before.trim().ends_with(',');
                if !starts_clean {
                    break;
                }
                let Some((literal, remainder)) = after.split_once('"') else {
                    break;
                };
                if literal.contains('\\') {
                    break;
                }
                literals.push(literal.to_owned());
                cursor = remainder;
            }
            if literals.len() == 3 {
                let (english, chinese, japanese) = (
                    literals[0].clone(),
                    literals[1].clone(),
                    literals[2].clone(),
                );
                found.push((english, chinese, japanese));
            } else if !literals.is_empty() {
                panic!(
                    "{file_name}: a status_message! call needs three literals, found {literals:?}"
                );
            }
        }
        found
    }

    #[test]
    fn every_composed_status_is_actually_translated() {
        // The minimums only guard the scanner: if a call site stops being
        // recognisable, the count drops and this fails instead of checking nothing.
        let sources = [
            ("workspace.rs", include_str!("workspace.rs"), 40),
            ("package_manager.rs", include_str!("package_manager.rs"), 10),
        ];
        let mut checked = 0;
        for (file_name, source, minimum) in sources {
            let statuses = composed_statuses(file_name, source);
            assert!(
                statuses.len() >= minimum,
                "{file_name}: expected at least {minimum} status calls, found {}",
                statuses.len()
            );
            for (english, chinese, japanese) in statuses {
                assert!(
                    !chinese.trim().is_empty() && chinese != english,
                    "{file_name}: \"{english}\" has no Chinese wording"
                );
                assert!(
                    !japanese.trim().is_empty() && japanese != english,
                    "{file_name}: \"{english}\" has no Japanese wording"
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 50,
            "only {checked} composed statuses were checked"
        );
    }

    #[test]
    fn one_call_renders_the_same_facts_in_every_language() {
        let message = status_message!(
            "Layer {} is locked",
            "图层 {} 已锁定",
            "レイヤー {} はロックされています",
            2
        );
        assert_eq!(message.english(), "Layer 2 is locked");
        assert_eq!(message.simplified_chinese(), "图层 2 已锁定");
        assert_eq!(message.japanese(), "レイヤー 2 はロックされています");
    }

    #[test]
    fn a_message_without_arguments_keeps_its_wording() {
        let message = status_message!("Ready", "就绪", "準備完了");
        assert_eq!(message.english(), "Ready");
        assert_eq!(message.japanese(), "準備完了");
        assert_eq!(StatusMessage::default().english(), "");
    }
}
