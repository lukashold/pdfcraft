//! Complete catalogs and embedded-only script coverage for the new interface languages.

use super::*;
use std::collections::BTreeSet;

const NEW_CODES: &[&str] = &["id", "ko", "hi", "bn", "ur"];

#[test]
fn new_languages_cover_existing_catalog_keys_and_interface_literals() {
    let expected: BTreeSet<_> = LANGUAGES
        .iter()
        .flat_map(|info| catalog::parse_entries(info.source, info.plural_forms()).0)
        .map(|entry| (entry.context, entry.source))
        .collect();
    for code in NEW_CODES {
        let lang = Lang::from_code(code).expect("registered language");
        let (entries, errors) = catalog::parse_entries(lang.0.source, lang.0.plural_forms());
        assert!(errors.is_empty(), "{code}: {errors:?}");
        let actual: BTreeSet<_> = entries.into_iter().map(|entry| (entry.context, entry.source)).collect();
        let missing: Vec<_> = expected.difference(&actual).collect();
        assert!(missing.is_empty(), "{code}: absent catalog keys {missing:?}");
        let missing: Vec<_> = super::tests::ui_literals().into_iter().filter(|label| !has(lang, label)).collect();
        assert!(missing.is_empty(), "{code}: absent UI literals {missing:?}");
        for command in pdfcraft_engine::commands::COMMANDS {
            assert!(has(lang, command.label), "{code}: {}", command.label);
            if let Some(menu) = command.menu {
                assert!(has(lang, menu), "{code}: {menu}");
            }
        }
    }
}

#[test]
fn new_language_regions_and_explicit_preferences_survive_roundtrip() {
    for (code, regions) in [
        ("id", ["id-ID", "ID_id.UTF-8"]),
        ("ko", ["ko-KR", "KO_kr.UTF-8"]),
        ("hi", ["hi-IN", "HI_in.UTF-8"]),
        ("bn", ["bn-BD", "bn-IN"]),
        ("ur", ["ur-PK", "ur-IN"]),
    ] {
        let lang = Lang::from_code(code).expect("registered language");
        for region in regions {
            assert_eq!(primary_language([region, "en-US"].into_iter()), lang, "{region}");
        }
        assert_eq!(primary_language(["zz-ZZ", regions[0]].into_iter()), Lang::EN);
        let mut app = crate::PdfCraftApp::default();
        app.set_option("language", code).expect("language accepted");
        let mut restored = crate::PdfCraftApp::default();
        restored.restore(&app.persist());
        assert_eq!(restored.language, code);
        assert_eq!(Lang::from_pref(&restored.language), lang);
    }
}

#[test]
fn new_catalogs_have_embedded_glyphs_in_desktop_and_web_faces() {
    if pdfcraft_fonts::CRAFT_FONTS.is_empty() {
        eprintln!("skipping script font coverage: set CRAFT_FONTS_DIR to the documented font revision");
        return;
    }
    let mut web = crate::theme::font_definitions();
    for family in web.families.values_mut() {
        family.retain(|name| pdfcraft_fonts::CRAFT_FONTS.iter().find(|font| font.name() == *name).is_none_or(pdfcraft_fonts::CraftFont::on_web));
    }
    for (build, definitions) in [("desktop", crate::theme::font_definitions()), ("web/mobile", web)] {
        assert!(!definitions.font_data.contains_key(crate::theme::SYSTEM_FALLBACK));
        let mut fonts = egui::epaint::text::Fonts::new(egui::epaint::text::TextOptions::default(), definitions);
        for code in NEW_CODES {
            let lang = Lang::from_code(code).expect("registered language");
            let (entries, _) = catalog::parse_entries(lang.0.source, lang.0.plural_forms());
            let calendar = pdfcraft_engine::dates::DATE_LANGUAGES.iter().find(|item| item.code == *code).unwrap();
            let calendar_text: String =
                calendar.months.iter().chain(&calendar.months_short).chain(&calendar.days).chain(&calendar.days_short).copied().collect();
            let characters: BTreeSet<_> = entries
                .iter()
                .flat_map(|entry| entry.translation.chars())
                .chain(lang.name().chars())
                .chain(calendar_text.chars())
                .filter(|c| !c.is_control())
                .collect();
            for id in [egui::FontId::proportional(13.0), egui::FontId::monospace(13.0), crate::theme::medium(13.0), crate::theme::semibold(17.0)] {
                let missing: Vec<_> = characters.iter().filter(|c| !fonts.has_glyphs(&id, &c.to_string())).collect();
                assert!(missing.is_empty(), "{code}, {build}, {id:?}: missing glyphs {missing:?}");
            }
        }
    }
}

#[test]
fn new_plural_rules_and_urdu_display_preserve_values() {
    for (code, expected) in [("hi", [0, 0, 1, 1]), ("bn", [0, 0, 1, 1]), ("ur", [1, 0, 1, 1])] {
        let lang = Lang::from_code(code).unwrap();
        assert_eq!([0, 1, 2, 101].map(lang.0.plural), expected, "{code}");
        assert_eq!(lang.0.plural_forms(), 2);
        for n in [0, 1, 2, 101] {
            let translated = trn(lang, n, "{n} page", "{n} pages");
            assert!(translated.contains(&n.to_string()), "{code}: {translated}");
            assert!(!translated.contains('{'), "{code}: unfilled plural placeholder");
        }
    }
    for code in ["id", "ko"] {
        let lang = Lang::from_code(code).unwrap();
        assert_eq!(lang.0.plural_forms(), 1);
    }
    let ur = Lang::from_code("ur").unwrap();
    assert!(ur.rtl());
    assert_eq!(tr(ur, "Save as…"), "…کریں محفوظ سے نام اس");
    let filled = fmt(tr(ur, "Couldn't open {name}: {e}"), &[("name", "a {e}.pdf"), ("e", "network-error")]);
    assert!(filled.contains("a {e}.pdf") && filled.contains("network-error"), "{filled}");
    assert!(!filled.contains("{name}"), "{filled}");
    assert_eq!(tr(ur, "user-file.pdf"), "user-file.pdf");
}
