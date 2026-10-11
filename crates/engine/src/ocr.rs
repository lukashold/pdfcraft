//! Scan & OCR ▸ Recognize text: render pages, read the words in them and add an invisible text
//! layer ([`Edit::AddOcrText`]), making scanned pages searchable and selectable.
//!
//! Recognition is slow (about a second a page), so it is split from the edit: [`Session::ocr_job`]
//! captures what it needs, [`OcrJob::run`] works anywhere (the UI runs it on a worker thread) and
//! [`Session::apply_ocr`] applies the result as one undoable step.

use std::sync::{Arc, Mutex};

use pdfcraft_render::{PageInfo, PageRenderer, RenderConfig, RenderRequest, RequestKind};

pub use pdfcraft_ocr::scan::Correction;
pub use pdfcraft_ocr::{LANGUAGES, Models, Ocr, OcrError, PlacedWord, language_for_locale};

use crate::{DocId, Edit, EditError, Session};

/// Recognize Text settings.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrSettings {
    /// Resolution pages are rendered at for recognition (Acrobat's "Downsample to" choices).
    pub dpi: f32,
    /// Language code from [`LANGUAGES`]; recognized using the multilingual model.
    pub language: String,
    /// Leave pages that already have text alone (Acrobat reports "page contains renderable
    /// text" and skips them).
    pub skip_text_pages: bool,
    pub auto_rotate: bool,
    pub deskew: bool,
    /// Detect the paper edge before alignment/OCR. Opt-in for ordinary recognition.
    pub auto_crop: bool,
    /// Extra paper-edge padding, in displayed PDF points (0..20).
    pub crop_margin: f64,
    /// Disable to straighten scans without adding a text layer.
    pub recognize: bool,
}

impl Default for OcrSettings {
    fn default() -> Self {
        OcrSettings {
            dpi: 300.0,
            language: "en".into(),
            skip_text_pages: true,
            auto_rotate: true,
            deskew: true,
            auto_crop: false,
            crop_margin: 2.0,
            recognize: true,
        }
    }
}

/// What recognition found on one page.
#[derive(Clone, PartialEq)]
pub struct OcrPage {
    pub page: usize,
    pub words: Vec<PlacedWord>,
    /// Why the page was not read, if it was skipped.
    pub skipped: Option<String>,
    pub correction: Correction,
    /// Detected crop in original PDF user space, before rotation and deskew; None if uncertain.
    pub crop: Option<[f64; 4]>,
    /// Snapshot guard: never apply positions to pages changed while recognition was running.
    source: Arc<Vec<u8>>,
}

impl std::fmt::Debug for OcrPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrPage")
            .field("page", &self.page)
            .field("words", &self.words)
            .field("skipped", &self.skipped)
            .field("correction", &self.correction)
            .field("crop", &self.crop)
            .finish_non_exhaustive()
    }
}

impl OcrPage {
    pub fn changed(&self) -> bool {
        self.correction.changed() || self.crop.is_some()
    }
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// The recogniser, loaded once (it takes a moment) and shared.
pub fn engine() -> Result<Arc<Ocr>, String> {
    static OCR: Mutex<Option<Arc<Ocr>>> = Mutex::new(None);
    let mut slot = OCR.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = slot.as_ref() {
        return Ok(o.clone());
    }
    let o = Arc::new(Ocr::find().map_err(|e| e.to_string())?);
    *slot = Some(o.clone());
    Ok(o)
}

/// Whether the recognition models are installed.
pub fn available() -> bool {
    Models::find().is_some()
}

/// Everything recognition needs from a document, detached from the session.
pub struct OcrJob {
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    infos: Vec<PageInfo>,
    /// Per page: the render scale (pixels per point) that shows its images at their own
    /// resolution, if it has any.
    native: Vec<Option<f32>>,
    pub pages: Vec<usize>,
    pub settings: OcrSettings,
}

impl OcrJob {
    /// Read the pages. `progress(done, total)` is called before each page; returning `false`
    /// stops (the pages read so far are returned).
    pub fn run(self, ocr: &Ocr, progress: impl FnMut(usize, usize) -> bool) -> Vec<OcrPage> {
        let failure = |why: &str| {
            self.pages
                .iter()
                .map(|&page| OcrPage {
                    page,
                    words: Vec::new(),
                    skipped: Some(why.into()),
                    correction: Correction::default(),
                    crop: None,
                    source: self.bytes.clone(),
                })
                .collect()
        };
        if !self.settings.dpi.is_finite()
            || !(72.0..=600.0).contains(&self.settings.dpi)
            || !LANGUAGES.iter().any(|l| l.0 == self.settings.language)
            || !self.settings.crop_margin.is_finite()
            || !(0.0..=20.0).contains(&self.settings.crop_margin)
        {
            return failure("invalid OCR language, resolution or crop margin (0..20 pt)");
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run_inner(ocr, progress)))
            .unwrap_or_else(|_| failure("scan processing failed safely; the document was not changed"))
    }

    fn run_inner(&self, ocr: &Ocr, mut progress: impl FnMut(usize, usize) -> bool) -> Vec<OcrPage> {
        let config = RenderConfig { password: self.password.as_deref().map(Arc::from), ..Default::default() };
        let mut r = PageRenderer::new(self.bytes.clone(), config);
        let mut out = Vec::new();
        let total = self.pages.len();
        for (done, &page) in self.pages.iter().enumerate() {
            if !progress(done, total) {
                break;
            }
            let Some(info) = self.infos.get(page) else { continue };
            let skip = |why: &str| OcrPage {
                page,
                words: Vec::new(),
                skipped: Some(why.into()),
                correction: Correction::default(),
                crop: None,
                source: self.bytes.clone(),
            };
            if self.settings.skip_text_pages {
                let t = r.render(RenderRequest { page, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
                if t.text.as_ref().is_some_and(|t| !t.plain_text().trim().is_empty()) {
                    out.push(skip("the page already contains text"));
                    continue;
                }
            }
            // A scan is read at its own resolution (enlarging it only blurs the letters), at most
            // the chosen one.
            let wanted = self.settings.dpi.clamp(72.0, 600.0) / 72.0;
            let scale = self.native.get(page).copied().flatten().map_or(wanted, |n| n.clamp(1.0, wanted));
            let shot = r.render(RenderRequest { page, scale, ..Default::default() });
            if let Some(e) = shot.error {
                out.push(skip(&e));
                continue;
            }
            let Some(rgba) = image::RgbaImage::from_raw(shot.width, shot.height, shot.rgba.to_vec()) else {
                out.push(skip("invalid rendered image"));
                continue;
            };
            let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
            let to_pdf = |x: f64, y: f64| {
                info.view_to_user(x as f32 * info.width / shot.width as f32, y as f32 * info.height / shot.height as f32).map(f64::from)
            };
            let bounds = self
                .settings
                .auto_crop
                .then(|| pdfcraft_ocr::crop::paper_bounds(&rgb))
                .flatten()
                .map(|[l, t, r, b]| {
                    let mx = (self.settings.crop_margin * shot.width as f64 / info.width as f64).ceil() as u32;
                    let my = (self.settings.crop_margin * shot.height as f64 / info.height as f64).ceil() as u32;
                    [l.saturating_sub(mx), t.saturating_sub(my), r.saturating_add(mx).min(shot.width), b.saturating_add(my).min(shot.height)]
                })
                .filter(|b| *b != [0, 0, shot.width, shot.height]);
            let crop = bounds.map(|[l, t, r, b]| {
                let a = to_pdf(l as f64, t as f64);
                let b = to_pdf(r as f64, b as f64);
                [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
            });
            let [left, top, right, bottom] = bounds.unwrap_or([0, 0, shot.width, shot.height]);
            let rgb = image::imageops::crop_imm(&rgb, left, top, right - left, bottom - top).to_image();
            let rotation = if self.settings.auto_rotate {
                match ocr.orientation(&rgb) {
                    Ok(r) => r,
                    Err(e) => {
                        out.push(skip(&e.to_string()));
                        continue;
                    }
                }
            } else {
                0
            };
            let mut correction = Correction { rotation, deskew: 0.0 };
            let turned = correction.apply(&rgb);
            if self.settings.deskew {
                correction.deskew = pdfcraft_ocr::scan::deskew(&turned);
            }
            let corrected = Correction { rotation: 0, ..correction }.apply(&turned);
            let result = if self.settings.recognize { ocr.recognize_language(&corrected, &self.settings.language) } else { Ok(Vec::new()) };
            let lines = match result {
                Ok(l) => l,
                Err(e) => {
                    out.push(skip(&e.to_string()));
                    continue;
                }
            };
            let to_user = |x: f32, y: f32| {
                let [x, y] = correction.to_source(x as f64, y as f64, rgb.width() as f64, rgb.height() as f64);
                to_pdf(x + left as f64, y + top as f64)
            };
            let words = lines.iter().flat_map(|l| &l.words).map(|w| PlacedWord::place(w, to_user)).collect();
            out.push(OcrPage { page, words, skipped: None, correction, crop, source: self.bytes.clone() });
        }
        progress(total, total);
        out
    }
}

impl Session {
    /// Capture what recognising `pages` (0-based; empty = all) of document `id` needs.
    pub fn ocr_job(&self, id: DocId, pages: &[usize], settings: OcrSettings) -> Option<OcrJob> {
        let doc = self.get(id)?;
        let all = doc.info.pages.len();
        let pages: Vec<usize> = if pages.is_empty() { (0..all).collect() } else { pages.iter().copied().filter(|p| *p < all).collect() };
        let native = (0..all)
            .map(|p| {
                if !pages.contains(&p) {
                    return None;
                }
                doc.page_images(p)
                    .iter()
                    .filter(|i| i.width > 1 && i.height > 1)
                    .map(|i| {
                        let m = i.matrix;
                        let (w, h) = (m[0].hypot(m[1]), m[2].hypot(m[3]));
                        (i.width as f64 / w.max(1e-6)).max(i.height as f64 / h.max(1e-6)) as f32
                    })
                    .reduce(f32::max)
            })
            .collect();
        Some(OcrJob { bytes: doc.bytes.clone(), password: doc.password.clone(), infos: doc.info.pages.clone(), native, pages, settings })
    }

    /// Add the words found by [`OcrJob::run`] as one undoable step. Returns the number of words.
    pub fn apply_ocr(&mut self, id: DocId, found: &[OcrPage]) -> Result<usize, EditError> {
        let doc = self.get(id).ok_or_else(|| EditError::Invalid("no such document".into()))?;
        if found.iter().any(|p| !Arc::ptr_eq(&p.source, &doc.bytes) && p.source.as_ref() != doc.bytes.as_ref()) {
            return Err(EditError::Invalid("the document changed during scan processing; run it again".into()));
        }
        let mut edits = Vec::new();
        for p in found.iter().filter(|p| p.skipped.is_none()) {
            if !p.words.is_empty() {
                edits.push(Edit::AddOcrText { page: p.page, words: p.words.clone() });
            }
            if let Some(rect) = p.crop {
                edits.push(Edit::SetPageBox { pages: vec![p.page], which: crate::PageBox::Crop, spec: crate::BoxSpec::Rect(rect) });
            }
            if p.correction.rotation != 0 {
                edits.push(Edit::RotatePages { pages: vec![p.page], degrees: i64::from(p.correction.rotation) });
            }
            if p.correction.deskew.abs() >= 0.05 {
                edits.push(Edit::DeskewPage { page: p.page, degrees: p.correction.deskew });
            }
        }
        let words = found.iter().map(|p| p.words.len()).sum();
        if !edits.is_empty() {
            self.apply(id, Edit::Batch { label: if words > 0 { "Recognize text" } else { "Correct scans" }.into(), edits })?;
        }
        Ok(words)
    }

    /// Recognise text on `pages` and add it, in one call (the CLI and agents use this).
    pub fn recognize_text(&mut self, id: DocId, pages: &[usize], settings: OcrSettings) -> Result<Vec<OcrPage>, String> {
        if !settings.dpi.is_finite() || !(72.0..=600.0).contains(&settings.dpi) {
            return Err("OCR resolution must be between 72 and 600 dpi".into());
        }
        if !LANGUAGES.iter().any(|l| l.0 == settings.language) {
            return Err(format!("unsupported language: {}", settings.language));
        }
        if !settings.crop_margin.is_finite() || !(0.0..=20.0).contains(&settings.crop_margin) {
            return Err("crop margin must be between 0 and 20 pt".into());
        }
        if let Some(doc) = self.get(id)
            && pages.iter().any(|&p| p >= doc.info.pages.len())
        {
            return Err("OCR page does not exist".into());
        }
        let job = self.ocr_job(id, pages, settings).ok_or("no such document")?;
        if let Some(why) = self.get(id).and_then(|d| d.read_only_reason.clone()) {
            return Err(why);
        }
        let ocr = engine()?;
        let found = job.run(&ocr, |_, _| true);
        self.apply_ocr(id, &found).map_err(|e| e.to_string())?;
        Ok(found)
    }
}

/// What [`recognize_file`] did to one file.
#[derive(Clone, Debug, PartialEq)]
pub struct FileResult {
    /// The new file (incrementally saved), or the original when nothing was recognised.
    pub bytes: Arc<Vec<u8>>,
    pub pages: Vec<OcrPage>,
}

impl FileResult {
    pub fn words(&self) -> usize {
        self.pages.iter().map(|p| p.words.len()).sum()
    }
}

/// Recognize text in multiple files: one PDF's bytes in, the searchable PDF out. `progress`
/// works as for [`OcrJob::run`].
pub fn recognize_file(
    name: &str,
    bytes: Arc<Vec<u8>>,
    password: Option<&str>,
    settings: OcrSettings,
    ocr: &Ocr,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<FileResult, String> {
    let mut s = Session::new();
    let id = s.open(name, None, bytes.clone(), password).map_err(|e| e.to_string())?;
    if let Some(why) = s.get(id).and_then(|d| d.read_only_reason.clone()) {
        return Err(why);
    }
    let job = s.ocr_job(id, &[], settings).ok_or("the document could not be read")?;
    let pages = job.run(ocr, progress);
    let words = s.apply_ocr(id, &pages).map_err(|e| e.to_string())?;
    if words == 0 && !pages.iter().any(|p| p.changed() && p.skipped.is_none()) {
        return Ok(FileResult { bytes, pages });
    }
    Ok(FileResult { bytes: s.save_bytes(id).map_err(|e| e.to_string())?, pages })
}
