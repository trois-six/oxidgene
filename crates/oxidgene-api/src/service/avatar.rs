//! Uploaded avatars (docs/authentication.md §6.7): a JPEG, PNG or WebP
//! image, its type sniffed from its bytes, cropped to the square the user
//! chose, turned upright, scaled to 256 and 64 pixels, flattened onto white
//! and re-encoded as JPEG. Re-encoding keeps the pixels and nothing else —
//! no EXIF, GPS, XMP or ICC block of the original survives.
//!
//! Both sizes live in the media store under `users/{user_id}/avatar/`,
//! outside every tree, named by the SHA-256 of the 256-pixel rendition,
//! which is also their `ETag`.

use std::io::Cursor;
use std::sync::Arc;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgb, RgbImage};
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{AppUser, AppUserChanges, AppUserRepo};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::auth::Principal;
use crate::media::{MediaStore, sha256_hex, sniff_mime};
use crate::service::account::{require_user, visible_user};

/// The largest upload, refused before it is decoded.
pub const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;
/// The most pixels an upload may decode to, refused from its header.
const MAX_AVATAR_PIXELS: u64 = 40_000_000;
/// The renditions stored and served.
pub const AVATAR_SIZES: [u32; 2] = [256, 64];
/// The JPEG quality of the renditions.
const QUALITY: u8 = 88;

/// The square of the image the user chose, in pixels of the image as it is
/// displayed — upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// The store key of `user_id`'s avatar `sha256` at `size`.
pub fn avatar_key(user_id: Uuid, sha256: &str, size: u32) -> String {
    format!("users/{user_id}/avatar/{sha256}-{size}.jpg")
}

/// The renditions of an upload, largest first, and the digest naming them.
#[derive(Debug)]
pub struct Renditions {
    pub sha256: String,
    pub images: Vec<(u32, Vec<u8>)>,
}

/// Decode, crop, orient, scale and re-encode `bytes`. CPU-bound: run it on a
/// blocking thread.
pub fn render(bytes: &[u8], crop: Option<Crop>) -> Result<Renditions, OxidGeneError> {
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(invalid("the image is larger than 5 MiB"));
    }
    let format = match sniff_mime(bytes) {
        Some("image/jpeg") => ImageFormat::Jpeg,
        Some("image/png") => ImageFormat::Png,
        Some("image/webp") => ImageFormat::WebP,
        _ => return Err(invalid("the image must be a JPEG, PNG or WebP file")),
    };
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(|_| invalid("the image cannot be read"))?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_AVATAR_PIXELS {
        return Err(invalid("the image has more than 40 megapixels"));
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|_| invalid("the image cannot be read"))?;
    image.apply_orientation(orientation);
    let square = square(&image, crop)?;
    let cropped = image.crop_imm(square.x, square.y, square.size, square.size);
    let mut images = Vec::with_capacity(AVATAR_SIZES.len());
    for size in AVATAR_SIZES {
        let scaled = cropped.resize_exact(size, size, FilterType::Lanczos3);
        images.push((size, jpeg(&on_white(&scaled))?));
    }
    Ok(Renditions {
        sha256: sha256_hex(&images[0].1),
        images,
    })
}

/// The crop, checked against the image; the centred largest square when
/// none was chosen.
fn square(image: &DynamicImage, crop: Option<Crop>) -> Result<Crop, OxidGeneError> {
    let (width, height) = (image.width(), image.height());
    let Some(crop) = crop else {
        let size = width.min(height);
        return Ok(Crop {
            x: (width - size) / 2,
            y: (height - size) / 2,
            size,
        });
    };
    let fits = crop.size > 0
        && crop
            .x
            .checked_add(crop.size)
            .is_some_and(|right| right <= width)
        && crop
            .y
            .checked_add(crop.size)
            .is_some_and(|bottom| bottom <= height);
    if !fits {
        return Err(invalid("the crop square lies outside the image"));
    }
    Ok(crop)
}

/// `image` with any transparency flattened onto white.
fn on_white(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let blend = |channel: u8| {
            let channel = u16::from(channel);
            let alpha = u16::from(a);
            u8::try_from((channel * alpha + 255 * (255 - alpha) + 127) / 255).unwrap_or(255)
        };
        Rgb([blend(r), blend(g), blend(b)])
    })
}

fn jpeg(image: &RgbImage) -> Result<Vec<u8>, OxidGeneError> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(image)
        .map_err(|_| OxidGeneError::Internal("avatar encoding failed".into()))?;
    Ok(out)
}

fn invalid(message: &str) -> OxidGeneError {
    OxidGeneError::Validation(message.to_string())
}

/// Replace the signed-in user's avatar with `bytes` cropped to `crop`.
pub async fn set_avatar(
    db: &DatabaseConnection,
    media: &Arc<dyn MediaStore>,
    principal: &Principal,
    bytes: Vec<u8>,
    crop: Option<Crop>,
) -> Result<AppUser, OxidGeneError> {
    let user_id = require_user(principal)?.user_id;
    let renditions = tokio::task::spawn_blocking(move || render(&bytes, crop))
        .await
        .map_err(|_| OxidGeneError::Internal("avatar rendering failed".into()))??;
    for (size, image) in &renditions.images {
        media
            .put_user_bytes(&avatar_key(user_id, &renditions.sha256, *size), image)
            .await?;
    }
    replace(db, media, user_id, Some(renditions.sha256)).await
}

/// Return the signed-in user to the generated avatar.
pub async fn delete_avatar(
    db: &DatabaseConnection,
    media: &Arc<dyn MediaStore>,
    principal: &Principal,
) -> Result<AppUser, OxidGeneError> {
    let user_id = require_user(principal)?.user_id;
    replace(db, media, user_id, None).await
}

/// Point the user at `sha256`, then delete the files of the avatar it
/// replaced once that is committed.
async fn replace(
    db: &DatabaseConnection,
    media: &Arc<dyn MediaStore>,
    user_id: Uuid,
    sha256: Option<String>,
) -> Result<AppUser, OxidGeneError> {
    let previous = AppUserRepo::get(db, user_id).await?.avatar_sha256;
    let updated = AppUserRepo::update(
        db,
        user_id,
        AppUserChanges {
            avatar_sha256: Some(sha256.clone()),
            ..AppUserChanges::default()
        },
    )
    .await?;
    if let Some(previous) = previous.filter(|previous| Some(previous) != sha256.as_ref()) {
        delete_files(media.as_ref(), user_id, &previous).await;
    }
    Ok(updated)
}

/// Delete the files of avatar `sha256`. A file left behind is logged, not
/// an error: the change it belonged to is committed.
pub async fn delete_files(media: &dyn MediaStore, user_id: Uuid, sha256: &str) {
    for size in AVATAR_SIZES {
        if media
            .delete(&avatar_key(user_id, sha256, size))
            .await
            .is_err()
        {
            tracing::warn!(
                error = "avatar_cleanup",
                "An avatar file could not be deleted"
            );
        }
    }
}

/// A served avatar: where its JPEG is stored and its `ETag`.
#[derive(Debug)]
pub struct AvatarImage {
    pub key: String,
    pub etag: String,
}

/// The avatar of `user_id` at `size`, for whoever may see the user;
/// `not_found` when they have no uploaded avatar. The bytes are read by the
/// caller, once it knows the client lacks them.
pub async fn avatar_image(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
    user_id: Uuid,
    size: u32,
) -> Result<AvatarImage, OxidGeneError> {
    if !AVATAR_SIZES.contains(&size) {
        return Err(invalid("size must be 256 or 64"));
    }
    let user = visible_user(db, principal, user_id).await?;
    let sha256 = user.avatar_sha256.ok_or(OxidGeneError::NotFound {
        entity: "Avatar",
        id: user_id,
    })?;
    Ok(AvatarImage {
        key: avatar_key(user_id, &sha256, size),
        etag: format!("{sha256}-{size}"),
    })
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, Rgba, RgbaImage};

    use super::*;

    fn png(width: u32, height: u32, pixel: Rgba<u8>) -> Vec<u8> {
        let image: RgbaImage = ImageBuffer::from_pixel(width, height, pixel);
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn an_upload_becomes_two_square_jpegs() {
        let renditions = render(&png(300, 200, Rgba([10, 20, 30, 255])), None).unwrap();
        assert_eq!(renditions.sha256.len(), 64);
        for (size, bytes) in &renditions.images {
            assert_eq!(sniff_mime(bytes), Some("image/jpeg"));
            let decoded = image::load_from_memory(bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (*size, *size));
        }
    }

    #[test]
    fn transparency_is_flattened_onto_white() {
        let renditions = render(&png(64, 64, Rgba([0, 0, 0, 0])), None).unwrap();
        let decoded = image::load_from_memory(&renditions.images[1].1)
            .unwrap()
            .to_rgb8();
        let [r, g, b] = decoded.get_pixel(32, 32).0;
        assert!(r > 245 && g > 245 && b > 245, "{r} {g} {b}");
    }

    #[test]
    fn the_crop_must_lie_inside_the_image() {
        let bytes = png(100, 80, Rgba([200, 0, 0, 255]));
        assert!(
            render(
                &bytes,
                Some(Crop {
                    x: 10,
                    y: 0,
                    size: 80
                })
            )
            .is_ok()
        );
        for crop in [
            Crop {
                x: 30,
                y: 0,
                size: 80,
            },
            Crop {
                x: 0,
                y: 1,
                size: 80,
            },
            Crop {
                x: 0,
                y: 0,
                size: 0,
            },
            Crop {
                x: u32::MAX,
                y: 0,
                size: 2,
            },
        ] {
            assert!(render(&bytes, Some(crop)).is_err(), "{crop:?}");
        }
    }

    #[test]
    fn only_jpeg_png_and_webp_are_taken() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0; 32]);
        assert!(matches!(
            render(&gif, None),
            Err(OxidGeneError::Validation(_))
        ));
        assert!(matches!(
            render(&vec![0xFF; MAX_AVATAR_BYTES + 1], None),
            Err(OxidGeneError::Validation(_))
        ));
    }

    #[test]
    fn keys_are_user_scoped() {
        let user = Uuid::now_v7();
        assert_eq!(
            avatar_key(user, &"a".repeat(64), 64),
            format!("users/{user}/avatar/{}-64.jpg", "a".repeat(64))
        );
    }
}
