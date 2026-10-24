// ChalakDesk: phone-number IDs and online reporting to the support panel.
use hbb_common::{config::Config, log, sodiumoxide, tokio};

// ChalakDesk site.
pub const SITE: &str = "https://chalak-desk.ir";
const HEARTBEAT_URL: &str = "https://chalak-desk.ir/api/devices/heartbeat";
const RESULT_URL: &str = "https://chalak-desk.ir/api/install/result";
const HEARTBEAT_SECS: u64 = 60;

// Public key for verifying remote-install jobs. The matching secret key lives only on the
// server, so a forged server or man-in-the-middle cannot order an install. Base64 of 32 bytes.
const INSTALL_PUBKEY_B64: &str = "vy+jxfLdBKjAUH4Xm78RTNBGojL3Nbd7LKtPifc1J1s=";

/// Iranian mobile number (09121234567), optionally followed by one digit 2-9 for a
/// customer's additional computers (091212345672).
pub fn is_phone_id(id: &str) -> bool {
    let ok_len = id.len() == 11 || (id.len() == 12 && !id.ends_with(['0', '1']));
    ok_len && id.starts_with("09") && id.chars().all(|c| c.is_ascii_digit())
}

/// Periodically tells chalak-desk.ir that this device is online, so supporters can see
/// who is waiting for help. Only reports once the ID has been set to a phone number.
pub async fn heartbeat_loop() {
    loop {
        let id = Config::get_id();
        if is_phone_id(&id) {
            let body = serde_json::json!({
                "id": id,
                "phone": &id[..11],
                "hostname": crate::common::hostname(),
                "os": std::env::consts::OS,
                "version": crate::VERSION,
            })
            .to_string();
            match crate::post_request(
                HEARTBEAT_URL.to_owned(),
                body,
                "Content-Type: application/json",
            )
            .await
            {
                Ok(resp) => {
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    handle_heartbeat_response(&resp, &id).await;
                }
                Err(e) => log::debug!("chalak heartbeat failed: {e}"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_SECS)).await;
    }
}

/// A remote-install job the server attached to a heartbeat reply. Acts only when the signature
/// checks out, the job is meant for this device, and it has not expired. The service runs as the
/// local system account, so the silent MSI install needs no extra elevation.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn handle_heartbeat_response(resp: &str, my_id: &str) {
    // Only the installed service (running with system privileges) may install software.
    if !crate::platform::is_installed() {
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(resp) {
        Ok(v) => v,
        Err(_) => return,
    };
    let install = match v.get("install") {
        Some(i) => i,
        None => return,
    };
    let job_str = install.get("job").and_then(|x| x.as_str()).unwrap_or_default();
    let sig_b64 = install.get("sig").and_then(|x| x.as_str()).unwrap_or_default();
    if !verify_install_sig(job_str, sig_b64) {
        log::warn!("chalak install: bad signature, ignored");
        return;
    }
    let job: serde_json::Value = match serde_json::from_str(job_str) {
        Ok(j) => j,
        Err(_) => return,
    };
    let job_id = job.get("job_id").and_then(|x| x.as_i64()).unwrap_or(0);
    let for_id = job.get("id").and_then(|x| x.as_str()).unwrap_or_default();
    let exp = job.get("exp").and_then(|x| x.as_i64()).unwrap_or(0);
    let url = job.get("url").and_then(|x| x.as_str()).unwrap_or_default().to_owned();
    let sha256 = job.get("sha256").and_then(|x| x.as_str()).unwrap_or_default().to_lowercase();
    let name = job.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_owned();
    // The job must be addressed to this device and still valid, and only http(s) on our site.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if for_id != my_id || now > exp || !url.starts_with("https://chalak-desk.ir/") {
        return;
    }
    // Run one install at a time; ignore if another is already in progress.
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLING: AtomicBool = AtomicBool::new(false);
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return;
    }
    let my_id = my_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let (ok, result) = run_install_job(&url, &sha256);
        report_install_result(job_id, &my_id, ok, &result);
        INSTALLING.store(false, Ordering::SeqCst);
        log::info!("chalak install '{name}' job {job_id}: ok={ok} {result}");
    });
}

/// Verifies the ed25519 signature of a job string against the baked-in public key.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn verify_install_sig(job: &str, sig_b64: &str) -> bool {
    use sodiumoxide::base64::Variant::Original;
    let _ = sodiumoxide::init();
    let pk_bytes = match sodiumoxide::base64::decode(INSTALL_PUBKEY_B64, Original) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let sig_bytes = match sodiumoxide::base64::decode(sig_b64, Original) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk = match sodiumoxide::crypto::sign::PublicKey::from_slice(&pk_bytes) {
        Some(pk) => pk,
        None => return false,
    };
    let sig = match sodiumoxide::crypto::sign::Signature::from_bytes(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    sodiumoxide::crypto::sign::verify_detached(&sig, job.as_bytes(), &pk)
}

/// Downloads the package zip, checks its SHA-256, extracts it, and silently installs the MSI
/// inside. Returns (success, short message) for the support panel. Windows only.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_install_job(url: &str, sha256: &str) -> (bool, String) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (url, sha256);
        return (false, "unsupported platform".to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        // Download.
        let bytes = match download_bytes(url) {
            Ok(b) => b,
            Err(e) => return (false, format!("download failed: {e}")),
        };
        // Verify hash before touching disk.
        let got = hex::encode(Sha256::digest(&bytes));
        if !sha256.is_empty() && got != sha256 {
            return (false, "checksum mismatch".to_owned());
        }
        // Work inside a fresh temp folder.
        let dir = std::env::temp_dir().join(format!("chalak_pkg_{}", got.get(..16).unwrap_or("pkg")));
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return (false, format!("temp dir failed: {e}"));
        }
        let zip_path = dir.join("pkg.zip");
        match std::fs::File::create(&zip_path).and_then(|mut f| f.write_all(&bytes)) {
            Ok(_) => {}
            Err(e) => return (false, format!("write failed: {e}")),
        }
        // Extract.
        let msi = match extract_and_find_msi(&zip_path, &dir) {
            Ok(Some(p)) => p,
            Ok(None) => return (false, "no msi in package".to_owned()),
            Err(e) => return (false, format!("extract failed: {e}")),
        };
        // Silent install.
        let status = std::process::Command::new("msiexec")
            .args(["/i", &msi.to_string_lossy(), "/qn", "/norestart"])
            .status();
        let _ = std::fs::remove_dir_all(&dir);
        match status {
            Ok(s) if s.success() => (true, "installed".to_owned()),
            // 3010 = success, reboot required.
            Ok(s) if s.code() == Some(3010) => (true, "installed (reboot pending)".to_owned()),
            Ok(s) => (false, format!("msiexec exit {}", s.code().unwrap_or(-1))),
            Err(e) => (false, format!("msiexec failed: {e}")),
        }
    }
}

#[cfg(all(target_os = "windows", not(any(target_os = "android", target_os = "ios"))))]
fn download_bytes(url: &str) -> hbb_common::ResultType<Vec<u8>> {
    // Blocking context (spawn_blocking): drive the async download on a tiny runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let client = crate::hbbs_http::create_http_client_async_with_url(url).await;
        let resp = client.get(url).send().await?;
        if !resp.status().is_success() {
            hbb_common::bail!("http {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    })
}

#[cfg(all(target_os = "windows", not(any(target_os = "android", target_os = "ios"))))]
fn extract_and_find_msi(
    zip_path: &std::path::Path,
    out: &std::path::Path,
) -> hbb_common::ResultType<Option<std::path::PathBuf>> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut msi: Option<std::path::PathBuf> = None;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // Guard against zip-slip: keep only the file name, drop any path.
        let name = match entry.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_owned())) {
            Some(n) => n,
            None => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let dest = out.join(&name);
        let mut f = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut f)?;
        if dest.extension().map_or(false, |e| e.eq_ignore_ascii_case("msi")) {
            msi = Some(dest);
        }
    }
    Ok(msi)
}

/// Tells the panel how the install went, so the supporter sees success or the error.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn report_install_result(job_id: i64, id: &str, ok: bool, result: &str) {
    let body = serde_json::json!({ "job_id": job_id, "id": id, "ok": ok, "result": result })
        .to_string();
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let _ = rt.block_on(crate::post_request(
        RESULT_URL.to_owned(),
        body,
        "Content-Type: application/json",
    ));
}

/// Sets a phone-number ID. The open-source hbbs ignores "change ID" requests, so instead the
/// ID is written to the service config and the service registers it like a fresh ID. If the
/// number already belongs to another computer the server answers UUID_MISMATCH and the service
/// falls back to a random ID, which is reported here as "Not available".
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub async fn set_phone_id(id: String) -> &'static str {
    if crate::ipc::set_config_async("id", id.clone()).await.is_err() {
        return "Failed to connect to rendezvous server";
    }
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let cur = crate::ipc::get_config_async("id", 1_000).await.ok().flatten().unwrap_or_default();
        if !cur.is_empty() && cur != id {
            return "Not available";
        }
        let confirmed = crate::ipc::get_config_async("fingerprint", 1_000)
            .await
            .ok()
            .flatten()
            .map_or(false, |f| !f.is_empty());
        if cur == id && confirmed {
            return "";
        }
    }
    // Not confirmed yet (e.g. server unreachable). Keep the ID; the service keeps retrying, and
    // if the number turns out to be taken the app asks again once the ID falls back.
    ""
}
