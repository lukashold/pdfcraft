#![cfg(not(target_arch = "wasm32"))]
use pdfcraft_automation::{Automation, Content};
use serde_json::{Value, json};

fn call(a: &mut Automation, name: &str, args: Value) -> Value {
    match a.call(name, &args).unwrap().remove(0) {
        Content::Json(value) => value,
        _ => panic!("expected JSON"),
    }
}

#[test]
fn hot_folder_analysis_only_preserves_pages_and_skips_processing_options() {
    let root = tempfile::tempdir().unwrap();
    for folder in ["in", "out"] {
        std::fs::create_dir(root.path().join(folder)).unwrap();
    }
    let session = pdfcraft_engine::Session::new();
    let text = session.create_from_text("source", "Analyze existing words without changing them").unwrap();
    let blank = session.create_blank(100.0, 100.0, 1).unwrap();
    let source = session.combine(&[("text.pdf".into(), text), ("blank.pdf".into(), blank)]).unwrap();
    std::fs::write(root.path().join("in/source.pdf"), source.as_slice()).unwrap();
    let mut a = Automation::new().with_root(root.path()).unwrap();
    let task = json!({"id":"analysis","name":"Analysis","input":"in","output":"out","analysis_only":true,"remove_blank_pages":true,"enabled":true,"interval_seconds":1,"settle_seconds":1,"preprocessing":{"invert":true,"split_spreads":true},"output_plan":{"steps":[{"format":"pdf"}]}});
    call(&mut a, "hot_folder_configure", json!({"config":"tasks.json","settings":{"tasks":[task]}}));
    call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let state = call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(state["tasks"]["analysis"]["completed"], 1, "{state}");
    assert_eq!(std::fs::read_dir(root.path().join("out")).unwrap().count(), 1);
    let report: Value = serde_json::from_slice(&std::fs::read(root.path().join("out/source.json")).unwrap()).unwrap();
    assert_eq!(report["format"], "pdfcraft-analysis");
    assert_eq!(report["ocr_performed"], false);
    assert_eq!(report["pages"].as_array().unwrap().len(), 2);
    assert_eq!(report["pages"][0]["content"], "existing_text");
    assert_eq!(report["pages"][1]["content"], "empty_content");
    assert_eq!(std::fs::read(root.path().join("in/source.pdf")).unwrap(), *source);
    let config = call(&mut a, "hot_folder_status", json!({"config":"tasks.json"}));
    assert_eq!(config["settings"]["tasks"][0]["settings"]["recognize"], true);
    assert_eq!(config["settings"]["tasks"][0]["output_plan"]["steps"][0]["format"], "pdf");
}

#[test]
fn hot_folder_project_sources_preserve_order_and_ack_only_after_all_outputs() {
    let root = std::env::temp_dir().join(format!("pdfcraft-hot-folder-project-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for folder in ["sources", "stage", "out"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    let session = pdfcraft_engine::Session::new();
    for (name, text) in [("z.pdf", "Zebra comes first"), ("a.pdf", "Apple comes second")] {
        std::fs::write(root.join("sources").join(name), session.create_from_text("source", text).unwrap().as_slice()).unwrap();
    }
    let mut a = Automation::new().with_root(&root).unwrap();
    let project = call(&mut a, "hot_folder_project", json!({"path":"sources/project.json","files":["sources/z.pdf","sources/a.pdf"]}));
    assert_eq!(project["project"]["files"], json!(["z.pdf", "a.pdf"]));
    assert!(a.call("hot_folder_project", &json!({"path":"../escaped.json","files":["sources/z.pdf"]})).is_err());
    assert!(a.call("hot_folder_project", &json!({"path":"sources/project.json","files":["sources/z.pdf"]})).is_err());
    let task = json!({"id":"project","name":"Project","input":"stage","output":"out","enabled":true,"interval_seconds":1,"settle_seconds":1,"settings":{"language":"de","recognize":false,"auto_rotate":false,"deskew":false},"source":{"kind":"project","manifest":"sources/project.json","action":{"kind":"delete"}},"output_plan":{"steps":[{"format":"pdf","grouping":"all","name_template":"combined"},{"format":"txt","grouping":"all","name_template":"combined"},{"format":"xlsx","grouping":"all","name_template":"combined"},{"format":"epub","grouping":"all","name_template":"combined"}]}});
    call(&mut a, "hot_folder_configure", json!({"config":"tasks.json","settings":{"tasks":[task]}}));
    let mut overlapping = call(&mut a, "hot_folder_status", json!({"config":"tasks.json"}))["settings"].clone();
    overlapping["tasks"].as_array_mut().unwrap().push(json!({"id":"other","name":"Other","input":"sources","output":"out","enabled":true}));
    assert!(a.call("hot_folder_configure", &json!({"config":"overlap.json","settings":overlapping})).unwrap_err().to_string().contains("overlap"));
    call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert!(root.join("sources/z.pdf").exists());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let state = call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(state["tasks"]["project"]["completed"], 2, "{state}");
    assert!(!root.join("sources/z.pdf").exists());
    assert!(!root.join("sources/a.pdf").exists());
    let text = std::fs::read_to_string(root.join("out/combined.txt")).unwrap();
    assert!(text.find("Zebra").unwrap() < text.find("Apple").unwrap(), "{text}");
    call(&mut a, "hot_folder_retry", json!({"config":"tasks.json","task":"project"}));
    call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(std::fs::read_dir(root.join("out")).unwrap().count(), 4);
    for extension in ["xlsx", "epub"] {
        assert!(std::fs::read(root.join(format!("out/combined.{extension}"))).unwrap().starts_with(b"PK\x03\x04"));
    }
    let error = a.call("hot_folder_source", &json!({"config":"tasks.json","task":"project","action":"credential_status"})).unwrap_err();
    assert!(error.to_string().contains("no authenticated"));
}

#[test]
fn hot_folder_tools_configure_poll_stop_and_confine_all_nested_paths() {
    let root = std::env::temp_dir().join(format!("pdfcraft-hot-folder-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("in")).unwrap();
    std::fs::create_dir_all(root.join("out")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let task =
        json!({"id":"incoming", "name":"Incoming scans", "input":"in", "output":"out", "enabled":true, "settle_seconds":1, "interval_seconds":1});
    call(&mut a, "hot_folder_configure", json!({"config":"tasks.json","settings":{"tasks":[task.clone()]}}));
    let state = call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(state["tasks"]["incoming"]["completed"], 0);
    let status = call(&mut a, "hot_folder_status", json!({"config":"tasks.json"}));
    assert_eq!(status["settings"]["tasks"][0]["enabled"], true);
    let mut stopped = task.clone();
    stopped["enabled"] = json!(false);
    call(&mut a, "hot_folder_configure", json!({"config":"tasks.json","settings":{"tasks":[stopped]}}));
    call(&mut a, "hot_folder_retry", json!({"config":"tasks.json","task":"incoming"}));
    assert!(call(&mut a, "hot_folder_status", json!({"config":"tasks.json"}))["state"]["tasks"].as_object().unwrap().is_empty());
    for field in ["input", "output"] {
        let mut escaped = task.clone();
        escaped[field] = json!("../outside");
        assert!(a.call("hot_folder_configure", &json!({"config":"bad.json","settings":{"tasks":[escaped]}})).is_err());
    }
    assert!(!root.join("bad.json").exists());
}

#[test]
fn hot_folder_tools_produce_searchable_output_and_preserve_source() {
    if !pdfcraft_engine::ocr::available() {
        return;
    }
    let root = std::env::temp_dir().join(format!("pdfcraft-hot-folder-ocr-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("in")).unwrap();
    std::fs::create_dir_all(root.join("out")).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let source = pdfcraft_engine::Session::new().create_from_text("source", "Hot folder searchable output").unwrap();
    std::fs::write(root.join("in/source.pdf"), source.as_slice()).unwrap();
    call(
        &mut a,
        "hot_folder_configure",
        json!({"config":"tasks.json","settings":{"tasks":[{"id":"test","name":"Test","input":"in","output":"out","enabled":true,"interval_seconds":1,"settle_seconds":1}]}}),
    );
    call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let result = call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(result["tasks"]["test"]["completed"], 1, "{result}");
    assert_eq!(std::fs::read(root.join("in/source.pdf")).unwrap(), *source);
    let doc = call(&mut a, "doc_open", json!({"path":"out/source.pdf"}))["doc"].clone();
    assert!(call(&mut a, "text_extract", json!({"doc":doc})).to_string().contains("searchable"));
}

#[test]
fn hot_folder_tools_apply_page_templates_blank_archives_and_multiple_outputs() {
    let root = std::env::temp_dir().join(format!("pdfcraft-hot-folder-pipeline-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for folder in ["in", "out", "blank"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    let session = pdfcraft_engine::Session::new();
    let text = session.create_from_text("source", "Pipeline output remains searchable").unwrap();
    let blank = session.create_blank(100.0, 100.0, 1).unwrap();
    let source = session.combine(&[("text.pdf".into(), text), ("blank.pdf".into(), blank)]).unwrap();
    std::fs::write(root.join("in/source.pdf"), source.as_slice()).unwrap();
    let mut a = Automation::new().with_root(&root).unwrap();
    let task = json!({"id":"pipeline","name":"Pipeline","input":"in","output":"out","blank_archive":"blank","remove_blank_pages":true,"enabled":true,"interval_seconds":1,"settle_seconds":1,"settings":{"recognize":false,"auto_rotate":false,"deskew":false},"preprocessing":{"pages":"1-2"},"output_plan":{"steps":[{"format":"pdf"},{"format":"txt"}]}});
    let mut escaped = task.clone();
    escaped["output_plan"]["steps"][1]["folder"] = json!("../outside");
    assert!(a.call("hot_folder_configure", &json!({"config":"bad.json","settings":{"tasks":[escaped]}})).is_err());
    call(&mut a, "hot_folder_configure", json!({"config":"tasks.json","settings":{"tasks":[task]}}));
    call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let result = call(&mut a, "hot_folder_poll", json!({"config":"tasks.json"}));
    assert_eq!(result["tasks"]["pipeline"]["completed"], 1, "{result}");
    assert!(root.join("out/source.pdf").exists());
    assert!(root.join("out/source.txt").exists());
    assert!(root.join("blank/source_blank_pages.pdf").exists());
    assert!(std::fs::read_to_string(root.join("out/source.txt")).unwrap().contains("Pipeline"));
    assert_eq!(std::fs::read(root.join("in/source.pdf")).unwrap(), *source);
}
