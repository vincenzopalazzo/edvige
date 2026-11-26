use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use image::{DynamicImage, GenericImageView, ImageDecoder};
use rmcp::model::{Annotations, CallToolResult, ContentBlock, TextContent};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::edit::resolve_path;

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// Longest side, in pixels, of the image `read_image` hands to the model.
/// Office scanners produce 1200-dpi pages (about 10k x 14k px): vision
/// backends refuse to decode them, and every image stays in the conversation
/// history. Override with `GOOSE_READ_IMAGE_MAX_SIDE`; 0 sends images as-is.
const DEFAULT_MAX_SIDE: u32 = 2048;
/// JPEG quality for re-encoded opaque images: text stays legible.
const JPEG_QUALITY: u8 = 85;

fn configured_max_side() -> u32 {
    crate::config::Config::global()
        .get_param::<u32>("GOOSE_READ_IMAGE_MAX_SIDE")
        .unwrap_or(DEFAULT_MAX_SIDE)
}

fn visible_text(text: impl Into<String>) -> ContentBlock {
    ContentBlock::Text(
        TextContent::new(text).with_annotations(Annotations::default().with_priority(0.0)),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImageReadParams {
    /// Local file path or http(s) URL of the image to load.
    pub source: String,
    /// Optional crop rectangle in pixels of the original image (see originalWidth and
    /// originalHeight), measured from the top-left corner. Use it to zoom in and get more
    /// details: the crop is taken at full resolution, then scaled to the size limit.
    #[serde(default)]
    pub crop: Option<CropParams>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CropParams {
    /// Left edge of the crop rectangle in pixels.
    pub x: u32,
    /// Top edge of the crop rectangle in pixels.
    pub y: u32,
    /// Width of the crop rectangle in pixels.
    pub width: u32,
    /// Height of the crop rectangle in pixels.
    pub height: u32,
}

pub struct ImageTool;

impl ImageTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn image_read_with_cwd(
        &self,
        params: ImageReadParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        match load_image(&params, working_dir, configured_max_side()).await {
            Ok(loaded) => {
                let mut result = CallToolResult::success(vec![
                    visible_text(loaded.summary(&params.source)),
                    ContentBlock::image(loaded.data, loaded.mime_type.clone()),
                ]);
                result.structured_content = Some(json!({
                    "source": params.source,
                    "mimeType": loaded.mime_type,
                    "width": loaded.width,
                    "height": loaded.height,
                    "bytes": loaded.bytes_len,
                    "originalWidth": loaded.original_width,
                    "originalHeight": loaded.original_height,
                    "crop": params.crop,
                    "downscaled": loaded.downscaled,
                    "maxSide": loaded.max_side,
                }));
                result
            }
            Err(error) => CallToolResult::error(vec![visible_text(format!("Error: {error}"))]),
        }
    }
}

impl Default for ImageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct LoadedImage {
    data: String,
    mime_type: String,
    bytes_len: usize,
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
    cropped: bool,
    downscaled: bool,
    max_side: u32,
}

impl LoadedImage {
    fn summary(&self, source: &str) -> String {
        let note = match (self.cropped, self.downscaled) {
            (false, false) => String::new(),
            (true, false) => format!(
                " Cropped from {}x{} to {}x{}.",
                self.original_width, self.original_height, self.width, self.height
            ),
            (false, true) => format!(
                " Downscaled from {}x{} to {}x{} (longest side {} px).",
                self.original_width, self.original_height, self.width, self.height, self.max_side
            ),
            (true, true) => format!(
                " Cropped from {}x{}, then downscaled to {}x{} (longest side {} px).",
                self.original_width, self.original_height, self.width, self.height, self.max_side
            ),
        };

        format!(
            "Loaded image from {source} ({} bytes, {}, {}x{}).{note}",
            self.bytes_len, self.mime_type, self.width, self.height
        )
    }
}

async fn load_image(
    params: &ImageReadParams,
    working_dir: Option<&Path>,
    max_side: u32,
) -> Result<LoadedImage, String> {
    if params.source.trim().is_empty() {
        return Err("source cannot be empty".to_string());
    }

    let bytes = load_image_bytes(&params.source, working_dir).await?;
    ensure_image_size(bytes.len() as u64)?;

    let format = image::guess_format(&bytes).map_err(|_| {
        "unsupported image format; supported formats are png, jpeg, gif, and webp".to_string()
    })?;
    let mime_type = mime_type(format)?;
    let (mut image, (original_width, original_height), decoded_smaller) =
        decode(&bytes, format, max_side, params.crop.is_none())?;

    let cropped = params.crop.is_some();
    if let Some(crop) = &params.crop {
        validate_crop(crop, original_width, original_height)?;
        image = image.crop_imm(crop.x, crop.y, crop.width, crop.height);
    }
    let (width, height) = image.dimensions();
    let too_big = max_side > 0 && width.max(height) > max_side;
    if too_big {
        image = shrink(&image, max_side);
    }
    let downscaled = too_big || decoded_smaller;

    if !cropped && !downscaled {
        return Ok(LoadedImage {
            data: base64::prelude::BASE64_STANDARD.encode(&bytes),
            mime_type: mime_type.to_string(),
            bytes_len: bytes.len(),
            width: original_width,
            height: original_height,
            original_width,
            original_height,
            cropped: false,
            downscaled: false,
            max_side,
        });
    }

    let (encoded, encoded_mime) = encode(&image)?;
    ensure_image_size(encoded.len() as u64)?;
    let (width, height) = image.dimensions();
    Ok(LoadedImage {
        data: base64::prelude::BASE64_STANDARD.encode(&encoded),
        mime_type: encoded_mime.to_string(),
        bytes_len: encoded.len(),
        width,
        height,
        original_width,
        original_height,
        cropped,
        downscaled,
        max_side,
    })
}

/// Decode `bytes`; returns the image, the ORIGINAL dimensions, and whether the
/// decoder already produced a smaller image. A large JPEG is decoded at 1/2,
/// 1/4 or 1/8 of its size straight from the DCT (no full-size buffer, which
/// for a 1200-dpi page is ~420 MB, close to the 512 MB decode limit). Not
/// with a crop: its coordinates are in original pixels.
fn decode(
    bytes: &[u8],
    format: image::ImageFormat,
    max_side: u32,
    may_decode_smaller: bool,
) -> Result<(DynamicImage, (u32, u32), bool), String> {
    let decode_error = |error: image::ImageError| format!("failed to decode image: {error}");
    if format == image::ImageFormat::Jpeg && may_decode_smaller && max_side > 0 {
        let mut decoder =
            image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
        let (width, height) = decoder.dimensions();
        if width.max(height) > max_side {
            let (target_width, target_height) = fit_within(width, height, max_side);
            let clamp = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
            let (scaled_width, _) = decoder
                .scale(clamp(target_width), clamp(target_height))
                .map_err(decode_error)?;
            let smaller = u32::from(scaled_width) < width;
            let image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
            return Ok((image, (width, height), smaller));
        }
    }
    let image = image::load_from_memory_with_format(bytes, format).map_err(decode_error)?;
    let dimensions = image.dimensions();
    Ok((image, dimensions, false))
}

/// `(width, height)` scaled so the longest side is `max_side`, aspect kept.
fn fit_within(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_side {
        return (width, height);
    }
    let scale = |v: u32| ((u64::from(v) * u64::from(max_side)) / u64::from(longest)).max(1) as u32;
    (scale(width), scale(height))
}

/// Downscale so the longest side is `max_side`. Big reductions use the fast
/// area-averaging thumbnail (good for text); small ones a triangle filter,
/// where the thumbnail would alias.
fn shrink(image: &DynamicImage, max_side: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width.max(height) >= max_side.saturating_mul(2) {
        image.thumbnail(max_side, max_side)
    } else {
        image.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    }
}

/// Re-encode a cropped or downscaled image: JPEG for opaque images (a PNG of
/// a scan is several times larger), PNG when there is transparency.
fn encode(image: &DynamicImage) -> Result<(Vec<u8>, &'static str), String> {
    let mut out = Cursor::new(Vec::new());
    if image.color().has_alpha() {
        image
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|error| format!("failed to encode image: {error}"))?;
        return Ok((out.into_inner(), "image/png"));
    }
    let opaque = match image {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageRgb8(_) => image.clone(),
        DynamicImage::ImageLuma16(_) => DynamicImage::ImageLuma8(image.to_luma8()),
        _ => DynamicImage::ImageRgb8(image.to_rgb8()),
    };
    opaque
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut out,
            JPEG_QUALITY,
        ))
        .map_err(|error| format!("failed to encode image: {error}"))?;
    Ok((out.into_inner(), "image/jpeg"))
}

async fn load_image_bytes(source: &str, working_dir: Option<&Path>) -> Result<Vec<u8>, String> {
    if let Ok(url) = url::Url::parse(source) {
        match url.scheme() {
            "http" | "https" => load_url_bytes(url).await,
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| "invalid file URL".to_string())?;
                load_file_bytes(path)
            }
            _ => load_file_bytes(resolve_path(source, working_dir)),
        }
    } else {
        load_file_bytes(resolve_path(source, working_dir))
    }
}

async fn load_url_bytes(url: url::Url) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "goose/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/aaif-goose/goose)"
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("failed to create HTTP client: {error}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("failed to download image: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to download image: {error}"))?;

    collect_response_bytes(response, MAX_IMAGE_BYTES).await
}

async fn collect_response_bytes(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    if let Some(len) = response.content_length() {
        ensure_size_limit(len, max_bytes)?;
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("failed to read image response: {error}"))?
    {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| size_limit_error(u64::MAX, max_bytes))?;
        ensure_size_limit(next_len as u64, max_bytes)?;
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

fn load_file_bytes(path: PathBuf) -> Result<Vec<u8>, String> {
    let file =
        std::fs::File::open(path).map_err(|error| format!("failed to read image file: {error}"))?;
    let file_size = file
        .metadata()
        .map_err(|error| format!("failed to read image file: {error}"))?
        .len();
    ensure_image_size(file_size)?;

    let bytes = read_bounded(file, MAX_IMAGE_BYTES)
        .map_err(|error| format!("failed to read image file: {error}"))?;
    ensure_image_size(bytes.len() as u64)?;
    Ok(bytes)
}

fn read_bounded(reader: impl Read, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max_bytes + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn validate_crop(crop: &CropParams, image_width: u32, image_height: u32) -> Result<(), String> {
    if crop.width == 0 || crop.height == 0 {
        return Err("crop width and height must be greater than zero".to_string());
    }

    let right = crop
        .x
        .checked_add(crop.width)
        .ok_or_else(|| "crop rectangle is out of bounds".to_string())?;
    let bottom = crop
        .y
        .checked_add(crop.height)
        .ok_or_else(|| "crop rectangle is out of bounds".to_string())?;

    if right > image_width || bottom > image_height {
        return Err(format!(
            "crop rectangle {}x{} at {},{} exceeds image bounds {}x{}",
            crop.width, crop.height, crop.x, crop.y, image_width, image_height
        ));
    }

    Ok(())
}

fn ensure_image_size(len: u64) -> Result<(), String> {
    ensure_size_limit(len, MAX_IMAGE_BYTES)
}

fn ensure_size_limit(len: u64, max_bytes: u64) -> Result<(), String> {
    if len > max_bytes {
        Err(size_limit_error(len, max_bytes))
    } else {
        Ok(())
    }
}

fn size_limit_error(len: u64, max_bytes: u64) -> String {
    format!("image is too large: {len} bytes exceeds {max_bytes} byte limit")
}

fn mime_type(format: image::ImageFormat) -> Result<&'static str, String> {
    match format {
        image::ImageFormat::Png => Ok("image/png"),
        image::ImageFormat::Jpeg => Ok("image/jpeg"),
        image::ImageFormat::Gif => Ok("image/gif"),
        image::ImageFormat::WebP => Ok("image/webp"),
        _ => Err(
            "unsupported image format; supported formats are png, jpeg, gif, and webp".to_string(),
        ),
    }
}
#[cfg(test)]
mod local_file_tests {
    use super::*;

    const SMALL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    #[test]
    fn bounded_reader_accepts_limit_and_detects_extra_byte() {
        assert_eq!(read_bounded(&b"12345678"[..], 8).unwrap(), b"12345678");
        assert_eq!(read_bounded(&b"123456789"[..], 8).unwrap(), b"123456789");
    }

    #[test]
    fn bounded_reader_stops_productive_infinite_source() {
        assert_eq!(read_bounded(std::io::repeat(0), 8).unwrap().len(), 9);
    }

    #[test]
    fn oversized_sparse_local_file_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("oversized.png");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();

        let error = load_file_bytes(path).unwrap_err();

        assert_eq!(
            error,
            format!(
                "image is too large: {} bytes exceeds {MAX_IMAGE_BYTES} byte limit",
                MAX_IMAGE_BYTES + 1
            )
        );
    }

    #[tokio::test]
    async fn local_path_and_file_url_still_decode_small_image() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("small.png");
        let png = base64::prelude::BASE64_STANDARD.decode(SMALL_PNG).unwrap();
        std::fs::write(&path, &png).unwrap();
        let file_url = url::Url::from_file_path(&path).unwrap().to_string();

        for source in [path.to_string_lossy().into_owned(), file_url] {
            let loaded = load_image(
                &ImageReadParams { source, crop: None },
                None,
                DEFAULT_MAX_SIDE,
            )
            .await
            .unwrap();

            assert_eq!(loaded.mime_type, "image/png");
            assert_eq!(loaded.bytes_len, png.len());
            assert_eq!((loaded.width, loaded.height), (1, 1));
            assert_eq!(
                base64::prelude::BASE64_STANDARD
                    .decode(loaded.data)
                    .unwrap(),
                png
            );
        }
    }

    #[tokio::test]
    async fn invalid_local_file_keeps_unsupported_format_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not-an-image.bin");
        std::fs::write(&path, b"not an image").unwrap();

        let error = load_image(
            &ImageReadParams {
                source: path.to_string_lossy().into_owned(),
                crop: None,
            },
            None,
            DEFAULT_MAX_SIDE,
        )
        .await
        .unwrap_err();

        assert_eq!(
            error,
            "unsupported image format; supported formats are png, jpeg, gif, and webp"
        );
    }

    fn encode_test_image(image: &DynamicImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    /// An opaque gradient: JPEG-encodable, non-trivial pixels.
    fn rgb_image(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8])
        }))
    }

    async fn load_file(
        bytes: &[u8],
        name: &str,
        crop: Option<CropParams>,
        max_side: u32,
    ) -> LoadedImage {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        load_image(
            &ImageReadParams {
                source: path.to_string_lossy().into_owned(),
                crop,
            },
            None,
            max_side,
        )
        .await
        .unwrap()
    }

    fn decoded_dimensions(loaded: &LoadedImage) -> (u32, u32) {
        let bytes = base64::prelude::BASE64_STANDARD
            .decode(&loaded.data)
            .unwrap();
        image::load_from_memory(&bytes).unwrap().dimensions()
    }

    #[tokio::test]
    async fn large_jpeg_is_downscaled_to_the_max_side() {
        // like a scanned page, just smaller: the JPEG is decoded at 1/2 size
        // by the DCT, then resized to fit 1024
        let jpeg = encode_test_image(&rgb_image(4000, 3000), image::ImageFormat::Jpeg);
        let loaded = load_file(&jpeg, "scan.jpg", None, 1024).await;

        assert!(loaded.downscaled && !loaded.cropped);
        assert_eq!((loaded.width, loaded.height), (1024, 768));
        assert_eq!(
            (loaded.original_width, loaded.original_height),
            (4000, 3000)
        );
        assert_eq!(loaded.mime_type, "image/jpeg");
        assert_eq!(decoded_dimensions(&loaded), (1024, 768));
        assert!(loaded.bytes_len < jpeg.len());
        assert!(loaded
            .summary("scan.jpg")
            .contains("Downscaled from 4000x3000 to 1024x768"));
    }

    #[tokio::test]
    async fn portrait_page_keeps_its_aspect_ratio() {
        let jpeg = encode_test_image(&rgb_image(1240, 1754), image::ImageFormat::Jpeg);
        let loaded = load_file(&jpeg, "page.jpg", None, 1000).await;

        assert_eq!(loaded.height, 1000);
        // 1240 * 1000 / 1754 = 706.95; the resampler may round either way
        assert!((706..=707).contains(&loaded.width), "{}", loaded.width);
    }

    #[tokio::test]
    async fn transparent_png_stays_png_when_downscaled() {
        let rgba = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(3000, 1000, |x, _| {
            image::Rgba([0, 0, 0, (x % 256) as u8])
        }));
        let png = encode_test_image(&rgba, image::ImageFormat::Png);
        let loaded = load_file(&png, "overlay.png", None, 1000).await;

        assert_eq!(loaded.mime_type, "image/png");
        assert_eq!((loaded.width, loaded.height), (1000, 333));
    }

    #[tokio::test]
    async fn small_image_and_disabled_limit_pass_through_untouched() {
        let jpeg = encode_test_image(&rgb_image(800, 600), image::ImageFormat::Jpeg);
        let small = load_file(&jpeg, "small.jpg", None, 2048).await;
        assert!(!small.downscaled);
        assert_eq!(
            base64::prelude::BASE64_STANDARD
                .decode(&small.data)
                .unwrap(),
            jpeg
        );

        let big = encode_test_image(&rgb_image(3000, 2000), image::ImageFormat::Jpeg);
        let off = load_file(&big, "big.jpg", None, 0).await;
        assert!(!off.downscaled);
        assert_eq!((off.width, off.height), (3000, 2000));
        assert_eq!(
            base64::prelude::BASE64_STANDARD.decode(&off.data).unwrap(),
            big
        );
    }

    #[tokio::test]
    async fn crop_uses_original_pixels_then_scales() {
        let jpeg = encode_test_image(&rgb_image(4000, 3000), image::ImageFormat::Jpeg);
        let crop = CropParams {
            x: 1000,
            y: 0,
            width: 3000,
            height: 3000,
        };
        let loaded = load_file(&jpeg, "scan.jpg", Some(crop), 1024).await;

        assert!(loaded.cropped && loaded.downscaled);
        assert_eq!((loaded.width, loaded.height), (1024, 1024));
        assert_eq!(
            (loaded.original_width, loaded.original_height),
            (4000, 3000)
        );
        // an opaque crop is re-encoded as JPEG, not as a larger PNG
        assert_eq!(loaded.mime_type, "image/jpeg");
        assert!(loaded
            .summary("scan.jpg")
            .contains("Cropped from 4000x3000, then downscaled to 1024x1024"));
    }

    #[tokio::test]
    async fn crop_outside_the_original_image_is_still_rejected() {
        let jpeg = encode_test_image(&rgb_image(4000, 3000), image::ImageFormat::Jpeg);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("scan.jpg");
        std::fs::write(&path, &jpeg).unwrap();
        let error = load_image(
            &ImageReadParams {
                source: path.to_string_lossy().into_owned(),
                crop: Some(CropParams {
                    x: 3500,
                    y: 0,
                    width: 1000,
                    height: 100,
                }),
            },
            None,
            1024,
        )
        .await
        .unwrap_err();
        assert!(error.contains("exceeds image bounds 4000x3000"), "{error}");
    }

    #[test]
    fn fit_within_keeps_aspect_and_never_reaches_zero() {
        assert_eq!(fit_within(9924, 13960, 2048), (1455, 2048));
        assert_eq!(fit_within(100, 50, 2048), (100, 50));
        assert_eq!(fit_within(100_000, 1, 2048), (2048, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const TEST_MAX_BYTES: u64 = 8;

    async fn serve_response(
        headers: &str,
        body: Vec<u8>,
    ) -> (url::Url, tokio::task::JoinHandle<()>) {
        serve_response_with_version("HTTP/1.1", headers, body).await
    }

    async fn serve_close_delimited_response(
        body: Vec<u8>,
    ) -> (url::Url, tokio::task::JoinHandle<()>) {
        serve_response_with_version("HTTP/1.0", "Connection: close\r\n", body).await
    }

    async fn serve_response_with_version(
        version: &str,
        headers: &str,
        body: Vec<u8>,
    ) -> (url::Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut response = format!("{version} 200 OK\r\n{headers}\r\n").into_bytes();
        response.extend_from_slice(&body);

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let mut buffer = [0; 1024];
                let bytes_read = stream.read(&mut buffer).await.unwrap();
                if bytes_read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..bytes_read]);
            }
            stream.write_all(&response).await.unwrap();
            stream.shutdown().await.unwrap();
        });

        (
            url::Url::parse(&format!("http://{address}/image.png")).unwrap(),
            server,
        )
    }

    async fn fetch_response(url: url::Url) -> reqwest::Response {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn response_at_limit_without_content_length_is_accepted() {
        let (url, server) = serve_close_delimited_response(b"12345678".to_vec()).await;
        let response = fetch_response(url).await;

        let bytes = collect_response_bytes(response, TEST_MAX_BYTES)
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(bytes, b"12345678");
    }

    #[tokio::test]
    async fn response_over_limit_without_content_length_is_rejected() {
        let (url, server) = serve_close_delimited_response(b"123456789".to_vec()).await;
        let response = fetch_response(url).await;

        let error = collect_response_bytes(response, TEST_MAX_BYTES)
            .await
            .unwrap_err();
        server.await.unwrap();

        assert_eq!(error, "image is too large: 9 bytes exceeds 8 byte limit");
    }

    #[tokio::test]
    async fn chunked_response_over_limit_is_rejected() {
        let body = b"4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n".to_vec();
        let (url, server) =
            serve_response("Transfer-Encoding: chunked\r\nConnection: close\r\n", body).await;
        let response = fetch_response(url).await;

        let error = collect_response_bytes(response, TEST_MAX_BYTES)
            .await
            .unwrap_err();
        server.await.unwrap();

        assert_eq!(error, "image is too large: 9 bytes exceeds 8 byte limit");
    }

    #[tokio::test]
    async fn oversized_content_length_is_rejected_before_collection() {
        let (url, server) =
            serve_response("Content-Length: 9\r\nConnection: close\r\n", Vec::new()).await;
        let response = fetch_response(url).await;

        let error = collect_response_bytes(response, TEST_MAX_BYTES)
            .await
            .unwrap_err();
        server.await.unwrap();

        assert_eq!(error, "image is too large: 9 bytes exceeds 8 byte limit");
    }

    #[tokio::test]
    async fn small_png_response_still_decodes() {
        let png = base64::prelude::BASE64_STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        let (url, server) = serve_response(
            &format!(
                "Content-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n",
                png.len()
            ),
            png.clone(),
        )
        .await;
        let params = ImageReadParams {
            source: url.to_string(),
            crop: None,
        };

        let loaded = load_image(&params, None, DEFAULT_MAX_SIDE).await.unwrap();
        server.await.unwrap();

        assert_eq!(loaded.mime_type, "image/png");
        assert_eq!(loaded.bytes_len, png.len());
        assert_eq!((loaded.width, loaded.height), (1, 1));
    }
}
