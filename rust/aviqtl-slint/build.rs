use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../i18n/AviQtl_en_US.ts");
    println!("cargo:rerun-if-changed=../../i18n/AviQtl_zh_CN.ts");
    generate_effect_metadata_translations().expect("generate effect metadata translations");

    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile Slint UI");
}

fn generate_effect_metadata_translations() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let source_root = manifest_dir.join("../..");
    let english = read_effect_metadata_translations(&source_root.join("i18n/AviQtl_en_US.ts"))?;
    let chinese = read_effect_metadata_translations(&source_root.join("i18n/AviQtl_zh_CN.ts"))?;
    let output = PathBuf::from(env::var("OUT_DIR")?).join("effect_metadata_translations.rs");
    let mut generated = String::new();
    write_translation_array(&mut generated, "EFFECT_METADATA_ENGLISH", &english);
    write_translation_array(
        &mut generated,
        "EFFECT_METADATA_SIMPLIFIED_CHINESE",
        &chinese,
    );
    fs::write(output, generated)?;
    Ok(())
}

fn read_effect_metadata_translations(
    path: &Path,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let xml = fs::read_to_string(path)?;
    let document = roxmltree::Document::parse_with_options(
        &xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )?;
    let mut translations = document
        .descendants()
        .filter(|node| node.has_tag_name("context"))
        .filter(|context| {
            context
                .children()
                .find(|node| node.has_tag_name("name"))
                .and_then(|node| node.text())
                == Some("AviQtl::Core::EffectRegistry")
        })
        .flat_map(|context| {
            context
                .children()
                .filter(|node| node.has_tag_name("message"))
        })
        .filter_map(|message| {
            let source = message
                .children()
                .find(|node| node.has_tag_name("source"))?
                .text()?;
            let translation = message
                .children()
                .find(|node| node.has_tag_name("translation"))?
                .text()?;
            (!source.is_empty() && !translation.is_empty())
                .then(|| (source.to_owned(), translation.to_owned()))
        })
        .collect::<Vec<_>>();
    translations.sort_by(|left, right| left.0.cmp(&right.0));
    translations.dedup_by(|left, right| left.0 == right.0);
    Ok(translations)
}

fn write_translation_array(output: &mut String, name: &str, translations: &[(String, String)]) {
    output.push_str(&format!("const {name}: &[(&str, &str)] = &[\n"));
    for (source, translation) in translations {
        output.push_str(&format!("    ({source:?}, {translation:?}),\n"));
    }
    output.push_str("];\n");
}
