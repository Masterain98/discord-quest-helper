#[allow(dead_code)]
pub(crate) struct Strings {
    pub title: &'static str,
    pub cdp_already_running: &'static str,
    pub restart_confirm: &'static str,
}

const EN: Strings = Strings {
    title: "Discord CDP Launcher",
    cdp_already_running: "Discord is already running with CDP mode enabled.",
    restart_confirm: "Discord is already running. Do you want to restart it with CDP mode enabled?",
};

#[cfg(target_os = "windows")]
const ZH: Strings = Strings {
    title: "Discord CDP 启动器",
    cdp_already_running: "Discord 已在 CDP 模式下运行。",
    restart_confirm: "Discord 正在运行。是否要重启并启用 CDP 模式？",
};

#[cfg(target_os = "windows")]
const ZH_TW: Strings = Strings {
    title: "Discord CDP 啟動器",
    cdp_already_running: "Discord 已在 CDP 模式下執行。",
    restart_confirm: "Discord 正在執行。是否要重新啟動並啟用 CDP 模式？",
};

#[cfg(target_os = "windows")]
const JA: Strings = Strings {
    title: "Discord CDP ランチャー",
    cdp_already_running: "Discord は既に CDP モードで実行中です。",
    restart_confirm: "Discord は実行中です。CDP モードを有効にして再起動しますか？",
};

#[cfg(target_os = "windows")]
const KO: Strings = Strings {
    title: "Discord CDP 런처",
    cdp_already_running: "Discord가 이미 CDP 모드로 실행 중입니다.",
    restart_confirm: "Discord가 실행 중입니다. CDP 모드를 활성화하여 재시작하시겠습니까?",
};

#[cfg(target_os = "windows")]
const RU: Strings = Strings {
    title: "Discord CDP Лаунчер",
    cdp_already_running: "Discord уже запущен в режиме CDP.",
    restart_confirm: "Discord уже запущен. Хотите перезапустить его с включенным CDP?",
};

#[cfg(target_os = "windows")]
const ES: Strings = Strings {
    title: "Discord CDP Lanzador",
    cdp_already_running: "Discord ya está ejecutándose con el modo CDP activado.",
    restart_confirm: "Discord ya está ejecutándose. ¿Deseas reiniciarlo con el modo CDP activado?",
};

pub(crate) fn get_strings() -> &'static Strings {
    #[cfg(target_os = "windows")]
    {
        let primary = crate::dialogs::system_ui_language() & 0x3ff;
        let full = crate::dialogs::system_ui_language();
        match primary {
            0x04 if full == 0x0404 => &ZH_TW,
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
        &EN
    }
}
