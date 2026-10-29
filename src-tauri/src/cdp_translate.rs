//! CDP-based message translation module
//!
//! Installs a resident script into the Discord renderer (via the same
//! Runtime.evaluate pattern as `cdp_quest`) that decorates messages with a
//! Translate button, auto-translates incoming messages through a
//! MutationObserver, adds composer Translate / Summarize buttons, and renders
//! translations, toasts and the summary panel.
//!
//! Transport contract: the Rust ticker polls the page with `js_drain` every
//! tick; the drain doubles as a health check (a `ready:false` reply triggers a
//! re-init after a Discord reload). Provider calls happen only in Rust — the
//! page never reaches the network for translations, so provider failures
//! cannot affect the CDP connection or the quest pipeline.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::cdp_client;
use crate::discord_api::DiscordApiClient;
use crate::logger::{log, LogCategory, LogLevel};
use crate::translation::{
    channel_summary_system_prompt, channel_summary_user_prompt, composer_translation_system_prompt,
    composer_translation_user_prompt, resolve_keys, validate_llm_base_url, TranslationCache,
    TranslationConfig, TranslationHttp, TranslationJob, TranslationJobKind, TranslationJobResult,
    TranslationProvider,
};

/// How often the ticker drains the page queue.
const TICK_INTERVAL_MS: u64 = 250;
/// CDP evaluation timeout for drain/deliver calls.
const TICK_EVAL_TIMEOUT_SECS: u64 = 8;
/// Maximum jobs pulled from the page per tick.
const MAX_DRAIN_ITEMS: usize = 24;
/// Version of the resident script; bumping it forces a reinstall.
const RESIDENT_SCRIPT_VERSION: u32 = 1;
/// Fallback accent color when the configured one is not a valid CSS color.
const DEFAULT_ACCENT_COLOR: &str = "#51efc5";

// ---------------------------------------------------------------------------
// Resident injected script
// ---------------------------------------------------------------------------

const JS_INIT_TRANSLATE: &str = r###"
(async () => {
    const VERSION = __DQH_VERSION__;
    const CONFIG = __DQH_CONFIG_JSON__;
    try {
        const existing = window.__dqh_translate;
        if (existing && existing.initialized && existing._initVersion === VERSION) {
            existing.config = CONFIG;
            existing.scan && existing.scan();
            return JSON.stringify({ ready: true, cached: true });
        }
        if (existing && existing.observer) {
            try { existing.observer.disconnect(); } catch (e) {}
        }

        const CLASS = {
            row: "dqh-action-row",
            btn: "dqh-action-btn",
            btnTranslate: "dqh-btn-message-translate",
            composerTranslate: "dqh-btn-composer-translate",
            summary: "dqh-btn-summary",
            divider: "dqh-divider",
            toast: "dqh-toast"
        };
        const ATTR = { original: "data-dqh-original", translation: "data-dqh-translation" };
        const SEL = {
            article: '[role="article"][aria-roledescription="Message"]',
            content: '[id^="message-content-"]',
            editor: 'form [role="textbox"][data-slate-editor="true"][contenteditable="true"]',
            buttonsContainer: '[class*="buttons_"]'
        };

        const state = {
            initialized: true,
            _initVersion: VERSION,
            config: CONFIG,
            observer: null,
            manualQueue: [],
            pending: new Map(),
            autoQueue: new Map(),
            sessionCache: new Map(),
            latestKeys: new Map(),
            failedKeys: new Set(),
            stats: { translated: 0, cacheHits: 0, errors: 0 },
            scanTimer: null,
            nextId: 1
        };
        window.__dqh_translate = state;

        const STYLE_ID = "dqh-translate-styles";
        const existingStyle = document.getElementById(STYLE_ID);
        if (existingStyle) existingStyle.remove();
        const style = document.createElement("style");
        style.id = STYLE_ID;
        style.textContent = [
            "." + CLASS.row + " { display:flex; gap:4px; padding:2px 10px 4px 56px; flex-wrap:wrap; }",
            "." + CLASS.btn + " { appearance:none; border:1px solid var(--background-modifier-accent,#4e5058); background:var(--background-secondary,#2b2d31); color:var(--text-normal,#dbdee1); font-size:11px; font-weight:600; padding:2px 8px; border-radius:4px; cursor:pointer; line-height:16px; }",
            "." + CLASS.btn + ":hover { background:var(--background-modifier-hover,#35373c); }",
            "." + CLASS.btn + ":disabled { opacity:.5; cursor:default; }",
            "[" + ATTR.translation + "] { color:" + CONFIG.accentColor + "; margin-top:6px; white-space:pre-wrap; word-break:break-word; }",
            "." + CLASS.divider + " { border:0; border-top:1px solid var(--background-modifier-accent,#4e5058); margin:6px 0 0 0; }",
            ".dqh-summary-panel { position:fixed; right:20px; bottom:20px; width:min(420px,calc(100vw - 32px)); max-height:70vh; overflow:auto; z-index:2147483647; padding:18px; border-radius:18px; border:1px solid rgba(255,255,255,.12); background:rgba(17,18,20,.98); box-shadow:0 24px 70px rgba(0,0,0,.45); color:#f5f7fa; font-family:'gg sans',Inter,system-ui,sans-serif; font-size:14px; }",
            "." + CLASS.toast + " { position:fixed; left:50%; transform:translateX(-50%); bottom:90px; z-index:2147483647; background:#1a1b1e; color:#f5f7fa; border:1px solid rgba(255,255,255,.14); padding:10px 14px; border-radius:10px; font-size:13px; max-width:min(480px,80vw); box-shadow:0 12px 40px rgba(0,0,0,.4); }"
        ].join("\n");
        document.head.appendChild(style);

        function isVisible(element) {
            return !!(element && element.offsetParent !== null);
        }
        function getArticles() {
            return Array.from(document.querySelectorAll(SEL.article));
        }
        function getContentElement(article) {
            return article.querySelector(SEL.content);
        }
        function stableId(article, contentElement) {
            return article.id || (contentElement && contentElement.id) || "";
        }
        function hashString(value) {
            let hash = 0;
            for (let index = 0; index < value.length; index += 1) {
                hash = ((hash << 5) - hash + value.charCodeAt(index)) | 0;
            }
            return Math.abs(hash).toString(36);
        }
        function requestKey(stableMessageId, text) {
            const textHash = hashString(text);
            return stableMessageId ? stableMessageId + ":" + textHash : textHash;
        }
        function identityKey(stableMessageId, text) {
            return state.config.provider + ":" + state.config.targetLanguage + ":" + requestKey(stableMessageId, text);
        }

        // DOM -> markdown-ish text; skips our own injected nodes.
        function domToMarkdown(node) {
            if (node.nodeType === Node.TEXT_NODE) return node.nodeValue || "";
            if (node.nodeType !== Node.ELEMENT_NODE) return "";
            const el = node;
            if (el.hasAttribute && (el.hasAttribute(ATTR.translation) || el.classList.contains(CLASS.divider))) return "";
            if (el.tagName === "BR") return "\n";
            if (el.tagName === "CODE" && !el.closest("pre")) return "`" + el.textContent + "`";
            if (el.tagName === "PRE") {
                const codeEl = el.querySelector("code");
                const code = ((codeEl ? codeEl.textContent : el.textContent) || "").replace(/\n$/, "");
                return "\n```\n" + code + "\n```\n";
            }
            if (el.tagName === "A" && el.href) {
                const text = el.textContent || "";
                return text === el.href ? el.href : "[" + text + "](" + el.href + ")";
            }
            if (el.tagName === "IMG") return el.getAttribute("alt") || "";
            if (el.getAttribute("role") === "img") return el.getAttribute("aria-label") || "";
            if (el.tagName === "UL" || el.tagName === "OL") {
                const items = Array.from(el.children).filter(child => child.tagName === "LI");
                return items.map((li, index) =>
                    (el.tagName === "OL" ? (index + 1) + ". " : "- ") + domToMarkdown(li).trim()
                ).join("\n");
            }
            if (el.tagName === "BLOCKQUOTE") {
                return Array.from(el.childNodes).map(domToMarkdown).join("")
                    .trim().split("\n").map(line => "> " + line).join("\n");
            }
            let inner = "";
            for (const child of el.childNodes) inner += domToMarkdown(child);
            return inner;
        }
        function getMessageText(contentElement) {
            const raw = domToMarkdown(contentElement);
            return raw.replace(/\uFEFF/g, "").replace(/\n{3,}/g, "\n\n").trim();
        }
        function getAuthorName(article) {
            const username = article.querySelector('[id^="message-username-"]');
            return (username && username.textContent || "").trim();
        }

        const CODE_BLOCK_SELECTOR = "pre, code";
        const COMMAND_PREFIX_PATTERN = /^(\/\w+|[>$#]\s*\w+|(npm|npx|pnpm|yarn|bun|node|python|pip|git|curl|docker|kubectl|uv|cargo|go|java|javac|dotnet|powershell|winget|brew|sudo|apt(-get)?)(\s|$))/i;
        const CODE_FENCE_PATTERN = /```|`[^`]+`/;
        const CODE_SYMBOL_PATTERN = /[{}[\]();<>:=]{3,}|=>|::|&&|\|\|/;
        function looksLikeCodeOrCommand(text) {
            if (!text.trim()) return false;
            if (CODE_FENCE_PATTERN.test(text)) return true;
            const rawLines = text.split("\n").filter(line => line.trim());
            if (!rawLines.length) return false;
            const commandLineCount = rawLines.filter(line => COMMAND_PREFIX_PATTERN.test(line.trim())).length;
            if (commandLineCount >= Math.max(1, Math.floor(rawLines.length / 2))) return true;
            const symbolLineCount = rawLines.filter(line => CODE_SYMBOL_PATTERN.test(line)).length;
            return symbolLineCount >= Math.max(2, Math.ceil(rawLines.length * 0.6));
        }

        function showToast(message) {
            try {
                const toast = document.createElement("div");
                toast.className = CLASS.toast;
                toast.textContent = message;
                document.body.appendChild(toast);
                window.setTimeout(() => toast.remove(), 4000);
            } catch (e) {}
        }

        function upsertTranslation(contentElement, text, canonicalKey, identityKeyValue) {
            let wrapper = contentElement.querySelector("[" + ATTR.original + "]");
            if (!wrapper) {
                wrapper = document.createElement("div");
                wrapper.setAttribute(ATTR.original, "true");
                const nodes = Array.from(contentElement.childNodes).filter(node =>
                    !(node.nodeType === 1 && (node.hasAttribute(ATTR.translation) || node.classList.contains(CLASS.divider)))
                );
                nodes.forEach(node => wrapper.appendChild(node));
                contentElement.prepend(wrapper);
            }
            let divider = contentElement.querySelector("." + CLASS.divider);
            let translation = contentElement.querySelector("[" + ATTR.translation + "]");
            if (!divider) {
                divider = document.createElement("hr");
                divider.className = CLASS.divider;
            }
            if (!translation) {
                translation = document.createElement("div");
                translation.setAttribute(ATTR.translation, "true");
            }
            translation.textContent = text;
            translation.style.color = state.config.accentColor;
            translation.setAttribute("data-dqh-key", canonicalKey || "");
            translation.setAttribute("data-dqh-identity", identityKeyValue || "");
            wrapper.style.display = state.config.displayMode === "replace" ? "none" : "";
            if (!divider.isConnected) contentElement.appendChild(divider);
            if (!translation.isConnected) contentElement.appendChild(translation);
        }

        function clearTranslation(contentElement) {
            const translation = contentElement.querySelector("[" + ATTR.translation + "]");
            if (translation) translation.remove();
            const divider = contentElement.querySelector("." + CLASS.divider);
            if (divider) divider.remove();
            const wrapper = contentElement.querySelector("[" + ATTR.original + "]");
            if (wrapper) {
                wrapper.style.display = "";
            }
        }

        function ensureActionRow(article) {
            let row = article.querySelector(":scope > ." + CLASS.row);
            if (!row) {
                row = document.createElement("div");
                row.className = CLASS.row;
                article.appendChild(row);
            }
            return row;
        }

        function requestManual(article, contentElement, button) {
            const text = getMessageText(contentElement);
            if (!text) return;
            const identityKeyValue = identityKey(stableId(article, contentElement), text);
            const cached = state.sessionCache.get(identityKeyValue);
            if (cached) {
                upsertTranslation(contentElement, cached.text, cached.key, identityKeyValue);
                article.setAttribute("data-dqh-translated", "true");
                state.stats.cacheHits += 1;
                return;
            }
            const id = "m" + state.nextId;
            state.nextId += 1;
            state.pending.set(id, { article, contentElement, identityKey: identityKeyValue, stableId: stableId(article, contentElement), kind: "message" });
            state.manualQueue.push({
                id,
                kind: "message",
                text,
                stableId: stableId(article, contentElement),
                targetLanguage: state.config.targetLanguage
            });
            if (button) {
                button.disabled = true;
                button.textContent = state.config.labels.translating;
            }
        }

        function ensureTranslateButton(article, contentElement) {
            const row = ensureActionRow(article);
            let button = row.querySelector(":scope > ." + CLASS.btnTranslate);
            if (!button) {
                button = document.createElement("button");
                button.type = "button";
                button.className = CLASS.btn + " " + CLASS.btnTranslate;
                button.addEventListener("click", event => {
                    event.preventDefault();
                    event.stopPropagation();
                    requestManual(article, contentElement, button);
                });
                row.appendChild(button);
            }
            button.textContent = state.config.labels.translate;
        }

        function getComposerContext() {
            const editor = Array.from(document.querySelectorAll(SEL.editor)).find(isVisible);
            if (!editor) return null;
            const form = editor.closest("form");
            const container = form ? form.querySelector(SEL.buttonsContainer) : null;
            return { editor, container };
        }
        function getChannelName() {
            const main = document.querySelector('[role="main"]');
            if (!main) return "";
            const heading = main.querySelector("h1, h2, h3");
            return (heading && heading.textContent || "").trim();
        }
        function collectRecentChannelMessages(limit) {
            const articles = getArticles().slice(-limit);
            const lines = [];
            for (const article of articles) {
                const contentElement = getContentElement(article);
                if (!contentElement) continue;
                const text = getMessageText(contentElement);
                if (!text) continue;
                const author = getAuthorName(article) || "Unknown";
                lines.push(author + ": " + text.split("\n").join(" "));
            }
            return lines.join("\n");
        }

        function resetComposerButtons() {
            const ctx = getComposerContext();
            if (!ctx || !ctx.container) return;
            const translateButton = ctx.container.querySelector("." + CLASS.composerTranslate);
            if (translateButton) {
                translateButton.disabled = false;
                translateButton.textContent = state.config.labels.translate;
            }
            const summaryButton = ctx.container.querySelector("." + CLASS.summary);
            if (summaryButton) {
                summaryButton.disabled = false;
                summaryButton.textContent = state.config.labels.summarize.replace("{count}", String(state.config.summaryCount));
            }
        }

        function requestComposerTranslation() {
            const ctx = getComposerContext();
            if (!ctx) return;
            const draft = ((ctx.editor.innerText || ctx.editor.textContent || "").replace(/\uFEFF/g, "")).trim();
            if (!draft) {
                showToast(state.config.labels.composerEmpty);
                return;
            }
            if (!state.config.llm) {
                showToast(state.config.labels.llmMissing);
                return;
            }
            const translateButton = ctx.container && ctx.container.querySelector("." + CLASS.composerTranslate);
            if (translateButton) {
                translateButton.disabled = true;
                translateButton.textContent = state.config.labels.translating;
            }
            const id = "c" + state.nextId;
            state.nextId += 1;
            state.pending.set(id, { kind: "composer" });
            state.manualQueue.push({
                id,
                kind: "composer",
                text: draft,
                stableId: "",
                targetLanguage: state.config.targetLanguage,
                channelName: getChannelName() || null,
                recentMessages: collectRecentChannelMessages(6) || null
            });
        }

        function requestSummary() {
            const channelIdMatch = location.pathname.match(/^\/channels\/[^/]+\/(\d+)/);
            if (!channelIdMatch) {
                showToast(state.config.labels.llmMissing);
                return;
            }
            if (!state.config.llm) {
                showToast(state.config.labels.llmMissing);
                return;
            }
            const ctx = getComposerContext();
            const summaryButton = ctx && ctx.container && ctx.container.querySelector("." + CLASS.summary);
            if (summaryButton) {
                summaryButton.disabled = true;
                summaryButton.textContent = state.config.labels.summarizing;
            }
            const id = "s" + state.nextId;
            state.nextId += 1;
            state.pending.set(id, { kind: "summary" });
            state.manualQueue.push({
                id,
                kind: "summary",
                text: "",
                stableId: "",
                targetLanguage: state.config.targetLanguage,
                channelId: channelIdMatch[1],
                channelName: getChannelName() || null,
                messageCount: state.config.summaryCount
            });
        }

        function ensureComposerButtons() {
            const ctx = getComposerContext();
            if (!ctx || !ctx.container) return;
            let translateButton = ctx.container.querySelector(":scope > ." + CLASS.composerTranslate);
            if (!translateButton) {
                translateButton = document.createElement("button");
                translateButton.type = "button";
                translateButton.className = CLASS.btn + " " + CLASS.composerTranslate;
                translateButton.addEventListener("click", event => {
                    event.preventDefault();
                    event.stopPropagation();
                    requestComposerTranslation();
                });
                ctx.container.prepend(translateButton);
            }
            translateButton.textContent = state.config.labels.translate;
            if (!state.config.llm) return;
            let summaryButton = ctx.container.querySelector(":scope > ." + CLASS.summary);
            if (!summaryButton) {
                summaryButton = document.createElement("button");
                summaryButton.type = "button";
                summaryButton.className = CLASS.btn + " " + CLASS.summary;
                summaryButton.addEventListener("click", event => {
                    event.preventDefault();
                    event.stopPropagation();
                    requestSummary();
                });
                ctx.container.prepend(summaryButton);
            }
            summaryButton.textContent = state.config.labels.summarize.replace("{count}", String(state.config.summaryCount));
        }

        function buildSummaryPanel(result) {
            let panel = document.getElementById("dqh-summary-panel");
            if (!panel) {
                panel = document.createElement("aside");
                panel.id = "dqh-summary-panel";
                panel.className = "dqh-summary-panel";
                document.body.appendChild(panel);
            }
            panel.textContent = "";
            const header = document.createElement("div");
            header.style.cssText = "margin-bottom:12px;";
            const title = document.createElement("div");
            title.style.cssText = "font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:rgba(255,255,255,.6);";
            title.textContent = state.config.labels.summaryTitle;
            const channel = document.createElement("div");
            channel.style.cssText = "font-size:16px;font-weight:700;color:#fff;margin-top:4px;";
            channel.textContent = result.channelName ? "#" + result.channelName : "Current channel";
            const subtitle = document.createElement("div");
            subtitle.style.cssText = "font-size:12px;color:rgba(255,255,255,.55);margin-top:4px;";
            subtitle.textContent = state.config.labels.summarySubtitle.replace("{count}", String(result.messageCount || state.config.summaryCount));
            header.appendChild(title); header.appendChild(channel); header.appendChild(subtitle);
            const body = document.createElement("div");
            body.style.cssText = "line-height:1.65;white-space:pre-wrap;";
            body.textContent = result.ok ? (result.text || "") : ((state.config.labels.errorPrefix || "Error") + ": " + (result.error || ""));
            const footer = document.createElement("div");
            footer.style.cssText = "display:flex;justify-content:flex-end;gap:10px;margin-top:14px;";
            const copyButton = document.createElement("button");
            copyButton.type = "button";
            copyButton.textContent = state.config.labels.copy;
            copyButton.style.cssText = "border:1px solid rgba(255,255,255,.25);background:transparent;color:#f5f7fa;border-radius:999px;padding:8px 12px;font-size:12px;font-weight:700;cursor:pointer;";
            copyButton.addEventListener("click", async () => {
                try {
                    await navigator.clipboard.writeText(body.textContent || "");
                    copyButton.textContent = state.config.labels.copied;
                    window.setTimeout(() => { copyButton.textContent = state.config.labels.copy; }, 1400);
                } catch (error) {}
            });
            const closeButton = document.createElement("button");
            closeButton.type = "button";
            closeButton.textContent = state.config.labels.close;
            closeButton.style.cssText = "border:0;background:transparent;color:rgba(255,255,255,.6);font-size:20px;cursor:pointer;line-height:1;";
            closeButton.addEventListener("click", () => panel.remove());
            footer.appendChild(copyButton); footer.appendChild(closeButton);
            panel.appendChild(header); panel.appendChild(body); panel.appendChild(footer);
        }

        function rememberSessionCache(key, text, canonicalKey) {
            state.sessionCache.delete(key);
            state.sessionCache.set(key, { text, key: canonicalKey });
            while (state.sessionCache.size > 200) {
                const oldest = state.sessionCache.keys().next().value;
                if (!oldest) break;
                state.sessionCache.delete(oldest);
            }
        }

        function drain() {
            if (!window.__dqh_translate || !window.__dqh_translate.initialized) {
                return JSON.stringify({ ready: false, items: [] });
            }
            for (const [key, item] of state.autoQueue) {
                if (!item.article.isConnected || !item.contentElement.isConnected) {
                    if (item.article) delete item.article.dataset.dqhQueued;
                    state.autoQueue.delete(key);
                }
            }
            for (const [id, item] of state.pending) {
                if (item.article && !item.article.isConnected) {
                    state.pending.delete(id);
                }
            }
            const items = [];
            while (state.manualQueue.length && items.length < 24) {
                items.push(state.manualQueue.shift());
            }
            for (const [key, item] of state.autoQueue) {
                if (items.length >= 24) break;
                if (!item.article.isConnected) {
                    delete item.article.dataset.dqhQueued;
                    state.autoQueue.delete(key);
                    continue;
                }
                const id = "a" + state.nextId;
                state.nextId += 1;
                state.pending.set(id, item);
                items.push({
                    id,
                    kind: "message",
                    text: item.text,
                    stableId: item.stableId,
                    targetLanguage: state.config.targetLanguage
                });
                state.autoQueue.delete(key);
            }
            return JSON.stringify({ ready: true, items, stats: state.stats });
        }

        function deliver(results) {
            let applied = 0;
            for (const result of (results || [])) {
                const item = state.pending.get(result.id);
                state.pending.delete(result.id);
                if (result.kind === "summary") {
                    buildSummaryPanel(result);
                    resetComposerButtons();
                    if (result.ok) state.stats.translated += 1; else state.stats.errors += 1;
                    applied += 1;
                    continue;
                }
                if (result.kind === "composer") {
                    resetComposerButtons();
                    const ctx = getComposerContext();
                    if (result.ok && ctx) {
                        replaceComposerText(ctx.editor, result.text || "");
                        showToast("Composer translated");
                        state.stats.translated += 1;
                    } else if (!result.ok) {
                        showToast(result.error || "Translation failed");
                        state.stats.errors += 1;
                    }
                    applied += 1;
                    continue;
                }
                if (!item) continue;
                const row = item.article.querySelector(":scope > ." + CLASS.row);
                const button = row && row.querySelector("." + CLASS.btnTranslate);
                if (button) {
                    button.disabled = false;
                    button.textContent = state.config.labels.translate;
                }
                if (result.ok) {
                    rememberSessionCache(item.identityKey, result.text || "", result.cacheKey || "");
                    if (item.stableId) {
                        state.latestKeys.set(item.stableId, { canonicalKey: result.cacheKey || "", identityKey: item.identityKey });
                    }
                    upsertTranslation(item.contentElement, result.text || "", result.cacheKey || "", item.identityKey);
                    item.article.setAttribute("data-dqh-translated", "true");
                    state.stats.translated += 1;
                    if (result.cacheHit) state.stats.cacheHits += 1;
                } else {
                    state.failedKeys.add(item.identityKey);
                    state.stats.errors += 1;
                    upsertTranslation(item.contentElement, (state.config.labels.errorPrefix || "Error") + ": " + (result.error || ""), "", item.identityKey);
                    item.article.setAttribute("data-dqh-translated", "true");
                }
                applied += 1;
            }
            if (state.scan) state.scan();
            return JSON.stringify({ applied });
        }

        function replaceComposerText(editor, text) {
            try {
                editor.focus();
                const selection = window.getSelection();
                if (selection) {
                    const range = document.createRange();
                    range.selectNodeContents(editor);
                    selection.removeAllRanges();
                    selection.addRange(range);
                }
                document.execCommand("delete", false);
                document.execCommand("insertText", false, text);
                editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: null }));
                editor.dispatchEvent(new Event("change", { bubbles: true }));
                return true;
            } catch (error) {
                try {
                    editor.innerText = text;
                    editor.dispatchEvent(new Event("input", { bubbles: true }));
                    return true;
                } catch (innerError) {
                    return false;
                }
            }
        }

        function scanDebounceMs() {
            const pending = state.autoQueue.size;
            return pending >= 10 ? 120 : pending >= 4 ? 200 : 320;
        }
        function scheduleScan() {
            if (state.scanTimer !== null) return;
            state.scanTimer = window.setTimeout(() => {
                state.scanTimer = null;
                scan();
            }, scanDebounceMs());
        }
        function scan() {
            const articles = getArticles();
            for (const article of articles) {
                const contentElement = getContentElement(article);
                if (!contentElement) continue;
                if (!state.config.autoTranslate) {
                    ensureTranslateButton(article, contentElement);
                    continue;
                }
                const row = article.querySelector(":scope > ." + CLASS.row);
                if (row) {
                    const button = row.querySelector("." + CLASS.btnTranslate);
                    if (button) button.remove();
                    if (!row.children.length) row.remove();
                }
                if (article.getAttribute("data-dqh-translated") === "true") continue;
                if (article.getAttribute("data-dqh-queued") === "true") continue;
                const text = getMessageText(contentElement);
                if (!text) continue;
                const key = identityKey(stableId(article, contentElement), text);
                if (state.failedKeys.has(key)) continue;
                const cached = state.sessionCache.get(key);
                if (cached) {
                    upsertTranslation(contentElement, cached.text, cached.key, key);
                    article.setAttribute("data-dqh-translated", "true");
                    continue;
                }
                if (looksLikeCodeOrCommand(text)) continue;
                if (state.autoQueue.has(key)) continue;
                const stable = stableId(article, contentElement);
                const previous = state.latestKeys.get(stable);
                if (previous && previous.identityKey !== key) {
                    state.sessionCache.delete(previous.identityKey);
                    state.latestKeys.delete(stable);
                }
                article.setAttribute("data-dqh-queued", "true");
                state.autoQueue.set(key, { article, contentElement, identityKey: key, stableId: stable, text });
                while (state.autoQueue.size > 60) {
                    const oldest = state.autoQueue.keys().next().value;
                    if (!oldest) break;
                    const dropped = state.autoQueue.get(oldest);
                    if (dropped && dropped.article) delete dropped.article.dataset.dqhQueued;
                    state.autoQueue.delete(oldest);
                }
            }
            ensureComposerButtons();
        }

        state.drain = drain;
        state.deliver = deliver;
        state.scan = scan;

        const observer = new MutationObserver(() => scheduleScan());
        observer.observe(document.body, { childList: true, subtree: true });
        state.observer = observer;

        scan();
        return JSON.stringify({ ready: true, cached: false });
    } catch (error) {
        return JSON.stringify({ ready: false, error: String(error) });
    }
})()
"###;

fn js_init(config: &TranslationConfig) -> Result<String> {
    let config_json =
        serde_json::to_string(config).context("Could not serialize translation config")?;
    Ok(JS_INIT_TRANSLATE
        .replace("__DQH_VERSION__", &RESIDENT_SCRIPT_VERSION.to_string())
        .replace("__DQH_CONFIG_JSON__", &embed_json(&config_json)))
}

const JS_DRAIN: &str = r###"
(() => {
    const state = window.__dqh_translate;
    if (!state || !state.initialized || typeof state.drain !== "function") {
        return JSON.stringify({ ready: false, items: [] });
    }
    return state.drain();
})()
"###;

const JS_TEARDOWN: &str = r###"
(() => {
    const state = window.__dqh_translate;
    if (state && state.observer) {
        try { state.observer.disconnect(); } catch (error) {}
    }
    document.getElementById("dqh-translate-styles") && document.getElementById("dqh-translate-styles").remove();
    document.getElementById("dqh-summary-panel") && document.getElementById("dqh-summary-panel").remove();
    document.querySelectorAll(".dqh-action-row, .dqh-divider, .dqh-toast").forEach(element => element.remove());
    document.querySelectorAll("[data-dqh-translation]").forEach(element => element.remove());
    document.querySelectorAll("[data-dqh-original]").forEach(wrapper => {
        const parent = wrapper.parentNode;
        if (!parent) return;
        while (wrapper.firstChild) parent.insertBefore(wrapper.firstChild, wrapper);
        wrapper.remove();
    });
    delete window.__dqh_translate;
    return JSON.stringify({ ok: true });
})()
"###;

fn js_deliver(results_json: &str) -> String {
    let payload = embed_json(results_json);
    format!(
        "(() => {{ const state = window.__dqh_translate; if (!state || typeof state.deliver !== \"function\") return JSON.stringify({{ applied: 0 }}); return state.deliver({payload}); }})()"
    )
}

/// Embeds a JSON payload as a JS expression literal. JSON is valid JS except
/// for the U+2028/U+2029 line separators, which must be escaped.
fn embed_json(json_text: &str) -> String {
    json_text
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

// ---------------------------------------------------------------------------
// Rust side: runtime, ticker, commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationStatus {
    pub enabled: bool,
    pub translated: u64,
    pub cache_hits: u64,
    pub errors: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Default)]
pub struct StatsCounters {
    translated: AtomicU64,
    cache_hits: AtomicU64,
    errors: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl StatsCounters {
    fn status(&self, enabled: bool) -> TranslationStatus {
        TranslationStatus {
            enabled,
            translated: self.translated.load(Ordering::SeqCst),
            cache_hits: self.cache_hits.load(Ordering::SeqCst),
            errors: self.errors.load(Ordering::SeqCst),
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    fn record_success(&self, cache_hit: bool) {
        self.translated.fetch_add(1, Ordering::SeqCst);
        if cache_hit {
            self.cache_hits.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn record_error(&self, message: &str) {
        self.errors.fetch_add(1, Ordering::SeqCst);
        let mut last_error = self
            .last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last_error.as_deref() != Some(message) {
            *last_error = Some(message.to_string());
        }
    }
}

#[derive(Default)]
pub struct TranslationRuntimeState {
    cancel: Option<Arc<AtomicBool>>,
    handle: Option<JoinHandle<()>>,
    counters: Arc<StatsCounters>,
    config: Option<TranslationConfig>,
}

impl TranslationRuntimeState {
    fn status(&self) -> TranslationStatus {
        self.counters.status(
            self.handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished()),
        )
    }

    fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        self.config = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestProviderKind {
    /// Translate a sample string through the message provider.
    Message,
    /// Health-check the configured LLM endpoint.
    Llm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderRequest {
    pub kind: TestProviderKind,
    pub provider: TranslationProvider,
    pub target_language: String,
    pub microsoft_region: Option<String>,
    pub llm: Option<crate::translation::LlmConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DrainResponse {
    ready: bool,
    #[serde(default)]
    items: Vec<TranslationJob>,
}

/// Validates and normalizes a start config; never trusts frontend input.
pub fn sanitize_config(config: &TranslationConfig) -> TranslationConfig {
    let mut sanitized = config.clone();
    let color = sanitized.accent_color.trim();
    let is_css_hex = color.starts_with('#')
        && color.len() <= 9
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !is_css_hex {
        sanitized.accent_color = DEFAULT_ACCENT_COLOR.to_string();
    }
    sanitized.summary_count = sanitized.summary_count.clamp(1, 100);
    if sanitized.target_language.trim().is_empty() {
        sanitized.target_language = "en".to_string();
    }
    sanitized
}

fn map_key_ref(key_ref: &str) -> Result<crate::translation::ProviderKeyRef> {
    match key_ref {
        "microsoft" => Ok(crate::translation::ProviderKeyRef::Microsoft),
        "deepl" => Ok(crate::translation::ProviderKeyRef::DeepL),
        "llm" => Ok(crate::translation::ProviderKeyRef::Llm),
        other => Err(anyhow!("Unknown provider key reference: {}", other)),
    }
}

#[tauri::command]
pub async fn start_message_translation(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    config: TranslationConfig,
) -> Result<TranslationStatus, String> {
    use crate::translation::TranslationCache;

    let config = sanitize_config(&config);
    if let Some(llm) = &config.llm {
        validate_llm_base_url(&llm.base_url).map_err(|error| error.to_string())?;
    }
    let cache_path: PathBuf = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?
        .join(crate::translation::TRANSLATION_CACHE_FILE);
    let cache = TranslationCache::new(cache_path);

    let runtime = {
        let mut runtime = state.translation.lock().await;
        runtime.stop();
        let cancel = Arc::new(AtomicBool::new(false));
        let counters = runtime.counters.clone();
        let ticker_config = config.clone();
        let ticker_cancel = cancel.clone();
        let app_handle = app.clone();
        let handle = tokio::spawn(async move {
            translation_ticker(app_handle, ticker_config, ticker_cancel, counters, cache).await;
        });
        runtime.cancel = Some(cancel);
        runtime.handle = Some(handle);
        runtime.config = Some(config);
        runtime.status()
    };

    log(
        LogLevel::Info,
        LogCategory::Translation,
        "Message translation started",
        None,
    );
    let _ = app.emit("translation-status", &runtime);
    Ok(runtime)
}

#[tauri::command]
pub async fn stop_message_translation(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<TranslationStatus, String> {
    let status = {
        let mut runtime = state.translation.lock().await;
        runtime.stop();
        runtime.status()
    };
    teardown_discord_script().await;
    log(
        LogLevel::Info,
        LogCategory::Translation,
        "Message translation stopped",
        None,
    );
    let _ = app.emit("translation-status", &status);
    Ok(status)
}

#[tauri::command]
pub async fn get_translation_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<TranslationStatus, String> {
    Ok(state.translation.lock().await.status())
}

#[tauri::command]
pub async fn set_translation_provider_key(
    app: tauri::AppHandle,
    key_ref: String,
    api_key: String,
) -> Result<(), String> {
    let key_ref = map_key_ref(&key_ref).map_err(|error| error.to_string())?;
    let app_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    crate::translation::set_provider_key(key_ref, api_key.trim(), &app_dir)
        .map(|_storage| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_translation_provider_key(
    app: tauri::AppHandle,
    key_ref: String,
) -> Result<(), String> {
    let key_ref = map_key_ref(&key_ref).map_err(|error| error.to_string())?;
    let app_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    crate::translation::clear_provider_key(key_ref, &app_dir);
    Ok(())
}

#[tauri::command]
pub async fn is_translation_provider_key_set(
    app: tauri::AppHandle,
    key_ref: String,
) -> Result<bool, String> {
    let key_ref = map_key_ref(&key_ref).map_err(|error| error.to_string())?;
    let app_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    Ok(crate::translation::has_provider_key(key_ref, &app_dir))
}

#[tauri::command]
pub async fn test_translation_provider(
    app: tauri::AppHandle,
    request: TestProviderRequest,
) -> Result<String, String> {
    let app_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    let http = TranslationHttp::new().map_err(|error| error.to_string())?;
    let keys = resolve_keys(
        &TranslationConfig {
            provider: request.provider,
            target_language: request.target_language.clone(),
            auto_translate: false,
            display_mode: crate::translation::TranslationDisplayMode::Below,
            accent_color: String::new(),
            summary_count: 20,
            microsoft_region: request.microsoft_region.clone(),
            llm: request.llm.clone(),
            labels: Default::default(),
        },
        &app_dir,
    );
    let result: Result<String> = match request.kind {
        TestProviderKind::Message => {
            http.translate_markdown_aware(
                request.provider,
                "Hello, world!",
                &request.target_language,
                &keys.credentials,
            )
            .await
        }
        TestProviderKind::Llm => {
            let llm = request
                .llm
                .ok_or_else(|| "LLM endpoint is not configured".to_string())?;
            http.llm_complete(
                &llm,
                &keys.llm_key,
                "You are a health check.",
                "Reply with the word: ready",
            )
            .await
        }
    };
    result.map_err(|error| error.to_string())
}

async fn teardown_discord_script() {
    if let Ok(port) = cdp_port() {
        let _ = cdp_client::execute_js_via_primary_discord_target(
            port,
            JS_TEARDOWN,
            true,
            TICK_EVAL_TIMEOUT_SECS,
        )
        .await;
    }
}

fn cdp_port() -> Result<u16> {
    Ok(cdp_client::DEFAULT_CDP_PORT)
}

async fn translation_ticker(
    app: tauri::AppHandle,
    config: TranslationConfig,
    cancel: Arc<AtomicBool>,
    counters: Arc<StatsCounters>,
    cache: crate::translation::TranslationCache,
) {
    let http = match TranslationHttp::new() {
        Ok(http) => http,
        Err(error) => {
            counters.record_error(&error.to_string());
            log(
                LogLevel::Error,
                LogCategory::Translation,
                &format!("Translation ticker could not start: {error:#}"),
                None,
            );
            return;
        }
    };
    let app_dir = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let port = match cdp_port() {
        Ok(port) => port,
        Err(_) => return,
    };
    let mut consecutive_failures: u32 = 0;

    while !cancel.load(Ordering::SeqCst) {
        if let Err(error) = tick(&app, &http, &cache, &config, &counters, port, &app_dir).await {
            consecutive_failures += 1;
            let message = format!("{error:#}");
            counters.record_error(&message);
            let report = consecutive_failures == 1 || consecutive_failures.is_multiple_of(40);
            if report {
                log(
                    LogLevel::Warn,
                    LogCategory::Translation,
                    &format!(
                        "Translation tick failed ({consecutive_failures} consecutive): {message}"
                    ),
                    None,
                );
                let _ = app.emit("translation-error", json!({ "message": message }));
            }
        } else {
            consecutive_failures = 0;
        }
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        sleep(Duration::from_millis(TICK_INTERVAL_MS)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn tick(
    app: &tauri::AppHandle,
    http: &TranslationHttp,
    cache: &TranslationCache,
    config: &TranslationConfig,
    counters: &Arc<StatsCounters>,
    port: u16,
    app_dir: &std::path::Path,
) -> Result<usize> {
    let drain_raw = cdp_client::execute_js_via_primary_discord_target(
        port,
        JS_DRAIN,
        true,
        TICK_EVAL_TIMEOUT_SECS,
    )
    .await?;
    let mut drain: DrainResponse = serde_json::from_str(drain_raw.trim())
        .with_context(|| format!("Invalid drain response: {drain_raw:.200}"))?;
    if !drain.ready {
        // Resident script missing (Discord reloaded): reinstall and retry once.
        let init = js_init(config)?;
        cdp_client::execute_js_via_primary_discord_target(
            port,
            &init,
            true,
            TICK_EVAL_TIMEOUT_SECS,
        )
        .await?;
        let retry_raw = cdp_client::execute_js_via_primary_discord_target(
            port,
            JS_DRAIN,
            true,
            TICK_EVAL_TIMEOUT_SECS,
        )
        .await?;
        drain = serde_json::from_str(retry_raw.trim())
            .with_context(|| format!("Invalid drain response after re-init: {retry_raw:.200}"))?;
    }
    if drain.items.is_empty() {
        return Ok(0);
    }

    let keys = resolve_keys(config, app_dir);
    let mut results: Vec<TranslationJobResult> = Vec::new();
    for job in drain.items.into_iter().take(MAX_DRAIN_ITEMS) {
        results.push(process_job(app, http, cache, config, counters, &keys, job).await);
    }

    let deliver_script = js_deliver(&serde_json::to_string(&results)?);
    cdp_client::execute_js_via_primary_discord_target(
        port,
        &deliver_script,
        true,
        TICK_EVAL_TIMEOUT_SECS,
    )
    .await?;
    Ok(results.len())
}

async fn process_job(
    app: &tauri::AppHandle,
    http: &TranslationHttp,
    cache: &TranslationCache,
    config: &TranslationConfig,
    counters: &Arc<StatsCounters>,
    keys: &crate::translation::ResolvedKeys,
    job: TranslationJob,
) -> TranslationJobResult {
    let kind = job.kind;
    let outcome = process_job_inner(app, http, cache, config, keys, &job).await;
    match outcome {
        Ok((text, cache_key, cache_hit)) => {
            counters.record_success(cache_hit);
            TranslationJobResult {
                id: job.id.clone(),
                ok: true,
                text: Some(text),
                error: None,
                cache_key,
                cache_hit: if cache_hit { Some(true) } else { None },
                kind: Some(kind),
                message_count: job.message_count,
                channel_name: job.channel_name,
            }
        }
        Err(error) => {
            let message = format!("{error:#}");
            counters.record_error(&message);
            log(
                LogLevel::Error,
                LogCategory::Translation,
                &format!("Translation job failed ({}): {message}", kind_debug(kind)),
                None,
            );
            TranslationJobResult {
                id: job.id.clone(),
                ok: false,
                text: None,
                error: Some(message),
                cache_key: None,
                cache_hit: None,
                kind: Some(kind),
                message_count: job.message_count,
                channel_name: job.channel_name,
            }
        }
    }
}

fn kind_debug(kind: TranslationJobKind) -> &'static str {
    match kind {
        TranslationJobKind::Message => "message",
        TranslationJobKind::Composer => "composer",
        TranslationJobKind::Summary => "summary",
    }
}

type JobOutcome = (String, Option<String>, bool);

async fn process_job_inner(
    app: &tauri::AppHandle,
    http: &TranslationHttp,
    cache: &TranslationCache,
    config: &TranslationConfig,
    keys: &crate::translation::ResolvedKeys,
    job: &TranslationJob,
) -> Result<JobOutcome> {
    match job.kind {
        TranslationJobKind::Message => {
            let (text, cache_key, cache_hit) = cache
                .translate_with_cache(
                    http,
                    config.provider,
                    &job.text,
                    &job.target_language,
                    &job.stable_id,
                    &keys.credentials,
                )
                .await?;
            Ok((text, Some(cache_key), cache_hit))
        }
        TranslationJobKind::Composer => {
            let llm = config
                .llm
                .as_ref()
                .ok_or_else(|| anyhow!("LLM endpoint is not configured"))?;
            let text = http
                .llm_complete(
                    llm,
                    &keys.llm_key,
                    &composer_translation_system_prompt(),
                    &composer_translation_user_prompt(
                        &job.text,
                        &job.target_language,
                        job.channel_name.as_deref(),
                        job.channel_topic.as_deref(),
                        job.recent_messages.as_deref(),
                    ),
                )
                .await?;
            Ok((text, None, false))
        }
        TranslationJobKind::Summary => {
            let llm = config
                .llm
                .as_ref()
                .ok_or_else(|| anyhow!("LLM endpoint is not configured"))?;
            let channel_id = job
                .channel_id
                .clone()
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| anyhow!("Summary job has no channel id"))?;
            let count = job.message_count.unwrap_or(config.summary_count);
            let messages = fetch_channel_messages(app, &channel_id, count).await?;
            if messages.is_empty() {
                return Err(anyhow!("No channel messages were available to summarize"));
            }
            let transcript = messages
                .iter()
                .map(|message| format!("{}: {}", message.author, message.content))
                .collect::<Vec<_>>()
                .join("\n");
            let text = http
                .llm_complete(
                    llm,
                    &keys.llm_key,
                    &channel_summary_system_prompt(),
                    &channel_summary_user_prompt(
                        &transcript,
                        count,
                        job.channel_name.as_deref(),
                        job.channel_topic.as_deref(),
                    ),
                )
                .await?;
            Ok((text, None, false))
        }
    }
}

async fn fetch_channel_messages(
    app: &tauri::AppHandle,
    channel_id: &str,
    limit: u32,
) -> Result<Vec<crate::discord_api::ChannelMessage>> {
    let state = app.state::<crate::AppState>();
    let client: DiscordApiClient = {
        let guard = state
            .client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("Not logged in; channel summary requires a Discord session"))?
    };
    client.get_channel_messages(channel_id, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::{TranslationLabels, TranslationProvider};

    fn sample_config() -> TranslationConfig {
        TranslationConfig {
            provider: TranslationProvider::GoogleTranslate,
            target_language: "pt-BR".to_string(),
            auto_translate: true,
            display_mode: crate::translation::TranslationDisplayMode::Below,
            accent_color: "#ff00aa".to_string(),
            summary_count: 50,
            microsoft_region: None,
            llm: None,
            labels: TranslationLabels::default(),
        }
    }

    #[test]
    fn sanitize_config_clamps_and_defaults() {
        let mut config = sample_config();
        config.summary_count = 999;
        config.accent_color = "javascript:alert(1)".to_string();
        config.target_language = "  ".to_string();
        let sanitized = sanitize_config(&config);
        assert_eq!(sanitized.summary_count, 100);
        assert_eq!(sanitized.accent_color, DEFAULT_ACCENT_COLOR);
        assert_eq!(sanitized.target_language, "en");

        let mut config = sample_config();
        config.summary_count = 0;
        config.accent_color = "#abc".to_string();
        let sanitized = sanitize_config(&config);
        assert_eq!(sanitized.summary_count, 1);
        assert_eq!(sanitized.accent_color, "#abc");
    }

    #[test]
    fn embed_json_escapes_line_separators() {
        let embedded = embed_json("{\"a\":\"x\u{2028}y\u{2029}z\"}");
        assert!(embedded.contains("\\u2028"));
        assert!(embedded.contains("\\u2029"));
        assert!(!embedded.contains('\u{2028}'));
    }

    #[test]
    fn js_init_embeds_config_and_version() {
        let script = js_init(&sample_config()).unwrap();
        assert!(script.contains("\"provider\":\"google-translate\""));
        assert!(!script.contains("__DQH_CONFIG_JSON__"));
        assert!(!script.contains("__DQH_VERSION__"));
    }

    #[test]
    fn js_deliver_embeds_results_payload() {
        let script = js_deliver("[{\"id\":\"m1\",\"ok\":true,\"text\":\"oi\"}]");
        assert!(script.contains("{\"id\":\"m1\",\"ok\":true,\"text\":\"oi\"}"));
        assert!(!script.contains("__DQH_RESULTS__"));
    }

    #[test]
    fn drain_response_parses_missing_optionals() {
        let drain: DrainResponse =
            serde_json::from_str("{\"ready\":true,\"items\":[{\"id\":\"a1\",\"kind\":\"message\",\"text\":\"hi\",\"stableId\":\"chat-messages-1-2\",\"targetLanguage\":\"en\"}]}").unwrap();
        assert!(drain.ready);
        assert_eq!(drain.items.len(), 1);
        assert_eq!(drain.items[0].kind, TranslationJobKind::Message);
        assert!(drain.items[0].channel_id.is_none());
    }

    #[test]
    fn key_ref_mapping_rejects_unknown() {
        assert!(map_key_ref("microsoft").is_ok());
        assert!(map_key_ref("llm").is_ok());
        assert!(map_key_ref("nope").is_err());
    }
}
