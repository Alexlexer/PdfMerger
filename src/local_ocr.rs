//! Optional Tesseract executable. No linked OCR dependency and no network access.
use crate::{model::PreviewData, summarization::PageOcr};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn available() -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        "tesseract.exe"
    } else {
        "tesseract"
    };
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(executable))
                .collect()
        })
        .unwrap_or_default();
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(programs).join("Tesseract-OCR/tesseract.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn render_page(
    path: &Path,
    password: Option<&str>,
    number: u32,
    edge: f32,
) -> Result<PreviewData> {
    if fs::metadata(path)?.len() > 512 * 1024 * 1024 {
        bail!("PDF exceeds rendering size limit");
    }
    let pdf =
        hayro::hayro_syntax::Pdf::new_with_password(fs::read(path)?, password.unwrap_or_default())
            .map_err(|_| anyhow::anyhow!("Could not open source PDF"))?;
    let page = pdf
        .pages()
        .get(number.checked_sub(1).context("Invalid page")? as usize)
        .context("Source page no longer exists")?;
    let (width, height) = page.render_dimensions();
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        bail!("Invalid PDF page dimensions");
    }
    let scale = (edge.clamp(256.0, 2400.0) / width.max(height)).min(3.0);
    let png = hayro::render(
        page,
        &hayro::RenderCache::new(),
        &Default::default(),
        &hayro::RenderSettings {
            x_scale: scale,
            y_scale: scale,
            ..Default::default()
        },
    )
    .into_png()
    .map_err(|_| anyhow::anyhow!("Could not render source page"))?;
    let image = image::load_from_memory(&png)?.into_rgba8();
    Ok(PreviewData::new(
        image.width() as usize,
        image.height() as usize,
        image.into_raw(),
    ))
}
/// Internal renderer subprocess keeps slow rendering cancellable without touching the GUI.
/// Passwords travel through stdin, never process arguments or logs.
pub fn render_worker() -> Result<()> {
    use std::io::Read;
    let mut input = zeroize::Zeroizing::new(String::new());
    std::io::stdin()
        .take(64 * 1024)
        .read_to_string(&mut input)?;
    let (path, password, page, output): (
        PathBuf,
        Option<zeroize::Zeroizing<String>>,
        u32,
        PathBuf,
    ) = {
        let (path, password, page, output): (PathBuf, Option<String>, u32, PathBuf) =
            serde_json::from_str(&input)?;
        (path, password.map(zeroize::Zeroizing::new), page, output)
    };
    let preview = render_page(&path, password.as_ref().map(|s| s.as_str()), page, 2400.0)?;
    image::save_buffer(
        &output,
        &preview.rgba,
        preview.size[0] as u32,
        preview.size[1] as u32,
        image::ColorType::Rgba8,
    )?;
    Ok(())
}

fn render_for_ocr(
    path: &Path,
    password: Option<&str>,
    page: u32,
    output: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    use std::io::Write;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--local-ocr-render")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = ChildGuard(command.spawn()?);
    let payload = zeroize::Zeroizing::new(serde_json::to_vec(&(path, password, page, output))?);
    child
        .0
        .stdin
        .take()
        .context("Renderer stdin unavailable")?
        .write_all(&payload)?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if cancelled() {
            bail!("OCR rendering cancelled");
        }
        if Instant::now() >= deadline {
            bail!("OCR rendering exceeded 45 seconds");
        }
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                bail!("OCR renderer failed");
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Render in the bounded, cancellable helper, then encode a local image for a vision model.
pub fn vision_image(
    path: &Path,
    password: Option<&str>,
    page: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    if cancelled() {
        bail!("Page reading cancelled");
    }
    let scratch = Scratch::new()?;
    let output = scratch.0.join("page.bmp");
    render_for_ocr(path, password, page, &output, cancelled)?;
    if cancelled() {
        bail!("Page reading cancelled");
    }
    let rgb = image::open(&output)?.into_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&rgb)?;
    if jpeg.len() > 8 * 1024 * 1024 {
        bail!("Rendered page exceeds the local vision image limit");
    }
    Ok(jpeg)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdfmerger-ocr-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let builder = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder
            }
            #[cfg(not(unix))]
            {
                fs::DirBuilder::new()
            }
        };
        builder.create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct TesseractOcr {
    pub path: PathBuf,
    pub password: Option<zeroize::Zeroizing<String>>,
    pub executable: Option<PathBuf>,
    pub language: String,
    pub pages_left: usize,
}
impl PageOcr for TesseractOcr {
    fn recognize(&mut self, page: u32, cancelled: &dyn Fn() -> bool) -> Result<String> {
        if cancelled() {
            bail!("OCR cancelled");
        }
        let executable = self.executable.as_ref().context("Tesseract unavailable: install it and the required language data, then restart PdfMerger")?;
        if self.pages_left == 0 {
            bail!("OCR page limit reached (50 pages per source per job)");
        }
        self.pages_left -= 1;
        if self.language.is_empty()
            || !self
                .language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+'))
        {
            bail!("Invalid OCR language code");
        }
        let scratch = Scratch::new()?;
        let image_path = scratch.0.join("page.bmp");
        render_for_ocr(
            &self.path,
            self.password.as_ref().map(|s| s.as_str()),
            page,
            &image_path,
            cancelled,
        )?;
        let output = scratch.0.join("recognized");
        let mut command = Command::new(executable);
        command
            .arg(&image_path)
            .arg(&output)
            .args(["-l", &self.language, "--psm", "3", "txt"])
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = ChildGuard(command.spawn().context("Could not start local Tesseract")?);
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if cancelled() {
                bail!("OCR cancelled");
            }
            if Instant::now() >= deadline {
                bail!("OCR exceeded 45 seconds for this page");
            }
            if fs::metadata(output.with_extension("txt")).is_ok_and(|m| m.len() > 1024 * 1024) {
                bail!("OCR output exceeded 1 MiB");
            }
            if let Some(status) = child.0.try_wait()? {
                if !status.success() {
                    bail!("Tesseract failed; check installed language data");
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        use std::io::Read;
        let mut text = String::new();
        fs::File::open(output.with_extension("txt"))?
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 1024 * 1024 {
            bail!("OCR output exceeded 1 MiB");
        }
        Ok(text)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_ocr_and_cancellation_do_not_open_pdf() {
        let mut engine = TesseractOcr {
            path: "does-not-exist.pdf".into(),
            password: None,
            executable: None,
            language: "eng".into(),
            pages_left: 1,
        };
        assert!(
            engine
                .recognize(1, &|| false)
                .unwrap_err()
                .to_string()
                .contains("unavailable")
        );
        assert!(
            engine
                .recognize(1, &|| true)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
    }
}
