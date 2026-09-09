//! AGENT-07：先核对媒体容器签名与声明 MIME；不以扩展名推测内容。
use crate::{Result, error};
pub fn verify(bytes: &[u8], mime: &str) -> Result<()> {
    let detected = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"II\x2a\0") || bytes.starts_with(b"MM\0\x2a") {
        "image/tiff"
    } else {
        return Err(error("AGENT_UNREPRESENTABLE", "媒体容器签名不在已支持的验证集合中"));
    };
    if mime != detected {
        return Err(error("AGENT_ATTACHMENT_MISMATCH", "媒体 MIME 与容器签名不匹配"));
    }
    Ok(())
}
