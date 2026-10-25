(() => {
    if (String(performance.timeOrigin) !== __GENERATION__) throw Error('renderer_changed');
    if (!/^https:\/\/(?:canary\.|ptb\.)?discord(?:app)?\.com\//.test(location.href)) throw Error('not_discord');
    if (!window.webpackChunkdiscord_app) throw Error('webpack_unavailable');
    const req = webpackChunkdiscord_app.push([[Symbol()], {}, r => r]);
    webpackChunkdiscord_app.pop();
    const ids = __MODULE_IDS__;
    const candidates = module => {
        const exp = module?.exports;
        return [exp, exp?.default, ...(exp && typeof exp === 'object' ? Object.values(exp) : [])];
    };
    const find = (key, test) => {
        if (ids[key] && req.c[ids[key]]) {
            const match = candidates(req.c[ids[key]]).find(test);
            if (match) return match;
        }
        for (const [id, module] of Object.entries(req.c)) {
            try {
                const match = candidates(module).find(test);
                if (match) { ids[key] = id; return match; }
            } catch (_) {}
        }
        return null;
    };
    const analytics = find('properties', value => typeof value?.getSuperPropertiesBase64 === 'function' && typeof value?.getSuperProperties === 'function');
    const user = find('user', value => typeof value?.getCurrentUser === 'function' && typeof value?.getUser === 'function');
    const accountId = user?.getCurrentUser()?.id;
    if (!accountId) throw Error('account_unavailable');
    // Decode the cache that the client uses for HTTP requests.
    const base64 = analytics?.getSuperPropertiesBase64() ?? '';
    const decoded = base64 ? JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(base64), char => char.charCodeAt(0)))) : {};
    const windowStore = find('window', value => typeof value?.isAppFocused === 'function');
    const focused = windowStore ? !!windowStore.isAppFocused() : document.hasFocus();
    let heartbeatKnown = false, heartbeatSessionId = null;
    const sessionModule = id => {
        const factory = req.m?.[id];
        return typeof factory === 'function' && String(factory).includes('LAST_CLIENT_HEARTBEAT_SESSION') ? req.c[id]?.exports : null;
    };
    let heartbeat = ids.heartbeat ? sessionModule(ids.heartbeat) : null;
    if (!heartbeat) {
        for (const id of Object.keys(req.c)) {
            heartbeat = sessionModule(id);
            if (heartbeat) { ids.heartbeat = id; break; }
        }
    }
    // Read the synchronous session accessor without initializing the scheduler.
    if (typeof heartbeat?.Vc === 'function') {
        const current = heartbeat.Vc();
        if (current == null || typeof current.uuid === 'string') {
            heartbeatKnown = true; heartbeatSessionId = current?.uuid ?? null;
        }
    }
    const get = (obj, key) => { try { return Reflect.get(obj, key); } catch (_) { return undefined; } };
    const groups = [[19,['BetterDiscord','BetterDiscordPreload','BdApi']], [27,['rambox']],
        [36,['revenge','vendetta','bunny','kettu']], [43,['Vencord','VencordNative','VesktopNative','VencordMobile']],
        [52,['replugged','RepluggedNative']], [66,['legcord','LegcordRPC']],
        [72,['Dorion','__DORION_CONFIG__','__DORION_INIT__']], [79,['GCDP']], [89,['openasar']],
        [103,['shelter']], [116,['moonlight','moonlightNode']]];
    const modBitIndices = groups.filter(([, names]) => names.some(name => get(globalThis, name) !== undefined)).map(([bit]) => bit);
    if (['jQuery','$'].some(name => { const value = get(globalThis,name); if (value == null) return false; const fn=get(value,'fn'); return fn != null && get(fn,'jquery') != null; })) modBitIndices.push(8);
    const hasMods = ['jQuery','$','BetterDiscord','BdApi','rambox','Vencord','VencordNative'].some(name => get(window,name) != null);
    const native = window.DiscordNative;
    const fallback = {system_locale: navigator.languages?.[0] ?? navigator.language ?? '', has_client_mods:hasMods};
    if (native) {
        const platform = native.process.platform;
        Object.assign(fallback, {os: ({win32:'Windows',darwin:'Mac OS X',linux:'Linux'})[platform] ?? platform,
            browser:'Discord Client', release_channel:(window.GLOBAL_ENV?.RELEASE_CHANNEL ?? native.app.getReleaseChannel() ?? 'unknown').split('-')[0],
            client_version:native.app.getVersion(), os_version:native.os.release, os_arch:native.os.arch, app_arch:native.os.appArch});
        const nativeBuild = native.app.getBuildNumber();
        if (nativeBuild != null && !isNaN(nativeBuild)) fallback.native_build_number = nativeBuild;
        const electron = /Electron\/([^\s]+)/.exec(navigator.userAgent);
        if (electron) { fallback.browser_user_agent=navigator.userAgent; fallback.browser_version=electron[1]; }
        if (platform === 'win32') fallback.os_sdk_version=native.os.release?.split('.')[2];
        if (platform === 'darwin') fallback.os_sdk_version=native.os.release?.split('.')[0];
        if (platform === 'linux') {
            const meta=native.crashReporter?.getMetadata?.() ?? {};
            Object.assign(fallback,{window_manager:meta.wm,distro:meta.distro,runtime_environment:meta.runtime_environment,display_server:meta.display_server});
        }
    }
    return {base64, decoded, fallback, focused, heartbeat_known:heartbeatKnown, heartbeat_session_id:heartbeatSessionId,
        account_id:String(accountId), renderer_key:String(performance.timeOrigin), mod_bit_indices:modBitIndices, module_ids:ids, native_desktop:!!native};
})()
