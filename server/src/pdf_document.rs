//! Application geometry around the mature PdfCraft engine. Sources are never saved.
use super::*;
use printpdf::{
    BuiltinFont, Mm, Op, ParsedFont, PdfDocument, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};
use std::path::Path as FsPath;
#[derive(Clone, Copy, Debug)]
pub struct Rect(pub f64, pub f64, pub f64, pub f64);
impl Rect {
    pub fn from(v: &Value) -> Result<Self, ApiError> {
        let a = v
            .as_array()
            .filter(|a| a.len() == 4)
            .ok_or_else(|| bad("区域坐标应为四个数"))?;
        let n = a
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| bad("区域坐标无效"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let r = Self(n[0], n[1], n[2], n[3]);
        if r.width() <= 0.0 || r.height() <= 0.0 {
            return Err(bad("区域为空"));
        }
        Ok(r)
    }
    pub fn value(self) -> Value {
        json!([self.0, self.1, self.2, self.3])
    }
    pub fn width(self) -> f64 {
        self.2 - self.0
    }
    pub fn height(self) -> f64 {
        self.3 - self.1
    }
    pub fn contains(self, r: Self) -> bool {
        r.0 >= self.0 - 0.05 && r.1 >= self.1 - 0.05 && r.2 <= self.2 + 0.05 && r.3 <= self.3 + 0.05
    }
}
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub rect: Rect,
}
#[derive(Clone)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub rotation: i64,
    pub lines: Vec<Span>,
}
impl Page {
    pub fn bounds(&self) -> Rect {
        Rect(0., 0., self.width, self.height)
    }
    pub fn text(&self, clip: Option<Rect>) -> String {
        self.lines
            .iter()
            .filter(|l| clip.is_none_or(|r| r.contains(l.rect)))
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
pub struct Document {
    pub path: PathBuf,
    pub pages: Vec<Page>,
}
pub fn step(tool: &str, args: Value) -> Value {
    json!({"tool":tool,"args":args})
}
pub async fn info(app: &App, path: &FsPath) -> Result<Value, ApiError> {
    let r = pdf_engine::report(
        app,
        json!([
            step("doc_open", json!({"path":path})),
            step("doc_info", json!({"doc":1}))
        ]),
    )
    .await?;
    Ok(r["steps"][1]["result"][0].clone())
}
pub fn rotate_unrotated(steps: &mut Vec<Value>, pages: &[Page]) {
    for (i, p) in pages.iter().enumerate() {
        if p.rotation != 0 {
            steps.push(step(
                "page_rotate",
                json!({"doc":1,"pages":[i+1],"degrees":-p.rotation}),
            ))
        }
    }
}
impl Document {
    pub async fn load(app: &App, path: &FsPath, unrotated: bool) -> Result<Self, ApiError> {
        let meta = info(app, path).await?;
        let raw = meta["pages"]
            .as_array()
            .ok_or_else(|| bad("PDF 页面信息缺失"))?;
        if raw.is_empty() || raw.len() > 500 {
            return Err(bad("PDF 页数必须在 1–500 之间"));
        }
        let mut pages = raw
            .iter()
            .map(|v| {
                let rot = v["rotation"].as_i64().unwrap_or(0);
                let (mut w, mut h) = (
                    v["width"].as_f64().unwrap_or(0.),
                    v["height"].as_f64().unwrap_or(0.),
                );
                if unrotated && rot.rem_euclid(180) != 0 {
                    std::mem::swap(&mut w, &mut h)
                }
                Page {
                    width: w,
                    height: h,
                    rotation: rot,
                    lines: vec![],
                }
            })
            .collect::<Vec<_>>();
        for start in (0..pages.len()).step_by(20) {
            let end = (start + 20).min(pages.len());
            let mut steps = vec![step("doc_open", json!({"path":path}))];
            if unrotated {
                rotate_unrotated(&mut steps, &pages)
            }
            let offset = steps.len();
            for page in start..end {
                steps.push(step("text_lines", json!({"doc":1,"page":page+1})))
            }
            let r = pdf_engine::report(app, json!(steps)).await?;
            for i in start..end {
                let data = &r["steps"][offset + i - start]["result"][0];
                pages[i].lines = data["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| {
                        Ok(Span {
                            text: l["text"].as_str().unwrap_or("").trim().to_owned(),
                            rect: Rect::from(&l["rect"])?,
                        })
                    })
                    .collect::<Result<Vec<_>, ApiError>>()?;
            }
        }
        Ok(Self {
            path: path.into(),
            pages,
        })
    }
}
pub const RENDER_VERSION: &str = "pdfcraft-region-v2";
pub async fn render_region(
    app: &App,
    path: &FsPath,
    page: usize,
    bbox: Rect,
    dest: &FsPath,
) -> Result<Value, ApiError> {
    let meta = info(app, path).await?;
    let dims = meta["pages"]
        .as_array()
        .ok_or_else(|| bad("PDF 页面信息缺失"))?;
    let p = dims
        .get(page.wrapping_sub(1))
        .ok_or_else(|| bad("页码越界"))?;
    let rot = p["rotation"].as_i64().unwrap_or(0);
    let (mut w, mut h) = (
        p["width"].as_f64().unwrap_or(0.),
        p["height"].as_f64().unwrap_or(0.),
    );
    if rot.rem_euclid(180) != 0 {
        std::mem::swap(&mut w, &mut h)
    }
    if !Rect(0., 0., w, h).contains(bbox) {
        return Err(bad("区域超出原页范围"));
    }
    std::fs::create_dir_all(dest).map_err(internal)?;
    let tmp = tempfile::tempdir_in(dest).map_err(internal)?;
    let crop = tmp.path().join("region.pdf");
    let png = tmp.path().join("region.png");
    let mut steps = vec![step("doc_open", json!({"path":path}))];
    if rot != 0 {
        steps.push(step(
            "page_rotate",
            json!({"doc":1,"pages":[page],"degrees":-rot}),
        ))
    }
    steps.extend([
        step(
            "page_set_box",
            json!({"doc":1,"pages":[page],"box":"crop","rect":bbox.value()}),
        ),
        step("page_extract", json!({"doc":1,"pages":[page],"out":crop})),
    ]);
    pdf_engine::report(app, json!(steps)).await?;
    let dpi = (72. * (2400. / bbox.width().max(bbox.height())).min(2.)).min(144.);
    pdf_engine::run(
        app,
        &[
            "render".into(),
            crop.into_os_string(),
            "--page".into(),
            "1".into(),
            "--dpi".into(),
            dpi.to_string().into(),
            "--out".into(),
            png.clone().into_os_string(),
        ],
    )
    .await?;
    let bytes = std::fs::read(png).map_err(internal)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    std::fs::write(dest.join(format!("{hash}.png")), bytes).map_err(internal)?;
    Ok(
        json!({"image_sha256":hash,"render_version":RENDER_VERSION,"page":page,"bbox":bbox.value(),"filename":format!("{hash}.png")}),
    )
}
/// Only generated packets are stripped; immutable sources are never rewritten.
pub fn strip_interactive(path: &FsPath) -> Result<(), ApiError> {
    let mut doc = lopdf::Document::load(path).map_err(internal)?;
    for (_, id) in doc.get_pages() {
        if let Ok(d) = doc.get_object_mut(id).and_then(|v| v.as_dict_mut()) {
            for k in [b"Annots".as_slice(), b"AA", b"Metadata"] {
                d.remove(k);
            }
        }
    }
    if let Ok(root) = doc.trailer.get(b"Root").and_then(|v| v.as_reference()) {
        if let Ok(d) = doc.get_object_mut(root).and_then(|v| v.as_dict_mut()) {
            for k in [
                b"AcroForm".as_slice(),
                b"OpenAction",
                b"AA",
                b"Names",
                b"Outlines",
            ] {
                d.remove(k);
            }
        }
    }
    doc.prune_objects();
    doc.save(path).map_err(internal)?;
    Ok(())
}
#[derive(Clone)]
pub struct Text {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub text: String,
}
pub fn text(x: f32, y: f32, size: f32, s: impl ToString) -> Text {
    Text {
        x,
        y,
        size,
        text: s.to_string(),
    }
}
/// printpdf is used only for our headings/notes, never to reinterpret source art.
pub fn write_text_pdf(
    app: &App,
    path: &FsPath,
    title: &str,
    pages: &[Vec<Text>],
) -> Result<(), ApiError> {
    let mut doc = PdfDocument::new(title);
    let needs_unicode = pages
        .iter()
        .flatten()
        .any(|l| l.text.chars().any(|c| c as u32 > 255));
    let font = if needs_unicode {
        let bytes = std::fs::read(
            app.root
                .join("server/assets/fonts/StudyNotoSansSC-Regular.ttf"),
        )
        .map_err(internal)?;
        Some(
            ParsedFont::from_bytes(&bytes, 0, &mut vec![])
                .ok_or_else(|| bad("中文字体无法读取"))?,
        )
    } else {
        None
    };
    if let Some(f) = &font {
        for c in pages.iter().flatten().flat_map(|l| l.text.chars()) {
            if f.lookup_glyph_index(c as u32).is_none() {
                return Err(bad(format!("导出字体不支持字符 {c}，请调整标题或笔记")));
            }
        }
    }
    let id = font.as_ref().map(|f| doc.add_font(f));
    let mut out = vec![];
    for lines in pages {
        let mut ops = vec![];
        for l in lines {
            if l.text.is_empty() {
                continue;
            }
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point {
                        x: Pt(l.x),
                        y: Pt(842. - l.y),
                    },
                },
            ]);
            if let Some(id) = &id {
                ops.extend([
                    Op::SetFontSize {
                        size: Pt(l.size),
                        font: id.clone(),
                    },
                    Op::WriteText {
                        items: vec![TextItem::Text(l.text.clone())],
                        font: id.clone(),
                    },
                ])
            } else {
                ops.extend([
                    Op::SetFontSizeBuiltinFont {
                        size: Pt(l.size),
                        font: BuiltinFont::Helvetica,
                    },
                    Op::WriteTextBuiltinFont {
                        items: vec![TextItem::Text(l.text.clone())],
                        font: BuiltinFont::Helvetica,
                    },
                ])
            }
            ops.push(Op::EndTextSection)
        }
        out.push(PdfPage::new(
            Mm(595. / 72. * 25.4),
            Mm(842. / 72. * 25.4),
            ops,
        ));
    }
    let bytes = doc
        .with_pages(out)
        .save(&PdfSaveOptions::default(), &mut vec![]);
    std::fs::write(path, bytes).map_err(internal)
}
pub fn wrap(s: &str, max_width: f32, size: f32) -> Vec<String> {
    let mut out = vec![];
    for line in s.lines() {
        let mut buf = String::new();
        let mut width = 0.;
        for c in line.chars() {
            // Conservative advance bound; words wrap at whitespace where possible.
            let w = if c as u32 > 255 {
                size
            } else if "ilI.,' :;!|".contains(c) {
                size * 0.3
            } else if "MW@%".contains(c) {
                size * 0.9
            } else {
                size * 0.6
            };
            if width + w > max_width && !buf.is_empty() {
                let split = buf.rfind(' ').filter(|i| *i > buf.len() / 2);
                if let Some(i) = split {
                    let tail = buf.split_off(i + 1);
                    buf.truncate(i);
                    out.push(buf);
                    buf = tail;
                    width = buf
                        .chars()
                        .map(|c| if c as u32 > 255 { size } else { size * 0.6 })
                        .sum();
                } else {
                    out.push(std::mem::take(&mut buf));
                    width = 0.;
                }
            }
            buf.push(c);
            width += w;
        }
        out.push(buf);
    }
    out
}

impl Document {
    /// Boundary-only OCR. It never publishes transcription or changes a source PDF.
    pub async fn prepare_page(
        &mut self,
        app: &App,
        index: usize,
        kind: &str,
    ) -> Result<(), ApiError> {
        if index >= self.pages.len() {
            return Ok(());
        }
        let cache_dir = app.data.join("renders").join("geometry-v5");
        std::fs::create_dir_all(&cache_dir).map_err(internal)?;
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(&self.path).map_err(internal)?)
        );
        let cache = cache_dir.join(format!("{hash}-{}.json", index + 1));
        if let Ok(bytes) = std::fs::read(&cache) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(lines) = v["lines"].as_array() {
                    self.pages[index].lines = lines
                        .iter()
                        .map(|l| {
                            Ok(Span {
                                text: l["text"].as_str().unwrap_or("").into(),
                                rect: Rect::from(&l["rect"])?,
                            })
                        })
                        .collect::<Result<Vec<_>, ApiError>>()?;
                    return Ok(());
                }
            }
        }
        let body_letters = self.pages[index]
            .lines
            .iter()
            .filter(|l| l.rect.1 > 50. && l.rect.3 < self.pages[index].height - 55.)
            .flat_map(|l| l.text.chars())
            .filter(char::is_ascii_alphabetic)
            .count();
        let labels = regex::Regex::new(r"^\d+(?:\([a-z0-9]+\))*\.?(?:\s|$)").unwrap();
        let has_label = self.pages[index].lines.iter().any(|l| {
            l.rect.0 >= 30.
                && l.rect.0 <= 110.
                && l.rect.1 > 50.
                && l.rect.3 < self.pages[index].height - 55.
                && labels.is_match(&l.text)
        });
        if self.pages[index].lines.is_empty() || !has_label || (kind == "qp" && body_letters < 80) {
            self.pages[index].lines.clear();
            let tmp = tempfile::tempdir_in(&cache_dir).map_err(internal)?;
            let png = tmp.path().join("page.png");
            pdf_engine::run(
                app,
                &[
                    "render".into(),
                    self.path.clone().into_os_string(),
                    "--page".into(),
                    (index + 1).to_string().into(),
                    "--dpi".into(),
                    "150".into(),
                    "--out".into(),
                    png.clone().into_os_string(),
                ],
            )
            .await?;
            let binary = std::env::var_os("STUDY_TESSERACT_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/tesseract"));
            let mut cmd = tokio::process::Command::new(binary);
            cmd.args([
                png.as_os_str(),
                std::ffi::OsStr::new("stdout"),
                std::ffi::OsStr::new("-l"),
                std::ffi::OsStr::new("eng"),
                std::ffi::OsStr::new("--psm"),
                std::ffi::OsStr::new("6"),
                std::ffi::OsStr::new("tsv"),
            ])
            .kill_on_drop(true);
            let out = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
                .await
                .map_err(|_| bad("本地题号定位 OCR 超时"))?
                .map_err(|_| bad("本地 OCR 引擎不可用，无法可靠定位此原卷边界"))?;
            if !out.status.success() {
                return Err(bad("本地 OCR 无法读取原卷边界"));
            }
            let raw = String::from_utf8_lossy(&out.stdout);
            let scale = 72. / 150.;
            for line in raw.lines().skip(1) {
                let cols = line.splitn(12, '\t').collect::<Vec<_>>();
                if cols.len() != 12 || cols[0] != "5" || cols[11].trim().is_empty() {
                    continue;
                }
                let dims = cols[6..10]
                    .iter()
                    .map(|s| s.parse::<f64>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(internal)?;
                self.pages[index].lines.push(Span {
                    text: cols[11].trim().into(),
                    rect: Rect(
                        dims[0] * scale,
                        dims[1] * scale,
                        (dims[0] + dims[2]) * scale,
                        (dims[1] + dims[3]) * scale,
                    ),
                });
            }
        }
        // PdfCraft can merge a bold number and a same-font part label into one line.
        // Ask the engine for the exact number glyph box before removing/replacing it.
        let prefix = regex::Regex::new(r"^(\d+(?:\([a-z0-9]+\))*)(?:\.?\s+)").unwrap();
        let merged = self.pages[index]
            .lines
            .iter()
            .filter(|l| l.rect.0 >= 30. && l.rect.0 <= 85.)
            .filter_map(|l| {
                prefix.captures(&l.text).map(|c| {
                    (
                        if kind == "qp" {
                            c[1].split('(').next().unwrap().to_owned()
                        } else {
                            c[1].to_owned()
                        },
                        l.rect,
                    )
                })
            })
            .collect::<Vec<_>>();
        if !merged.is_empty() {
            let mut steps = vec![step("doc_open", json!({"path":self.path}))];
            for (n, _) in &merged {
                steps.push(step(
                    "text_find",
                    json!({"doc":1,"query":n,"start_page":index+1,"limit":1000}),
                ));
            }
            let result = pdf_engine::report(app, json!(steps)).await?;
            for (i, (n, line)) in merged.iter().enumerate() {
                let data = &result["steps"][i + 1]["result"][0];
                if data["complete"] != true || data["warning_count"].as_u64().unwrap_or(1) != 0 {
                    return Err(bad("题号文字位置读取不完整"));
                }
                for m in data["matches"].as_array().into_iter().flatten() {
                    if m["page"] != index + 1 || m["text"] != *n {
                        continue;
                    }
                    for r in m["rects"].as_array().into_iter().flatten() {
                        let r = Rect::from(r)?;
                        if r.0 >= line.0 - 1. && r.0 < line.0 + 3. && (r.1 - line.1).abs() < 4. {
                            self.pages[index].lines.push(Span {
                                text: n.clone(),
                                rect: r,
                            });
                        }
                    }
                }
            }
        }
        self.pages[index].lines.sort_by(|a, b| {
            a.rect
                .1
                .total_cmp(&b.rect.1)
                .then(a.rect.0.total_cmp(&b.rect.0))
        });
        let lines = self.pages[index]
            .lines
            .iter()
            .map(|l| json!({"text":l.text,"rect":l.rect.value()}))
            .collect::<Vec<_>>();
        std::fs::write(cache,serde_json::to_vec(&json!({"source_sha256":hash,"page":index+1,"method":"PdfCraft native glyph positions; Tesseract 150 dpi PSM 6 only when native lines absent","purpose":"boundary location only; not AI question recognition","lines":lines})).map_err(internal)?).map_err(internal)?;
        Ok(())
    }
}

/// PdfCraft's PDF stamp uses the source MediaBox for its form boundary.
/// Tighten our already-redacted working slice so outside white redaction fills
/// cannot paint across headings or neighbouring slices when flattened.
pub fn tighten_slice_media(path: &FsPath) -> Result<(), ApiError> {
    let mut doc = lopdf::Document::load(path).map_err(internal)?;
    for (_, id) in doc.get_pages() {
        let raw = doc
            .get_object(id)
            .and_then(|v| v.as_dict())
            .and_then(|d| d.get(b"CropBox"))
            .map_err(internal)?
            .clone();
        let crop = match raw {
            lopdf::Object::Reference(id) => doc.get_object(id).map_err(internal)?.clone(),
            v => v,
        };
        if crop.as_array().is_err() {
            return Err(bad("裁片页面边界无效"));
        }
        doc.get_object_mut(id)
            .and_then(|v| v.as_dict_mut())
            .map_err(internal)?
            .set("MediaBox", crop);
    }
    doc.save(path).map_err(internal)?;
    Ok(())
}
