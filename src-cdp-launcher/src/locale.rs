#[allow(dead_code)]
pub(crate) struct Strings {
    pub title: &'static str,
    pub cdp_already_running: &'static str,
    pub restart_confirm: &'static str,
    pub restore_failure: &'static str,
}

const EN: Strings = Strings {
    title: "Discord CDP Launcher",
    cdp_already_running: "Discord is already running with CDP mode enabled.",
    restart_confirm: "Discord is already running. Do you want to restart it with CDP mode enabled?",
    restore_failure: "Some Discord clients could not be restarted in normal mode. Please fully quit and reopen Discord manually.",
};

const ZH: Strings = Strings {
    title: "Discord CDP 启动器",
    cdp_already_running: "Discord 已在 CDP 模式下运行。",
    restart_confirm: "Discord 正在运行。是否要重启并启用 CDP 模式？",
    restore_failure: "部分 Discord 客户端无法恢复到一般模式。请手动完全退出并重新打开 Discord。",
};

const ZH_TW: Strings = Strings {
    title: "Discord CDP 啟動器",
    cdp_already_running: "Discord 已在 CDP 模式下執行。",
    restart_confirm: "Discord 正在執行。是否要重新啟動並啟用 CDP 模式？",
    restore_failure: "部分 Discord 用戶端無法恢復至一般模式。請手動完全結束並重新開啟 Discord。",
};

const JA: Strings = Strings {
    title: "Discord CDP ランチャー",
    cdp_already_running: "Discord は既に CDP モードで実行中です。",
    restart_confirm: "Discord は実行中です。CDP モードを有効にして再起動しますか？",
    restore_failure: "一部の Discord クライアントを通常モードで再起動できませんでした。Discord を完全に終了して手動で再起動してください。",
};

const KO: Strings = Strings {
    title: "Discord CDP 런처",
    cdp_already_running: "Discord가 이미 CDP 모드로 실행 중입니다.",
    restart_confirm: "Discord가 실행 중입니다. CDP 모드를 활성화하여 재시작하시겠습니까?",
    restore_failure: "일부 Discord 클라이언트를 일반 모드로 다시 시작하지 못했습니다. Discord를 완전히 종료한 후 수동으로 다시 여세요.",
};

const RU: Strings = Strings {
    title: "Discord CDP Лаунчер",
    cdp_already_running: "Discord уже запущен в режиме CDP.",
    restart_confirm: "Discord уже запущен. Хотите перезапустить его с включенным CDP?",
    restore_failure: "Некоторые клиенты Discord не удалось перезапустить в обычном режиме. Полностью закройте и снова откройте Discord вручную.",
};

const ES: Strings = Strings {
    title: "Discord CDP Lanzador",
    cdp_already_running: "Discord ya está ejecutándose con el modo CDP activado.",
    restart_confirm: "Discord ya está ejecutándose. ¿Deseas reiniciarlo con el modo CDP activado?",
    restore_failure: "No se pudieron reiniciar algunos clientes de Discord en modo normal. Cierra Discord por completo y vuelve a abrirlo manualmente.",
};

pub(crate) fn get_strings() -> &'static Strings {
    #[cfg(target_os = "windows")]
    {
        let primary = crate::dialogs::system_ui_language() & 0x3ff;
        let full = crate::dialogs::system_ui_language();
        match primary {
            0x04 if matches!(full, 0x0404 | 0x0c04 | 0x1404) => &ZH_TW,
            0x04 => &ZH,
            0x11 => &JA,
            0x12 => &KO,
            0x19 => &RU,
            0x0a => &ES,
            _ => &EN,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let locale = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        match locale.as_str() {
            value
                if value.starts_with("zh_tw")
                    || value.starts_with("zh_hk")
                    || value.starts_with("zh_mo")
                    || value.starts_with("zh-hant") =>
            {
                &ZH_TW
            }
            value if value.starts_with("zh") => &ZH,
            value if value.starts_with("ja") => &JA,
            value if value.starts_with("ko") => &KO,
            value if value.starts_with("ru") => &RU,
            value if value.starts_with("es") => &ES,
            _ => &EN,
        }
    }
}
