//! Scan & OCR ▸ Recognize text in the real shell (egui_kittest): the dialog, the run and its
//! result (skipped without the OCR models: `cargo xtask models`).

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::{Session, export};
use pdfcraft_ui_egui::PdfCraftApp;

/// A one-page PDF that is only a picture of a sentence.
fn scan() -> Vec<u8> {
    let mut s = Session::new();
    let text = s.create_from_text("t", "Recognize these scanned words").unwrap();
    let id = s.open("text.pdf", None, text, None).unwrap();
    let png = export::Exporter::new(s.get(id).unwrap()).png(0, 150.0).unwrap();
    s.create_from_images(&[("scan.png".into(), png)]).unwrap().to_vec()
}

#[test]
fn recognize_text_dialog_adds_searchable_text() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("ocr.recognize"));
    // The scrollable dialog settles its size before pointer coordinates are read.
    h.run_steps(10);
    h.get_by_label("Document language");
    h.get_by_label("Output");
    h.get_by_label("Automatically rotate sideways and upside-down pages");
    h.get_by_label("Straighten skewed scans");
    assert!(h.state().ocr_draft.auto_rotate && h.state().ocr_draft.deskew);
    let expected = pdfcraft_engine::ocr::language_for_locale(sys_locale::get_locale().as_deref());
    assert_eq!(h.state().ocr_draft.language, expected, "OCR follows the system even when the interface is English");
    let label = pdfcraft_engine::ocr::LANGUAGES.iter().find(|(code, _)| *code == expected).unwrap().1;
    h.get_by_value(label).click();
    h.run_steps(2);
    for label in ["Nederlands", "Dansk", "Norsk bokmål", "Svenska", "Suomi", "Português"] {
        h.get_by_label(label);
    }
    h.get_by_label("Português").click();
    h.run_steps(2);
    assert_eq!(h.state().ocr_draft.language, "pt");
    h.get_by_value("Português").click();
    h.run_steps(2);
    h.get_by_label("Italiano").click();
    h.run_steps(2);
    assert_eq!(h.state().ocr_draft.language, "it");
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    h.get_by_label("Recognize text").click();
    h.run_steps(3);
    assert!(h.state().ocr_run.is_none(), "finished");
    let toast = h.state().toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && toast.contains("on 1 page"), "{toast}");
    let app = h.state();
    let id = app.active_ids().unwrap().1;
    assert_eq!(app.session.get(id).unwrap().can_undo(), Some("Recognize text"));
}

#[test]
fn historical_ocr_choice_requires_an_explicit_addon_download() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    h.state_mut().execute("ocr.recognize");
    h.run_steps(10);
    let language = h.state().ocr_draft.language.clone();
    let status = h.state_mut().ocr_addon.status.as_mut().unwrap();
    status.installed = false;
    status.supported = true;
    h.get_by_value("Modern print").click();
    h.run_steps(10);
    h.get_by_label("Historical print (Fraktur)").click();
    h.run_steps(10);
    assert_eq!(h.state().ocr_draft.recognizer, pdfcraft_engine::ocr::Recognizer::Historical);
    assert_eq!(h.state().ocr_draft.language, language, "style selection keeps the document language");
    h.get_by_label("Automatic (historical model)");
    h.get_by_label("Download historical OCR add-on (13 MB)");
    assert!(h.get_by_label("Recognize text").accesskit_node().is_disabled());
    assert!(h.state().ocr_run.is_none(), "choosing a style starts neither recognition nor download");
    h.get_by_value("Historical print (Fraktur)").click();
    h.run_steps(10);
    h.get_by_label("Historical handwriting (Kurrent)").click();
    h.run_steps(10);
    assert_eq!(h.state().ocr_draft.recognizer, pdfcraft_engine::ocr::Recognizer::HistoricalHandwriting);
    h.get_by_label("Handwriting requires upright horizontal lines in one text column. Columns, tables and marginal notes are not supported.");
    assert!(h.get_by_label("Recognize text").accesskit_node().is_disabled());
    h.get_by_value("Historical handwriting (Kurrent)").click();
    h.run_steps(10);
    h.get_by_label("Modern print").click();
    h.run_steps(10);
    assert_eq!(h.state().ocr_draft.recognizer, pdfcraft_engine::ocr::Recognizer::Modern);
}

#[test]
fn installed_historical_ocr_runs_from_the_dialog() {
    if !pdfcraft_engine::ocr::available() || !pdfcraft_engine::ocr_addons::status().installed {
        eprintln!("skipped: standard OCR and optional historical models are required");
        return;
    }
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app.run_inline = true;
        app.ocr_draft.recognizer = pdfcraft_engine::ocr::Recognizer::Historical;
        app.ocr_draft.auto_rotate = false;
        app.ocr_draft.deskew = false;
        app
    });
    h.run_steps(4);
    h.state_mut().execute("ocr.recognize");
    h.run_steps(10);
    h.get_by_label("Historical OCR add-on installed");
    h.get_by_label("Remove add-on");
    assert!(!h.get_by_label("Recognize text").accesskit_node().is_disabled());
    h.get_by_label("Recognize text").click();
    h.run_steps(3);
    assert!(h.state().ocr_run.is_none());
    let toast = h.state().toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && !toast.starts_with("Recognized 0 "), "{toast}");
    let app = h.state();
    let id = app.active_ids().unwrap().1;
    assert_eq!(app.session.get(id).unwrap().can_undo(), Some("Recognize text"));
}

/// Opt-in network smoke test. The caller must provide a fresh, isolated add-on
/// root; the ordinary test suite never downloads or removes installed resources.
#[test]
#[ignore = "explicit network test; set PDFCRAFT_OCR_ADDONS to a fresh isolated directory"]
fn historical_ocr_download_and_remove_buttons_manage_the_real_addon() {
    let root = std::env::var_os("PDFCRAFT_OCR_ADDONS").expect("an explicit isolated add-on root is required");
    let directory = std::path::Path::new(&root).join("historical-v1");
    assert!(directory.is_absolute() && !directory.exists(), "the test must not touch an existing add-on");
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app.run_inline = true;
        app.ocr_draft.recognizer = pdfcraft_engine::ocr::Recognizer::Historical;
        app
    });
    h.run_steps(4);
    h.state_mut().execute("ocr.recognize");
    h.run_steps(10);
    h.get_by_label("Download historical OCR add-on (13 MB)").click();
    h.run_steps(10);
    assert!(h.state().ocr_addon.status.as_ref().unwrap().installed, "{:?}", h.state().ocr_addon.message);
    h.get_by_label("Historical OCR add-on installed");
    h.get_by_label("Remove add-on").click();
    h.run_steps(10);
    assert!(!h.state().ocr_addon.status.as_ref().unwrap().installed);
    assert!(!directory.exists());
}

#[test]
fn enhance_scan_dialog_offers_correction_without_recognition() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("ocr.enhance"));
    h.run_steps(10);
    h.get_by_label("Correct scans");
    assert!(!h.state().ocr_draft.recognize);
    assert!(h.state().ocr_draft.auto_crop);
    assert_eq!(h.state().ocr_draft.language, pdfcraft_engine::ocr::language_for_locale(sys_locale::get_locale().as_deref()));
    h.get_by_label("Safety margin (pt)");
    h.get_by_label("Crop scanner borders to the paper edge").click();
    h.run_steps(2);
    assert!(!h.state().ocr_draft.auto_crop);
    h.get_by_label("Add searchable text").click();
    h.run_steps(2);
    assert!(h.state().ocr_draft.recognize);
}

#[test]
fn recognize_text_in_multiple_files_writes_searchable_copies() {
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdfcraft-ocr-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    app.run_inline = true;
    app.export_dir_override = Some(dir.to_string_lossy().into_owned());
    app.use_files(pdfcraft_ui_egui::FilePurpose::Ocr, vec![("one.pdf".into(), scan()), ("two.pdf".into(), scan())]);
    assert!(app.ocr_batch.is_none(), "finished");
    let toast = app.toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && toast.ends_with("in 2 files"), "{toast}");
    for name in ["one.pdf", "two.pdf"] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert!(bytes.len() > scan().len(), "an incremental update was appended");
    }
}
