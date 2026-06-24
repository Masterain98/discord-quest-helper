import { ref, computed } from 'vue'
import { defineStore } from 'pinia'

export interface ReleaseInfo {
    tag_name: string
    html_url: string
    published_at: string
    name: string
}

export const useVersionStore = defineStore('version', () => {
    const currentVersion = ref<string>('Dev')
    const latestRelease = ref<ReleaseInfo | null>(null)
    const checkError = ref<string | null>(null)
    const isChecking = ref(false)
    const hasChecked = ref(false)
    const checkPreRelease = ref(localStorage.getItem('checkPreRelease') === 'true')

    const hasUpdate = computed(() => {
        if (!latestRelease.value || currentVersion.value === 'Dev') return false
        // Compare versions (strip 'v' prefix if present)
        const current = currentVersion.value.replace(/^v/, '')
        const latest = latestRelease.value.tag_name.replace(/^v/, '')
        return latest !== current
    })

    const isLatest = computed(() => {
        return hasChecked.value && !hasUpdate.value && !checkError.value
    })

    async function loadCurrentVersion() {
        try {
            const res = await fetch('/version.txt')
            if (res.ok) {
                const text = await res.text()
                if (text) {
                    currentVersion.value = text.trim()
                }
            }
        } catch {
            // Keep 'Dev' as default
        }
    }

    async function checkForUpdate() {
        if (isChecking.value) return

        isChecking.value = true
        checkError.value = null

        try {
            const url = checkPreRelease.value
              ? 'https://api.github.com/repos/Masterain98/discord-quest-helper/releases'
              : 'https://api.github.com/repos/Masterain98/discord-quest-helper/releases/latest'

            const res = await fetch(url, {
                headers: {
                    'Accept': 'application/vnd.github.v3+json'
                }
            })

            if (!res.ok) {
                throw new Error(`GitHub API returned ${res.status}`)
            }

            const data = await res.json()

            if (checkPreRelease.value) {
                // Array of releases — pick the first one (newest, including pre-releases)
                const release = Array.isArray(data) ? data[0] : data
                if (!release) throw new Error('No releases found')
                latestRelease.value = {
                    tag_name: release.tag_name,
                    html_url: release.html_url,
                    published_at: release.published_at,
                    name: release.name
                }
            } else {
                latestRelease.value = {
                    tag_name: data.tag_name,
                    html_url: data.html_url,
                    published_at: data.published_at,
                    name: data.name
                }
            }
            hasChecked.value = true
        } catch (e) {
            checkError.value = e instanceof Error ? e.message : 'Failed to check for updates'
            console.error('Version check failed:', e)
        } finally {
            isChecking.value = false
        }
    }

    function setCheckPreRelease(value: boolean) {
        checkPreRelease.value = value
        localStorage.setItem('checkPreRelease', String(value))
        hasChecked.value = false
        latestRelease.value = null
        checkForUpdate()
    }

    async function initialize() {
        await loadCurrentVersion()
        await checkForUpdate()
    }

    return {
        currentVersion,
        latestRelease,
        checkError,
        isChecking,
        hasChecked,
        hasUpdate,
        isLatest,
        checkPreRelease,
        loadCurrentVersion,
        checkForUpdate,
        setCheckPreRelease,
        initialize
    }
})
