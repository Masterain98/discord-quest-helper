#[allow(dead_code)]
pub(crate) struct Strings {
    pub title: &'static str,
    pub cdp_already_running: &'static str,
    pub restart_confirm: &'static str,
    pub restore_failure: &'static str,
    pub restore_retry: &'static str,
    pub restore_action: &'static str,
    pub status_action: &'static str,
    pub launch_action: &'static str,
    pub launch_success: &'static str,
    pub restart_instruction: &'static str,
}

const EN: Strings = Strings {
    title: "Runtime Launcher",
    cdp_already_running: "Discord is already running with CDP mode enabled.",
    restart_confirm: "Discord is already running. Do you want to restart it with CDP mode enabled?",
    restore_failure: "Some Discord clients could not be confirmed in normal mode. Please fully quit and reopen Discord manually.",
    restore_retry: "Please fully quit Discord and try again.",
    restore_action: "Restore",
    status_action: "Status check",
    launch_action: "Launch",
    launch_success: "Launched Discord {channel} with CDP on port {port}.",
    restart_instruction: "Discord is already running without CDP. Re-run with --restart to close it and relaunch with CDP.",
};

const ZH: Strings = Strings {
    title: "运行时启动器",
    cdp_already_running: "Discord 已在 CDP 模式下运行。",
    restart_confirm: "Discord 正在运行。是否要重启并启用 CDP 模式？",
    restore_failure:
        "无法确认部分 Discord 客户端是否已恢复到一般模式。请手动完全退出并重新打开 Discord。",
    restore_retry: "请完全退出 Discord 后重试。",
    restore_action: "恢复",
    status_action: "状态检查",
    launch_action: "启动",
    launch_success: "已启动 Discord {channel}，CDP 端口为 {port}。",
    restart_instruction:
        "Discord 已在无 CDP 模式下运行。请使用 --restart 重新运行以关闭并启用 CDP。",
};

const ZH_TW: Strings = Strings {
    title: "執行階段啟動器",
    cdp_already_running: "Discord 已在 CDP 模式下執行。",
    restart_confirm: "Discord 正在執行。是否要重新啟動並啟用 CDP 模式？",
    restore_failure:
        "無法確認部分 Discord 用戶端是否已恢復至一般模式。請手動完全結束並重新開啟 Discord。",
    restore_retry: "請完全結束 Discord 後再試一次。",
    restore_action: "恢復",
    status_action: "狀態檢查",
    launch_action: "啟動",
    launch_success: "已啟動 Discord {channel}，CDP 連接埠為 {port}。",
    restart_instruction:
        "Discord 已在未啟用 CDP 的模式下執行。請使用 --restart 重新執行以關閉並啟用 CDP。",
};

const JA: Strings = Strings {
    title: "ランタイムランチャー",
    cdp_already_running: "Discord は既に CDP モードで実行中です。",
    restart_confirm: "Discord は実行中です。CDP モードを有効にして再起動しますか？",
    restore_failure: "一部の Discord クライアントが通常モードに復帰したことを確認できませんでした。Discord を完全に終了して手動で再起動してください。",
    restore_retry: "Discord を完全に終了して、もう一度お試しください。",
    restore_action: "復元",
    status_action: "状態確認",
    launch_action: "起動",
    launch_success: "Discord {channel} を CDP ポート {port} で起動しました。",
    restart_instruction: "Discord は CDP なしで実行中です。--restart を付けて再実行すると終了して CDP で起動します。",
};

const KO: Strings = Strings {
    title: "런타임 런처",
    cdp_already_running: "Discord가 이미 CDP 모드로 실행 중입니다.",
    restart_confirm: "Discord가 실행 중입니다. CDP 모드를 활성화하여 재시작하시겠습니까?",
    restore_failure: "일부 Discord 클라이언트가 일반 모드로 복원되었는지 확인할 수 없습니다. Discord를 완전히 종료한 후 수동으로 다시 여세요.",
    restore_retry: "Discord를 완전히 종료한 후 다시 시도하세요.",
    restore_action: "복원",
    status_action: "상태 확인",
    launch_action: "실행",
    launch_success: "Discord {channel}을(를) CDP 포트 {port}에서 실행했습니다.",
    restart_instruction: "Discord가 CDP 없이 실행 중입니다. --restart를 사용해 다시 실행하면 종료 후 CDP로 시작합니다.",
};

const RU: Strings = Strings {
    title: "Средство запуска",
    cdp_already_running: "Discord уже запущен в режиме CDP.",
    restart_confirm: "Discord уже запущен. Хотите перезапустить его с включенным CDP?",
    restore_failure: "Не удалось подтвердить, что некоторые клиенты Discord восстановлены в обычном режиме. Полностью закройте и снова откройте Discord вручную.",
    restore_retry: "Полностью закройте Discord и повторите попытку.",
    restore_action: "Восстановление",
    status_action: "Проверка состояния",
    launch_action: "Запуск",
    launch_success: "Discord {channel} запущен с CDP на порту {port}.",
    restart_instruction: "Discord уже запущен без CDP. Запустите снова с --restart, чтобы закрыть его и включить CDP.",
};

const ES: Strings = Strings {
    title: "Iniciador de Runtime",
    cdp_already_running: "Discord ya está ejecutándose con el modo CDP activado.",
    restart_confirm: "Discord ya está ejecutándose. ¿Deseas reiniciarlo con el modo CDP activado?",
    restore_failure: "No se pudo confirmar que algunos clientes de Discord se restauraran al modo normal. Cierra Discord por completo y vuelve a abrirlo manualmente.",
    restore_retry: "Cierra Discord por completo y vuelve a intentarlo.",
    restore_action: "Restaurar",
    status_action: "Comprobar estado",
    launch_action: "Iniciar",
    launch_success: "Discord {channel} se inició con CDP en el puerto {port}.",
    restart_instruction: "Discord ya se está ejecutando sin CDP. Vuelve a ejecutar con --restart para cerrarlo e iniciarlo con CDP.",
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
