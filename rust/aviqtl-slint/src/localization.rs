//! UI language selection and shared effect metadata translations.

include!(concat!(env!("OUT_DIR"), "/effect_metadata_translations.rs"));

use aviqtl_app::settings::SettingsStore;
use std::borrow::Cow;
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UiLanguage {
    English,
    SimplifiedChinese,
    Japanese,
}

impl UiLanguage {
    pub(super) fn slint_locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh_CN",
            Self::Japanese => "ja_JP",
        }
    }
}

thread_local! {
    pub(super) static CURRENT_UI_LANGUAGE: Cell<UiLanguage> = const { Cell::new(UiLanguage::English) };
}

pub(super) fn localized(
    english: &'static str,
    simplified_chinese: &'static str,
    japanese: &'static str,
) -> &'static str {
    match current_ui_language() {
        UiLanguage::English => english,
        UiLanguage::SimplifiedChinese => simplified_chinese,
        UiLanguage::Japanese => japanese,
    }
}

pub(super) fn current_ui_language() -> UiLanguage {
    CURRENT_UI_LANGUAGE.with(|language| language.get())
}

pub(super) fn localized_effect_metadata(value: &str) -> Cow<'_, str> {
    let translated = |catalog: &'static [(&'static str, &'static str)]| {
        catalog
            .binary_search_by_key(&value, |(source, _)| *source)
            .ok()
            .map(|index| catalog[index].1)
    };
    match current_ui_language() {
        UiLanguage::English => translated(EFFECT_METADATA_ENGLISH)
            .map(Cow::Borrowed)
            .unwrap_or_else(|| Cow::Borrowed(value)),
        UiLanguage::SimplifiedChinese => translated(EFFECT_METADATA_SIMPLIFIED_CHINESE)
            .or_else(|| translated(EFFECT_METADATA_ENGLISH))
            .map(Cow::Borrowed)
            .unwrap_or_else(|| Cow::Borrowed(value)),
        UiLanguage::Japanese => Cow::Borrowed(value),
    }
}

pub(super) fn localized_effect_categories(categories: &[String]) -> String {
    categories
        .iter()
        .map(|category| localized_effect_metadata(category).into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Selects a persisted language or follows the operating-system locale.
/// English is the source language and therefore also the fallback for an
/// unsupported or unavailable locale.
pub(super) fn select_bundled_ui_translation(
    settings: &SettingsStore,
) -> Result<UiLanguage, String> {
    let language = configured_ui_language(settings);
    slint::select_bundled_translation(language.slint_locale())
        .map_err(|error| error.to_string())?;
    CURRENT_UI_LANGUAGE.with(|current| current.set(language));
    Ok(language)
}

fn configured_ui_language(settings: &SettingsStore) -> UiLanguage {
    match settings
        .value("uiLanguage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("System")
    {
        "SimplifiedChinese" => UiLanguage::SimplifiedChinese,
        "Japanese" => UiLanguage::Japanese,
        "English" => UiLanguage::English,
        _ => system_ui_language(),
    }
}

fn system_ui_language() -> UiLanguage {
    let locale = sys_locale::get_locale()
        .or_else(|| {
            std::env::var("LC_ALL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            std::env::var("LC_MESSAGES")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| std::env::var("LANG").ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    ui_language_from_locale(&locale)
}

pub(super) fn ui_language_from_locale(locale: &str) -> UiLanguage {
    let locale = locale.trim().to_ascii_lowercase();
    if locale.starts_with("zh") {
        UiLanguage::SimplifiedChinese
    } else if locale.starts_with("ja") {
        UiLanguage::Japanese
    } else {
        UiLanguage::English
    }
}
