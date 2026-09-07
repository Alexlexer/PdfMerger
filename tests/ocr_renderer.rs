use lopdf::{Document, dictionary};
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
fn native_ocr_worker_renders_synthetic_pdf_without_ocr_installation() {
    let directory =
        std::env::temp_dir().join(format!("pdfmerger-render-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let input = directory.join("synthetic.pdf");
    let output = directory.join("render.bmp");
    let mut pdf = Document::with_version("1.5");
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! { "Type" =>"Page", "Parent" =>pages, "MediaBox" =>vec![0.into(),0.into(),300.into(),400.into()] });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" =>"Pages", "Kids" =>vec![lopdf::Object::Reference(page)], "Count" =>1 }
            .into(),
    );
    let root = pdf.add_object(dictionary! { "Type" =>"Catalog", "Pages" =>pages });
    pdf.trailer.set("Root", root);
    pdf.save(&input).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pdf-merger"))
        .arg("--local-ocr-render")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&(input, Option::<String>::None, 1, &output)).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
    let image = image::open(&output).unwrap();
    assert!(image.width() <= 2400 && image.height() <= 2400);
    assert!(image.width() > 312); // OCR gets more than a thumbnail.
    std::fs::remove_dir_all(directory).unwrap();
}
