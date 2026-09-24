//! Linux X11 窗口集成:补齐通知窗口状态和多档图标。
//! 图标负载由构建脚本生成,运行时直接写入 X11 属性。

include!(concat!(env!("OUT_DIR"), "/window_icons.rs"));

/// format=32 时 data_length 是 32 位元素个数(非字节数);
/// 传字节长会触发 x11rb 客户端断言 panic(UOS 闪退根因,回归测试钉死)
pub(super) fn property_units(blob: &[u8]) -> u32 {
    // 字节长转 u32 元素数:右移两位即 /4
    u32::try_from(blob.len() >> 2).unwrap_or(u32::MAX)
}

/// 从 ICON_BLOB 首档(128×128)生成 iced 窗口图标(RGBA):winit 创建期
/// 写 _NET_WM_ICON;x11rb 多档直写仍是兜底主链(UOS 任务栏走 desktop 链)
pub(super) fn settings_icon() -> Option<iced::window::Icon> {
    let (w_bytes, rest) = ICON_BLOB.split_at(4);
    let (h_bytes, all_pixels) = rest.split_at(4);
    let Ok(w) = <[u8; 4]>::try_from(w_bytes) else {
        return None;
    };
    let Ok(h) = <[u8; 4]>::try_from(h_bytes) else {
        return None;
    };
    let width = u32::from_le_bytes(w);
    let height = u32::from_le_bytes(h);
    let count = (width as usize).checked_mul(height as usize)?;
    let pixels = all_pixels.get(..count.checked_mul(4)?)?;
    // 小端 u32 ARGB 的字节序是 [B,G,R,A],转 RGBA 需交换首尾字节
    let mut rgba = Vec::with_capacity(pixels.len());
    let (chunks, _) = pixels.as_chunks::<4>();
    for px in chunks {
        let [blue, green, red, alpha] = *px;
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    iced::window::icon::from_rgba(rgba, width, height).ok()
}

fn merge_atoms(atoms: &mut Vec<u32>, required: &[u32]) {
    for atom in required {
        if !atoms.contains(atom) {
            atoms.push(*atom);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{ICON_BLOB, property_units};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;

    pub(super) fn set() {
        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            log::debug!("无 X 连接,跳过窗口属性设置");
            return;
        };
        let root = conn.setup().roots[screen_num].root;
        if let Some(wid) = find_window(&conn, root) {
            write_blob(&conn, wid, ICON_BLOB);
            configure_notification_window(&conn, root, wid);
            let _ = conn.flush();
        }
    }

    fn atom(conn: &x11rb::rust_connection::RustConnection, name: &[u8]) -> Option<u32> {
        conn.intern_atom(false, name)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|a| a.atom)
    }

    struct NotificationAtoms {
        state: u32,
        skip_taskbar: u32,
        skip_pager: u32,
        above: u32,
        window_type: u32,
        notification: u32,
    }

    impl NotificationAtoms {
        fn load(conn: &x11rb::rust_connection::RustConnection) -> Option<Self> {
            Some(Self {
                state: atom(conn, b"_NET_WM_STATE")?,
                skip_taskbar: atom(conn, b"_NET_WM_STATE_SKIP_TASKBAR")?,
                skip_pager: atom(conn, b"_NET_WM_STATE_SKIP_PAGER")?,
                above: atom(conn, b"_NET_WM_STATE_ABOVE")?,
                window_type: atom(conn, b"_NET_WM_WINDOW_TYPE")?,
                notification: atom(conn, b"_NET_WM_WINDOW_TYPE_NOTIFICATION")?,
            })
        }

        const fn required_states(&self) -> [u32; 3] {
            [self.skip_taskbar, self.skip_pager, self.above]
        }
    }

    fn contains_all(atoms: &[u32], required: &[u32]) -> bool {
        required.iter().all(|required| atoms.contains(required))
    }

    fn get_atoms(
        conn: &x11rb::rust_connection::RustConnection,
        wid: u32,
        property: u32,
    ) -> Vec<u32> {
        conn.get_property(
            false,
            wid,
            property,
            x11rb::protocol::xproto::AtomEnum::ATOM,
            0,
            128,
        )
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|reply| {
            let (words, _) = reply.value.as_chunks::<4>();
            words.iter().map(|word| u32::from_ne_bytes(*word)).collect()
        })
        .unwrap_or_default()
    }

    fn write_atoms(
        conn: &x11rb::rust_connection::RustConnection,
        wid: u32,
        property: u32,
        atoms: &[u32],
    ) {
        let bytes: Vec<u8> = atoms.iter().flat_map(|atom| atom.to_ne_bytes()).collect();
        if let Ok(len) = u32::try_from(atoms.len()) {
            let _ = conn.change_property(
                x11rb::protocol::xproto::PropMode::REPLACE,
                wid,
                property,
                x11rb::protocol::xproto::AtomEnum::ATOM,
                32,
                len,
                &bytes,
            );
        }
    }

    fn send_state_change(
        conn: &x11rb::rust_connection::RustConnection,
        root: u32,
        wid: u32,
        state: u32,
        first: u32,
        second: u32,
    ) {
        let event = x11rb::protocol::xproto::ClientMessageEvent {
            response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: wid,
            type_: state,
            data: x11rb::protocol::xproto::ClientMessageData::from([1, first, second, 1, 0]),
        };
        let _ = conn.send_event(
            false,
            root,
            x11rb::protocol::xproto::EventMask::SUBSTRUCTURE_REDIRECT
                | x11rb::protocol::xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        );
    }

    /// 补齐通知窗口状态;窗口类型已在 winit 创建属性中预置,这里同时读回验证。
    fn configure_notification_window(
        conn: &x11rb::rust_connection::RustConnection,
        root: u32,
        wid: u32,
    ) {
        let Some(atoms) = NotificationAtoms::load(conn) else {
            return;
        };

        let required_states = atoms.required_states();
        let mut states = get_atoms(conn, wid, atoms.state);
        let types = get_atoms(conn, wid, atoms.window_type);
        if types.contains(&atoms.notification) && contains_all(&states, &required_states) {
            return;
        }

        if !types.contains(&atoms.notification) {
            write_atoms(conn, wid, atoms.window_type, &[atoms.notification]);
        }
        if !contains_all(&states, &required_states) {
            super::merge_atoms(&mut states, &required_states);
            write_atoms(conn, wid, atoms.state, &states);
            send_state_change(
                conn,
                root,
                wid,
                atoms.state,
                atoms.skip_taskbar,
                atoms.skip_pager,
            );
            send_state_change(conn, root, wid, atoms.state, atoms.above, 0);
        }

        let applied_states = get_atoms(conn, wid, atoms.state);
        let applied_types = get_atoms(conn, wid, atoms.window_type);
        if !applied_types.contains(&atoms.notification)
            || !contains_all(&applied_states, &required_states)
        {
            log::warn!("Linux 通知窗口属性设置未生效: wid={wid}");
        }
    }

    /// 定位本服务窗口:EWMH 客户端清单优先;精简 WM 不维护清单时递归全树兜底
    /// (WM 会把客户端收进框架窗,须深入子树查找)
    fn find_window(conn: &x11rb::rust_connection::RustConnection, root: u32) -> Option<u32> {
        let list_atom = conn
            .intern_atom(false, b"_NET_CLIENT_LIST")
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|a| a.atom)?;
        if let Some(reply) = conn
            .get_property(
                false,
                root,
                list_atom,
                x11rb::protocol::xproto::AtomEnum::WINDOW,
                0,
                1024,
            )
            .ok()
            .and_then(|c| c.reply().ok())
        {
            // X11 线序为小端
            let (words, _) = reply.value.as_chunks::<4>();
            for wid in words.iter().map(|c| u32::from_le_bytes(*c)) {
                if window_matches(conn, wid) {
                    return Some(wid);
                }
            }
        }
        dfs_find(conn, root, 0)
    }

    fn dfs_find(conn: &x11rb::rust_connection::RustConnection, wid: u32, depth: u8) -> Option<u32> {
        if depth > 8 {
            return None;
        }
        if window_matches(conn, wid) {
            return Some(wid);
        }
        let children = conn
            .query_tree(wid)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.children);
        for child in children.into_iter().flatten() {
            if let Some(hit) = dfs_find(conn, child, depth + 1) {
                return Some(hit);
            }
        }
        None
    }

    /// 按窗口名识别本服务窗口:winit 设置的名称属性类型不定,两种都查
    fn window_matches(conn: &x11rb::rust_connection::RustConnection, wid: u32) -> bool {
        let target = b"x-notify-service";
        let prop_is = |prop: u32, ty: u32| {
            conn.get_property(false, wid, prop, ty, 0, 64)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.value == target)
        };
        let atom = |name: &[u8]| {
            conn.intern_atom(false, name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|a| a.atom)
        };
        prop_is(
            x11rb::protocol::xproto::AtomEnum::WM_NAME.into(),
            x11rb::protocol::xproto::AtomEnum::STRING.into(),
        ) || match (atom(b"_NET_WM_NAME"), atom(b"UTF8_STRING")) {
            (Some(name), Some(utf8)) => prop_is(name, utf8),
            _ => false,
        }
    }

    /// 写入图标负载:blob 已是构建期产出的线序字节,直接落属性
    fn write_blob(conn: &x11rb::rust_connection::RustConnection, wid: u32, blob: &[u8]) {
        let Some(atom) = conn
            .intern_atom(false, b"_NET_WM_ICON")
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|a| a.atom)
        else {
            return;
        };
        let _ = conn.change_property(
            x11rb::protocol::xproto::PropMode::REPLACE,
            wid,
            atom,
            x11rb::protocol::xproto::AtomEnum::CARDINAL,
            32,
            property_units(blob),
            blob,
        );
        log::debug!("已写入多档窗口图标");
    }
}

#[cfg(target_os = "linux")]
pub(super) fn set() {
    // X11 属性是增强能力,失败不得杀死事件循环线程
    if std::panic::catch_unwind(imp::set).is_err() {
        log::error!("窗口图标写入失败(panic 已拦截),通知功能不受影响");
    }
}

#[cfg(test)]
mod tests {
    use super::{ICON_BLOB, merge_atoms, property_units};

    #[test]
    fn merge_atoms_preserves_existing_and_adds_missing_values() {
        let mut atoms = vec![10, 20];

        merge_atoms(&mut atoms, &[20, 30, 40]);

        assert_eq!(atoms, vec![10, 20, 30, 40]);
    }

    /// 闪退根因回归:字节数必须换算为 32 位元素数
    #[test]
    fn property_length_counts_elements() {
        assert_eq!(property_units(&[0u8; 16]), 4);
        let big = vec![0u8; 79_904];
        assert_eq!(property_units(&big), 19_976);
    }

    /// 构建期生成的负载可整除且恰含四档图标(128/48/32/16)
    #[test]
    fn icon_blob_structure_valid() {
        assert_eq!(ICON_BLOB.len() & 3, 0, "负载须为 u32 流");
        let le = |o: usize| u32::from_le_bytes(ICON_BLOB[o..o + 4].try_into().unwrap());
        let mut off = 0;
        let mut sizes = Vec::new();
        while off < ICON_BLOB.len() {
            let (w, h) = (le(off) as usize, le(off + 4) as usize);
            sizes.push(w);
            assert_eq!(w, h, "图标须为正方形");
            off += 8 + w * h * 4;
        }
        assert_eq!(off, ICON_BLOB.len(), "负载无尾部残渣");
        assert_eq!(sizes, vec![128, 48, 32, 16], "四档按偏好序");
    }
}
