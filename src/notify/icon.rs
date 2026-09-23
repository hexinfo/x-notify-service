use std::sync::Arc;

pub fn popup_icon() -> Option<Arc<image::RgbaImage>> {
    let bytes = include_bytes!("../../assets/icons/hicolor/x-notify-service-128.png");
    match image::load_from_memory(bytes) {
        Ok(icon) => Some(Arc::new(icon.to_rgba8())),
        Err(error) => {
            log::warn!("通知窗口图标解码失败: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::popup_icon;

    #[test]
    fn embedded_linux_icon_decodes_as_128_pixel_rgba() {
        let icon = popup_icon().expect("embedded PNG should decode");
        assert_eq!(icon.dimensions(), (128, 128));
    }
}
