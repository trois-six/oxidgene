//! REST handlers for the Geneanet import wizard.
//!
//! Four calls, one per step that needs the server. The login step — signing in and
//! collecting the person↔photo mapping — has no endpoint at all: it happens
//! inside the WebView the user authenticated in, because that is the only
//! place a Geneanet session exists. What arrives here is its output.
//!
//! Operations that exchange **filesystem paths** are guarded by a runtime
//! capability enabled only by the desktop application, where the server runs
//! in-process and reads the very files the user picked. A preview or a plan
//! naming no archive path, reading a saved session, and importing one sent
//! with the request exchange none, and are served everywhere.

use super::authorize::Viewer;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};
use uuid::Uuid;

use oxidgene_geneanet::session;

use super::dto::{
    DecodeSessionResponse, EncodeSessionRequest, FileImportStartedResponse, GeneanetImportRequest,
    GeneanetPlanResponse, GeneanetPreviewRequest, GeneanetPreviewResponse, ImportGenewebQuery,
    IndexArchivesRequest, IndexArchivesResponse, IndexedArchive, InspectGenewebResponse,
    NeededMedia, ReadSessionResponse, ReleaseSessionMediaRequest,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::geneanet;

/// Default `origin_file` when the client sends no `?filename=`.
const DEFAULT_ORIGIN_FILE: &str = "import.gw";

/// POST /api/v1/geneweb/inspect
///
/// Parse a `.gw` file and report what it holds, writing nothing.
///
/// The body is the **raw file content**, as for a GeneWeb import job: `.gw` is
/// ISO-8859-1 unless it opts into UTF-8, so decoding it upstream would mangle
/// the accented names the join key is built from.
///
/// Not scoped to a tree: this runs before the user has chosen one, and it is
/// what tells them whether they picked the right export.
pub async fn inspect_geneweb_handler(
    Query(query): Query<ImportGenewebQuery>,
    body: Bytes,
) -> Result<Json<InspectGenewebResponse>, ApiError> {
    let file_name = query
        .filename
        .unwrap_or_else(|| DEFAULT_ORIGIN_FILE.to_string());

    let inspection = geneanet::inspect_gw_off_thread(body, file_name).await?;

    Ok(Json(InspectGenewebResponse {
        person_count: inspection.person_count,
        family_count: inspection.family_count,
        skipped_blocks: inspection.skipped_blocks,
    }))
}

/// POST /api/v1/geneanet/archives
///
/// Index the central directory of each named data archive, extracting nothing.
///
/// An archive that cannot be read is reported in its own row rather than
/// failing the request: users add several at once and one corrupt ZIP is no
/// reason to discard the four that opened.
pub async fn index_archives_handler(
    State(state): State<AppState>,
    Json(body): Json<IndexArchivesRequest>,
) -> Result<Json<IndexArchivesResponse>, ApiError> {
    state.local_file_access.require()?;
    let (set, reports) = geneanet::index_archives_off_thread(body.paths).await?;

    Ok(Json(IndexArchivesResponse {
        file_count: <_ as oxidgene_geneanet::archive::LocalOriginals>::file_count(&set),
        archives: reports
            .into_iter()
            .map(|report| IndexedArchive {
                path: report.path,
                file_name: report.file_name,
                file_count: report.file_count,
                image_count: report.image_count,
                error: report.error,
            })
            .collect(),
    }))
}

/// POST /api/v1/geneanet/preview
///
/// Join the collected mapping onto the `.gw` and report what an import would
/// do. No network access and no writes — this is the moment the user finds out
/// whether the two halves belong to each other, *before* anything is written.
pub async fn preview_handler(
    State(state): State<AppState>,
    Json(body): Json<GeneanetPreviewRequest>,
) -> Result<Json<GeneanetPreviewResponse>, ApiError> {
    require_paths_access(&state, &body.archive_paths)?;
    let preview = geneanet::preview_off_thread(join_inputs(body)?).await?;

    Ok(Json(GeneanetPreviewResponse {
        person_count: preview.person_count,
        photo_count: preview.photo_count,
        persons_with_photo: preview.persons_with_photo,
        attachment_count: preview.attachment_count,
        in_archives: preview.in_archives,
        to_match: preview.to_match,
        to_download: preview.to_download,
        group_photos: preview.group_photos,
        unlinked_views: preview.unlinked_views,
        documents: preview.documents,
        document_pages: preview.document_pages,
        unlinked_names: preview.unlinked_names,
        outside_tree: preview.outside_tree,
        ambiguous: preview.ambiguous,
        unlinked_names_sample: preview.unlinked_names_sample,
        outside_tree_names: preview.outside_tree_names,
        ambiguous_names: preview.ambiguous_names,
        mismatch: preview.mismatch,
    }))
}

/// POST /api/v1/geneanet/session/encode
///
/// Turn what the login window collected into the file the wizard offers to
/// save. The login step is the only part of the import that talks to Geneanet, and the
/// expensive half of it is one `HEAD` per deposit — several hundred on a real
/// account — so its result is worth keeping rather than re-collecting on every
/// run.
pub async fn encode_session_handler(
    State(state): State<AppState>,
    Json(body): Json<EncodeSessionRequest>,
) -> Result<Response, ApiError> {
    state.local_file_access.require()?;
    // The wizard holds paths; the archive holds bytes. Each file is streamed
    // into the archive as it is read rather than carried through the UI.
    let archive = crate::service::session_media::encode(
        session::Session {
            collection: body.collection,
            deposit_sizes: body.deposit_sizes,
            account: body.account,
            media: body.media,
        },
        body.archive_paths,
    )
    .await?;

    // The bytes themselves: the wizard writes them straight to the file the
    // user chose, and wrapping an archive in JSON would only base64 it again.
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"geneanet-session.zip\"",
            ),
        ],
        archive,
    )
        .into_response())
}

/// POST /api/v1/geneanet/session/decode
///
/// Read a saved session back, refusing anything that is not one. The body is
/// the file itself.
pub async fn decode_session_handler(
    State(state): State<AppState>,
    body: Body,
) -> Result<Json<DecodeSessionResponse>, ApiError> {
    state.local_file_access.require()?;
    let permit = crate::service::intake::slot().await?;
    let upload = spool(&state, body).await?;
    let work_dir = state.work_dir.clone();
    let restored = crate::service::blocking::spawn(move || {
        let _permit = permit;
        crate::service::session_media::decode(upload, &work_dir)
    })
    .await
    .map_err(|_| OxidGeneError::Internal("session decoding failed".into()))??;

    // Reported rather than inferred from the sizes, which only cover the
    // single-page deposits.
    let photo_count = oxidgene_geneanet::manifest_from_collection(&restored.collection)
        .map(|manifest| manifest.view_count)
        .unwrap_or(0);

    Ok(Json(DecodeSessionResponse {
        collection: restored.collection,
        deposit_sizes: restored.deposit_sizes,
        account: restored.account,
        photo_count,
        media: restored.media,
    }))
}

/// POST /api/v1/geneanet/session/release
///
/// Delete the staged media of a decoded session the wizard closed or reset
/// without importing. Queuing the import releases them too, and those never
/// released expire a day after they were staged.
pub async fn release_session_media_handler(
    State(state): State<AppState>,
    Json(body): Json<ReleaseSessionMediaRequest>,
) -> Result<StatusCode, ApiError> {
    state.local_file_access.require()?;
    crate::service::session_media::remove_owned(body.paths.iter().map(String::as_str));
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/geneanet/plan
///
/// Say which media the server cannot produce on its own, so the login window
/// can fetch them. Takes the same inputs as the preview.
///
/// This exists because the server never reaches Geneanet: every direct request
/// is challenged whatever the cookie, so the bytes come from the window the
/// user signed in to and are handed back with the import.
pub async fn plan_handler(
    State(state): State<AppState>,
    Json(body): Json<GeneanetPreviewRequest>,
) -> Result<Json<GeneanetPlanResponse>, ApiError> {
    require_paths_access(&state, &body.archive_paths)?;
    let needed = geneanet::plan_off_thread(join_inputs(body)?).await?;

    Ok(Json(GeneanetPlanResponse {
        needed: needed
            .into_iter()
            .map(|item| NeededMedia {
                deposit_id: item.deposit_id,
                view_id: item.view_id,
                page: item.page,
                url: item.url,
                original: item.original,
            })
            .collect(),
    }))
}

/// POST /api/v1/trees/:tree_id/geneanet/import
///
/// Copy every local input to durable storage and queue the import.
pub async fn import_handler(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<GeneanetImportRequest>,
) -> Result<(StatusCode, Json<FileImportStartedResponse>), ApiError> {
    state.local_file_access.require()?;
    let gw = decode_gw(&body.gw_base64)?;
    let _intake = crate::service::intake::slot().await?;
    let job_id = crate::service::background_job::stage_geneanet_import(
        &viewer,
        &state.db,
        &*state.media,
        &state.work_dir,
        tree_id,
        &gw,
        body.file_name,
        body.collection,
        body.deposit_sizes,
        &body.archive_paths,
        &body.fetched,
        body.media_fidelity,
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(FileImportStartedResponse { job_id }),
    ))
}

/// POST /api/v1/geneanet/session/read
///
/// What a saved session holds, its media checked but kept nowhere: the web
/// wizard's login step, which sends the same file again with its import. The
/// body is the file itself.
pub async fn read_session_handler(
    State(state): State<AppState>,
    body: Body,
) -> Result<Json<ReadSessionResponse>, ApiError> {
    let permit = crate::service::intake::slot().await?;
    let upload = spool(&state, body).await?;
    let session = crate::service::blocking::spawn(move || {
        let _permit = permit;
        crate::service::session_media::read(upload)
    })
    .await
    .map_err(|_| OxidGeneError::Internal("session reading failed".into()))??;

    let photo_count = oxidgene_geneanet::manifest_from_collection(&session.collection)
        .map(|manifest| manifest.view_count)
        .unwrap_or(0);
    let mut media_urls: Vec<String> = session.media.into_keys().collect();
    media_urls.sort_unstable();
    Ok(Json(ReadSessionResponse {
        collection: session.collection,
        deposit_sizes: session.deposit_sizes,
        account: session.account,
        photo_count,
        media_urls,
    }))
}

/// POST /api/v1/trees/:tree_id/geneanet/session-import
///
/// Queue the import of a `.gw` with the media of a saved session, both sent
/// as `multipart/form-data`: a `gw` part, the GeneWeb file, whose file name
/// is the import's origin; a `session` part, the saved session; and an
/// optional `media_fidelity` part, `renditions` (the default) or
/// `originals`. The session's media are copied into job storage before the
/// answer, so nothing waits for the browser afterwards.
pub async fn session_import_handler(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<FileImportStartedResponse>), ApiError> {
    // Before the body is read: a waiting upload holds a connection, not a
    // spool file. The slot is held until the media are in job storage.
    let _intake = crate::service::intake::slot().await?;
    let import = read_session_import_form(&state, multipart).await?;
    let job_id = crate::service::background_job::stage_geneanet_session_import(
        &viewer,
        &state.db,
        &*state.media,
        &state.work_dir,
        tree_id,
        import,
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(FileImportStartedResponse { job_id }),
    ))
}

/// The largest `.gw` a session import takes: the JSON bodies' limit, which
/// carries it base64-encoded beside the rest.
const GW_PART_LIMIT: usize = 32 * 1024 * 1024;

/// The parts of a session import, its session spooled to a private file.
async fn read_session_import_form(
    state: &AppState,
    mut multipart: Multipart,
) -> Result<crate::service::background_job::GeneanetSessionImport, ApiError> {
    let malformed = |error: axum::extract::multipart::MultipartError| {
        ApiError::from(OxidGeneError::Validation(format!(
            "malformed upload: {error}"
        )))
    };
    let mut gw = None;
    let mut session = None;
    let mut media_fidelity = geneanet::MediaFidelity::default();
    while let Some(mut field) = multipart.next_field().await.map_err(malformed)? {
        match field.name().unwrap_or_default() {
            "gw" => {
                let file_name = field
                    .file_name()
                    .map_or_else(|| DEFAULT_ORIGIN_FILE.to_string(), str::to_string);
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(malformed)? {
                    if bytes.len() + chunk.len() > GW_PART_LIMIT {
                        return Err(OxidGeneError::Validation(format!(
                            "the .gw exceeds the {GW_PART_LIMIT}-byte limit"
                        ))
                        .into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                gw = Some((file_name, bytes));
            }
            "session" => {
                let mut file = tokio::fs::File::from_std(
                    state.work_dir.anonymous_file().map_err(OxidGeneError::Io)?,
                );
                while let Some(chunk) = field.chunk().await.map_err(malformed)? {
                    file.write_all(&chunk).await.map_err(OxidGeneError::Io)?;
                }
                file.rewind().await.map_err(OxidGeneError::Io)?;
                session = Some(file.into_std().await);
            }
            "media_fidelity" => {
                let text = field.text().await.map_err(malformed)?;
                media_fidelity =
                    serde_json::from_value(serde_json::Value::String(text)).map_err(|_| {
                        OxidGeneError::Validation(
                            "`media_fidelity` is neither `renditions` nor `originals`".into(),
                        )
                    })?;
            }
            other => {
                return Err(OxidGeneError::Validation(format!(
                    "unexpected `{other}` part in a session import"
                ))
                .into());
            }
        }
    }
    let (Some((file_name, gw)), Some(session)) = (gw, session) else {
        return Err(OxidGeneError::Validation(
            "a session import needs a `gw` part and a `session` part".into(),
        )
        .into());
    };
    Ok(crate::service::background_job::GeneanetSessionImport {
        gw,
        file_name,
        session,
        media_fidelity,
    })
}

/// `body`, streamed to a private file of the working directory, rewound.
async fn spool(state: &AppState, body: Body) -> Result<std::fs::File, ApiError> {
    use futures_util::StreamExt as _;

    let mut upload =
        tokio::fs::File::from_std(state.work_dir.anonymous_file().map_err(OxidGeneError::Io)?);
    let mut chunks = body.into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk
            .map_err(|_| OxidGeneError::Validation("session upload was interrupted".into()))?;
        upload.write_all(&chunk).await.map_err(OxidGeneError::Io)?;
    }
    upload.rewind().await.map_err(OxidGeneError::Io)?;
    Ok(upload.into_std().await)
}

/// Archive paths name files of the server's own disk: only the desktop's
/// in-process server reads them.
fn require_paths_access(state: &AppState, archive_paths: &[String]) -> Result<(), ApiError> {
    if !archive_paths.is_empty() {
        state.local_file_access.require()?;
    }
    Ok(())
}

/// Decodes the base64 the JSON bodies carry the `.gw` in.
///
/// JSON cannot hold the raw bytes and the raw bytes are what the reader needs,
/// so the two calls that bundle a `.gw` with other fields encode it. The two
/// that send nothing else take it as a raw body instead.
/// What a preview or a plan request joins, its `.gw` decoded.
fn join_inputs(body: GeneanetPreviewRequest) -> Result<geneanet::JoinInputs, ApiError> {
    Ok(geneanet::JoinInputs {
        gw: decode_gw(&body.gw_base64)?,
        file_name: body.file_name,
        collection: body.collection,
        deposit_sizes: body.deposit_sizes,
        archive_paths: body.archive_paths,
        fidelity: body.media_fidelity,
    })
}

fn decode_gw(encoded: &str) -> Result<Vec<u8>, ApiError> {
    use base64::Engine as _;

    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| {
            ApiError::from(OxidGeneError::Validation(format!(
                "the .gw payload is not valid base64: {e}"
            )))
        })
}
