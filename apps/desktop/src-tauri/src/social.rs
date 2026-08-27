//! The optional social boundary. Local work never depends on this service.
use serde::{Deserialize, Serialize};
use tauri::State;
use crate::{db::Database, AppState};

const ORIGIN: &str = "https://social-api-production-edb6.up.railway.app";
const TOKEN: &str = "social.token";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Presence { pub state: String, pub last_seen_at: String, pub active_sessions: i64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Metrics { pub visibility: String, pub tokens_today: i64, pub tokens_week: i64, pub focus_minutes_week: i64, pub streak_days: i64, pub calendar: Vec<serde_json::Value>, pub observed_at: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Profile { pub id: String, pub handle: String, pub display_name: String, pub presence: Option<Presence>, pub metrics: Option<Metrics> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SocialReport { pub enabled: bool, pub profile: Option<Profile>, pub friends: Vec<Profile>, pub requests: Vec<Profile>, pub identity_name: Option<String> }

fn request<T: for<'a> Deserialize<'a>>(db: &Database, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<T,String> {
 let token: String=crate::settings::get(db,TOKEN).ok_or_else(||"social.notConnected".to_string())?;
 let url=format!("{ORIGIN}{path}"); let mut req=ureq::request(method,&url).set("authorization",&format!("Bearer {token}")).set("content-type","application/json");
 let response=match body { Some(value)=>req.send_string(&value.to_string()),None=>req.call() }.map_err(|e|e.to_string())?;
 serde_json::from_reader(response.into_reader()).map_err(|e|e.to_string())
}
pub fn report(db:&Database)->SocialReport { let identity=crate::identity::current(db); if crate::settings::get::<String>(db,TOKEN).is_none(){return SocialReport{enabled:false,profile:None,friends:vec![],requests:vec![],identity_name:identity.map(|a|a.display_name)}}; let me:Result<serde_json::Value,_>=request(db,"GET","/v1/me",None); let friends:Result<serde_json::Value,_>=request(db,"GET","/v1/friends",None); SocialReport{enabled:true,profile:me.ok().and_then(|v|serde_json::from_value(v["profile"].clone()).ok()),friends:friends.as_ref().ok().and_then(|v|serde_json::from_value(v["friends"].clone()).ok()).unwrap_or_default(),requests:friends.ok().and_then(|v|serde_json::from_value(v["requests"].clone()).ok()).unwrap_or_default(),identity_name:identity.map(|a|a.display_name)} }
pub fn heartbeat(db:&Database)->Result<(),String>{let active:i64=db.with(|c|c.query_row("SELECT COUNT(*) FROM sessions WHERE state IN ('running','working')",[],|r|r.get(0))).map_err(|e|e.to_string())?;let _:serde_json::Value=request(db,"POST","/v1/heartbeat",Some(serde_json::json!({"activeSessions":active})))?;Ok(())}
#[tauri::command] pub fn social_report(state:State<'_,AppState>)->SocialReport{report(&state.db)}
#[tauri::command] pub fn social_heartbeat(state:State<'_,AppState>)->Result<(),String>{heartbeat(&state.db)}
#[tauri::command] pub fn social_create_profile(state:State<'_,AppState>,handle:String)->Result<SocialReport,String>{let identity=crate::identity::current(&state.db).ok_or_else(||"identity.notSignedIn".to_string())?; let body=serde_json::json!({"handle":handle,"displayName":identity.display_name});let v:serde_json::Value=ureq::post(&format!("{ORIGIN}/v1/accounts")).set("content-type","application/json").send_string(&body.to_string()).map_err(|e|e.to_string()).and_then(|r|serde_json::from_reader(r.into_reader()).map_err(|e|e.to_string()))?;let token=v["token"].as_str().ok_or_else(||"social.invalidResponse".to_string())?;crate::settings::set(&state.db,TOKEN,&token.to_string())?;Ok(report(&state.db))}
#[tauri::command] pub fn social_request_friend(state:State<'_,AppState>,handle:String)->Result<SocialReport,String>{let _:serde_json::Value=request(&state.db,"POST",&format!("/v1/friends/{handle}"),Some(serde_json::json!({})))?;Ok(report(&state.db))}
#[tauri::command] pub fn social_accept_friend(state:State<'_,AppState>,requester_id:String)->Result<SocialReport,String>{let _:serde_json::Value=request(&state.db,"POST",&format!("/v1/friend-requests/{requester_id}/accept"),Some(serde_json::json!({})))?;Ok(report(&state.db))}
