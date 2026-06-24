import { ref, computed } from 'vue'
import { defineStore } from 'pinia'

export interface ReleaseInfo {
    tag_name: string
    html_url: string
    published_at: string
    name: string
}

/**
 * Parse a version string like "0.9.0-rc1" into its numeric components [0, 9, 0].
 * Pre-release suffixes (e.g. -rc1, -beta2) are stripped before parsing.
 */
function parseSemver(version: string): [number, number, number] {
    const cleaned = version.replace(/^v/, '').split('-')[0]
    const parts = cleaned.split('.').map(Number)
    return [parts[0] || 0, parts[1] || 0, parts[2] || 0]
}

/**
 * Compare two semver strings. Returns -1 / 0 / 1.
 */
function compareSemver(a: string, b: string): -1 | 0 | 1 {
    const [a0, a1, a2] = parseSemver(a)
    const [b0, b1, b2] = parseSemver(b)
    if (a0 !== b0) return a0 < b0 ? -1 : 1
    if (a1 !== b1) return a1 < b1 ? -1 : 1
    if (a2 !== b2) return a2 < b2 ? -1 : 1
    return 0
}

export const useVersionStore = defineStore('version', () => {
    const currentVersion = ref<string>('Dev')
    const latestRelease = ref<ReleaseInfo | null>(null)
    const checkError = ref<string | null>(null)
    const isChecking = ref(false)
    const hasChecked = ref(false)
    const checkPreRelease = ref(localStorage.getItem('checkPreRelease') === 'true')

    const isPreRelease = computed(() =>
        currentVersion.value !== 'Dev' && currentVersion.value.includes('rc'),
    )

    const hasUpdate = computed(() => {
        if (!latestRelease.value || currentVersion.value === 'Dev') return false
        const current = currentVersion.value.replace(/^v/, '')
        const latest = latestRelease.value.tag_name.replace(/^v/, '')
        return compareSemver(latest, current) > 0
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
        // Force pre-release update checks when running an RC build,
        // without persisting the user's original setting.
        if (isPreRelease.value) {
            checkPreRelease.value = true
        }
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
        isPreRelease,
        checkPreRelease,
        loadCurrentVersion,
        checkForUpdate,
        setCheckPreRelease,
        initialize
    }
})
