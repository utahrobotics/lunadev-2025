use tokio::process::Command;
use unfmt::unformat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInputInfo {
    pub name: String,
    pub media_type: String,
}

pub async fn list_media_input() -> std::io::Result<Vec<MediaInputInfo>> {
    let output = Command::new("ffmpeg")
        .arg("-list_devices")
        .arg("true")
        .arg("-f")
        .arg("dshow")
        .arg("-i")
        .arg("dummy")
        .output()
        .await?;

    let mut inputs = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines() {
        let Some((_, name, media_type)) = unformat!(r#"[dshow @ {}] "{}" ({})"#, line) else {
            continue;
        };
        inputs.push(MediaInputInfo {
            name: name.to_string(),
            media_type: media_type.to_string(),
        });
    }
    Ok(inputs)
}
