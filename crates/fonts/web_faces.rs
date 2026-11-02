//! Web font selection shared by the font build script and the mobile notice packager.

pub fn includes(family: &str, style: &str, scripts: &str) -> bool {
    (family == "BIZ UDPGothic" && style == "Regular") || scripts.split(',').any(|s| matches!(s.trim(), "Arab" | "Telu" | "Hebr" | "Thai" | "Deva"))
}
