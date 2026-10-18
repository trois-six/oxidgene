//! Small, fictitious PDFs written by hand for the media tests.
//!
//! Shared by the integration tests (`common::pdf`) and by the unit tests of
//! `media`, which include this file by path: one writer, so both test what
//! the same files do. Every page is US Letter; nothing here is a real
//! document.

use std::fmt::Write as _;

/// An image XObject a page lists among its resources.
pub struct Image<'a> {
    /// The PDF filter name the stream is compressed with, `DCTDecode` for a
    /// JPEG.
    pub filter: &'a str,
    pub width: u32,
    pub height: u32,
    /// Further entries of the image dictionary, such as `/DecodeParms`.
    pub extra: &'a str,
    /// The compressed stream.
    pub data: &'a [u8],
    /// Whether the page's content draws it, or only lists it.
    pub painted: bool,
}

impl<'a> Image<'a> {
    /// A JPEG of `width` × `height` pixels, painted over the page.
    pub fn jpeg(data: &'a [u8], width: u32, height: u32) -> Self {
        Self {
            filter: "DCTDecode",
            width,
            height,
            extra: "/ColorSpace /DeviceRGB /BitsPerComponent 8",
            data,
            painted: true,
        }
    }
}

/// One page: the images it lists, a line of text or not, and its `/Rotate`.
#[derive(Default)]
pub struct Page<'a> {
    pub images: Vec<Image<'a>>,
    pub text: bool,
    pub rotate: i32,
}

/// A PDF of `pages`, with `trailer` appended to its trailer dictionary.
pub fn build(pages: &[Page<'_>], trailer: &str) -> Vec<u8> {
    // Objects 1 and 2 are the catalog and the page tree; each page follows
    // with its content stream, its images and its font.
    let mut objects: Vec<Vec<u8>> = vec![Vec::new(), Vec::new()];
    let mut kids = Vec::new();
    for page in pages {
        let page_id = objects.len() + 1;
        objects.push(Vec::new());
        let content_id = objects.len() + 1;
        objects.push(Vec::new());
        let mut resources = String::new();
        let mut content = String::new();
        if !page.images.is_empty() {
            resources.push_str("/XObject <<");
            for (index, image) in page.images.iter().enumerate() {
                let image_id = objects.len() + 1;
                objects.push(stream(
                    &format!(
                        "/Type /XObject /Subtype /Image /Width {} /Height {} /Filter /{} {}",
                        image.width, image.height, image.filter, image.extra
                    ),
                    image.data,
                ));
                let _ = write!(resources, " /Im{index} {image_id} 0 R");
                if image.painted {
                    let _ = writeln!(content, "q 612 0 0 792 0 0 cm /Im{index} Do Q");
                }
            }
            resources.push_str(" >>");
        }
        if page.text {
            let font_id = objects.len() + 1;
            objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
            let _ = write!(resources, " /Font << /F1 {font_id} 0 R >>");
            content.push_str("BT /F1 12 Tf 72 720 Td (A fictitious page) Tj ET\n");
        }
        objects[content_id - 1] = stream("", content.as_bytes());
        objects[page_id - 1] = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {} \
             /Resources << {resources} >> /Contents {content_id} 0 R >>",
            page.rotate
        )
        .into_bytes();
        kids.push(format!("{page_id} 0 R"));
    }
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    )
    .into_bytes();

    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(table.as_bytes());
    out
}

/// A one-page PDF painting `jpeg`, `width` × `height` pixels, over the page.
pub fn scan(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
    build(
        &[Page {
            images: vec![Image::jpeg(jpeg, width, height)],
            ..Page::default()
        }],
        "",
    )
}

/// A PDF of `count` pages holding a line of text each, and no image.
pub fn text(count: usize) -> Vec<u8> {
    let pages: Vec<Page<'_>> = (0..count)
        .map(|_| Page {
            text: true,
            ..Page::default()
        })
        .collect();
    build(&pages, "")
}

/// A one-page PDF that only opens with a password: its `/Encrypt` keys
/// match no password, the empty one included.
pub fn locked() -> Vec<u8> {
    let key = "0123456789ABCDEF".repeat(4);
    build(
        &[Page {
            text: true,
            ..Page::default()
        }],
        &format!(
            "/Encrypt << /Filter /Standard /V 1 /R 2 /Length 40 /P -4 /O <{key}> /U <{key}> >> \
             /ID [<{key}> <{key}>]"
        ),
    )
}

/// A stream object: `dict` entries plus its `/Length`, then `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}
