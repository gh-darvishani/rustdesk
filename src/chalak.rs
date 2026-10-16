// ChalakDesk: phone-number IDs and online reporting to the support panel.
use hbb_common::{config::Config, log, tokio};

pub const SITE: &str = "https://chalak-desk.ir";
const HEARTBEAT_URL: &str = "https://chalak-desk.ir/api/devices/heartbeat";
const HEARTBEAT_SECS: u64 = 60;

/// Iranian mobile number, e.g. 09121234567.
pub fn is_phone_id(id: &str) -> bool {
    id.len() == 11 && id.starts_with("09") && id.chars().all(|c| c.is_ascii_digit())
}

/// Periodically tells chalak-desk.ir that this device is online, so supporters can see
/// who is waiting for help. Only reports once the ID has been set to a phone number.
pub async fn heartbeat_loop() {
    loop {
        let id = Config::get_id();
        if is_phone_id(&id) {
            let body = serde_json::json!({
                "id": id,
                "phone": id,
                "hostname": crate::common::hostname(),
                "os": std::env::consts::OS,
                "version": crate::VERSION,
            })
            .to_string();
            if let Err(e) = crate::post_request(
                HEARTBEAT_URL.to_owned(),
                body,
                "Content-Type: application/json",
            )
            .await
            {
                log::debug!("chalak heartbeat failed: {e}");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_SECS)).await;
    }
}
