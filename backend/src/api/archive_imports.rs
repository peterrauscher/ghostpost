use crate::api::problem::Problem;
use crate::api::router::AppState;
use crate::auth::extract::{enforce_web_mutation_guards, AuthSession};
use crate::blob::{BlobError, BlobStore, PresignedPost};
use crate::r#import::limits::MAX_ARCHIVE_BYTES;
use axum::extract::{Extension, FromRequestParts, Path, Query, State};
use axum::http::{request::Parts, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use uuid::Uuid;

const UPLOAD_TTL: Duration = Duration::from_secs(900);
const ALLOWED_TYPES: [&str; 2] = ["application/zip", "application/octet-stream"];

type ApiResult<T> = Result<T, Problem>;

fn problem(status: StatusCode, detail: &str, code: Option<&str>) -> Problem {
    Problem { type_uri: "https://ghostpost.app/problems/archive-import".into(), title: status.canonical_reason().unwrap_or("Request failed").into(), status: status.as_u16(), detail: detail.into(), instance: None, code: code.map(str::to_owned) }
}
fn db_problem(_: sqlx::Error) -> Problem { problem(StatusCode::INTERNAL_SERVER_ERROR, "Archive import operation failed", None) }
fn blob_problem(err: BlobError) -> Problem { match err { BlobError::NotFound => problem(StatusCode::UNPROCESSABLE_ENTITY,"Uploaded archive was not found",Some("UPLOAD_NOT_FOUND")), _ => problem(StatusCode::SERVICE_UNAVAILABLE,"Archive storage is unavailable",None) } }

pub struct Mutation { pub session: crate::auth::types::AppSession, pub key: String }
impl FromRequestParts<AppState> for Mutation {
    type Rejection = crate::auth::problem::AuthProblem;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let AuthSession(session) = AuthSession::from_request_parts(parts, state).await?;
        enforce_web_mutation_guards(parts, state, &session).await?;
        let key = parts.headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty() && v.len() <= 200).ok_or_else(|| crate::auth::problem::AuthProblem::bad_request("IDEMPOTENCY_KEY_REQUIRED", "missing or invalid Idempotency-Key"))?.to_owned();
        Ok(Self { session, key })
    }
}

#[derive(Deserialize)] #[serde(rename_all="camelCase")] pub struct ReserveRequest { platform:String, content_length:u64, content_type:String }
#[derive(Serialize)] #[serde(rename_all="camelCase")] pub struct UploadDto { method:&'static str, url:String, fields:BTreeMap<String,String> }
#[derive(Serialize)] #[serde(rename_all="camelCase")] pub struct ReserveResponse { id:Uuid, upload:UploadDto, expires_at:DateTime<Utc> }
#[derive(Serialize, sqlx::FromRow)] #[serde(rename_all="camelCase")] pub struct ArchiveDto { id:Uuid, platform:String, status:String, item_count:Option<i32>, error_code:Option<String>, parser_version:String, format_family:Option<String>, format_confidence:Option<String>, created_at:DateTime<Utc>, finished_at:Option<DateTime<Utc>>, duplicate_of_import_id:Option<Uuid> }
#[derive(Deserialize)] pub struct ListQuery { platform:Option<String>, status:Option<String>, cursor:Option<Uuid>, limit:Option<i64> }
#[derive(Serialize)] #[serde(rename_all="camelCase")] pub struct ListResponse { items:Vec<ArchiveDto>, next_cursor:Option<Uuid> }
#[derive(Serialize)] pub struct DeleteResponse { id:Uuid, status:&'static str }

pub async fn reserve(State(state):State<AppState>, Extension(blob):Extension<Arc<dyn BlobStore>>, Mutation{session,key:idem}:Mutation, Json(req):Json<ReserveRequest>) -> ApiResult<impl IntoResponse> {
    if req.platform != "reddit" && req.platform != "x" { return Err(problem(StatusCode::UNPROCESSABLE_ENTITY,"This platform is not available for archive import",Some("PLATFORM_COMING_SOON"))); }
    if req.content_length == 0 || req.content_length > MAX_ARCHIVE_BYTES || !ALLOWED_TYPES.contains(&req.content_type.as_str()) { return Err(problem(StatusCode::UNPROCESSABLE_ENTITY,"Invalid archive size or content type",Some("INVALID_ARCHIVE_RESERVATION"))); }
    if let Some(payload)=sqlx::query_scalar::<_,serde_json::Value>("SELECT upload_reservation FROM archive_imports WHERE tenant_id=$1 AND user_id=$2 AND reserve_idempotency_key=$3").bind(session.tenant_id).bind(session.user_id).bind(&idem).fetch_optional(&state.pool).await.map_err(db_problem)? {
        return Ok((StatusCode::CREATED,Json(payload)));
    }
    let id=Uuid::new_v4(); let storage_key=format!("archive-imports/{id}/source.zip"); let post=blob.presign_post_archive(&storage_key,req.content_length,&req.content_type,UPLOAD_TTL).await.map_err(blob_problem)?;
    let payload=serde_json::to_value(reserve_body(id,post)).map_err(|_|problem(StatusCode::INTERNAL_SERVER_ERROR,"Archive import operation failed",None))?;
    sqlx::query("INSERT INTO archive_imports(tenant_id,id,user_id,platform,raw_storage_key,byte_size,content_type,upload_expires_at,reserve_idempotency_key,upload_reservation) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(session.tenant_id).bind(id).bind(session.user_id).bind(&req.platform).bind(&storage_key).bind(req.content_length as i64).bind(&req.content_type).bind(payload["expiresAt"].as_str().and_then(|v|v.parse::<DateTime<Utc>>().ok()).ok_or_else(||problem(StatusCode::INTERNAL_SERVER_ERROR,"Archive import operation failed",None))?).bind(&idem).bind(&payload).execute(&state.pool).await.map_err(db_problem)?;
    Ok((StatusCode::CREATED,Json(payload)))
}
fn reserve_body(id:Uuid,p:PresignedPost)->ReserveResponse { ReserveResponse{id,upload:UploadDto{method:"POST",url:p.url,fields:p.fields},expires_at:p.expires_at} }

const DTO_SELECT:&str="id,platform,status,item_count,error_code,parser_version,format_family,format_confidence,created_at,finished_at,duplicate_of_import_id";
pub async fn get_one(State(state):State<AppState>, AuthSession(s):AuthSession, Path(id):Path<Uuid>)->ApiResult<Json<ArchiveDto>> { let q=format!("SELECT {DTO_SELECT} FROM archive_imports WHERE tenant_id=$1 AND user_id=$2 AND id=$3"); sqlx::query_as(&q).bind(s.tenant_id).bind(s.user_id).bind(id).fetch_optional(&state.pool).await.map_err(db_problem)?.map(Json).ok_or_else(||problem(StatusCode::NOT_FOUND,"Not found",None)) }
pub async fn list(State(state):State<AppState>, AuthSession(s):AuthSession, Query(q):Query<ListQuery>)->ApiResult<Json<ListResponse>> { let limit=q.limit.unwrap_or(50).clamp(1,100); let sql=format!("SELECT {DTO_SELECT} FROM archive_imports WHERE tenant_id=$1 AND user_id=$2 AND raw_storage_key IS NOT NULL AND ($3::text IS NULL OR platform=$3) AND ($4::text IS NULL OR status=$4) AND ($5::uuid IS NULL OR id>$5) ORDER BY created_at ASC,id ASC LIMIT $6"); let mut items:Vec<ArchiveDto>=sqlx::query_as(&sql).bind(s.tenant_id).bind(s.user_id).bind(q.platform).bind(q.status).bind(q.cursor).bind(limit+1).fetch_all(&state.pool).await.map_err(db_problem)?; let next_cursor=if items.len() as i64>limit {items.pop();items.last().map(|x|x.id)}else{None}; Ok(Json(ListResponse{items,next_cursor})) }

pub async fn complete(State(state):State<AppState>, Extension(blob):Extension<Arc<dyn BlobStore>>, Path(id):Path<Uuid>, Mutation{session,..}:Mutation, headers:HeaderMap)->ApiResult<Response> { if headers.get("content-length").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()).unwrap_or(0)>0{return Err(problem(StatusCode::BAD_REQUEST,"Completion body must be empty",None));} let row=sqlx::query("SELECT raw_storage_key,raw_storage_version_id,byte_size,content_type,status,upload_expires_at FROM archive_imports WHERE tenant_id=$1 AND user_id=$2 AND id=$3").bind(session.tenant_id).bind(session.user_id).bind(id).fetch_optional(&state.pool).await.map_err(db_problem)?.ok_or_else(||problem(StatusCode::NOT_FOUND,"Not found",None))?; let status:String=row.get("status"); if status=="awaiting_upload" { let key:String=row.get("raw_storage_key"); let expected:i64=row.get("byte_size"); let ct:String=row.get("content_type"); let expires:DateTime<Utc>=row.get("upload_expires_at"); if Utc::now()>expires{return Err(problem(StatusCode::UNPROCESSABLE_ENTITY,"Upload reservation expired",Some("UPLOAD_EXPIRED")));} let head=blob.head_object(&key).await.map_err(blob_problem)?; if head.content_length!=expected as u64 || head.content_type.as_deref()!=Some(ct.as_str()) || head.version_id.is_empty(){return Err(problem(StatusCode::UNPROCESSABLE_ENTITY,"Uploaded archive does not match the reservation",Some("INTEGRITY_MISMATCH")));} let mut tx=state.pool.begin().await.map_err(db_problem)?; sqlx::query("UPDATE archive_imports SET status='queued',raw_storage_version_id=$4,raw_delete_after=now()+interval '24 hours',updated_at=now() WHERE tenant_id=$1 AND user_id=$2 AND id=$3 AND status='awaiting_upload'").bind(session.tenant_id).bind(session.user_id).bind(id).bind(&head.version_id).execute(&mut *tx).await.map_err(db_problem)?; sqlx::query("INSERT INTO work_items(tenant_id,kind,subject_user_id,payload,dedupe_key,status,run_after) VALUES($1,'import.normalize',$2,jsonb_build_object('import_id',$3::text,'key',$4::text,'version_id',$5::text),$3::text,'pending',now()) ON CONFLICT (tenant_id,kind,dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING").bind(session.tenant_id).bind(session.user_id).bind(id).bind(&key).bind(&head.version_id).execute(&mut *tx).await.map_err(db_problem)?; tx.commit().await.map_err(db_problem)?; } let Json(dto)=get_one(State(state),AuthSession(session),Path(id)).await?; Ok((StatusCode::ACCEPTED,Json(dto)).into_response()) }

pub async fn delete_one(State(state):State<AppState>, Path(id):Path<Uuid>, Mutation{session,..}:Mutation)->ApiResult<impl IntoResponse> { let mut tx=state.pool.begin().await.map_err(db_problem)?; let changed=sqlx::query("UPDATE archive_imports SET status='deleting',cancel_requested_at=COALESCE(cancel_requested_at,now()),updated_at=now() WHERE tenant_id=$1 AND user_id=$2 AND id=$3 RETURNING id").bind(session.tenant_id).bind(session.user_id).bind(id).fetch_optional(&mut *tx).await.map_err(db_problem)?; if changed.is_none(){return Err(problem(StatusCode::NOT_FOUND,"Not found",None));} sqlx::query("INSERT INTO work_items(tenant_id,kind,subject_user_id,payload,dedupe_key) VALUES($1,'import.purge_raw',$2,jsonb_build_object('import_id',$3::text),$3::text) ON CONFLICT (tenant_id,kind,dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING").bind(session.tenant_id).bind(session.user_id).bind(id).execute(&mut *tx).await.map_err(db_problem)?; tx.commit().await.map_err(db_problem)?; Ok((StatusCode::ACCEPTED,Json(DeleteResponse{id,status:"deleting"}))) }
