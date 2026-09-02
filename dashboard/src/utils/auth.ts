// H-4: Avoid compiling or embedding shared secrets into public client bundles.
// Operators can enter the API key in the UI at runtime, which is stored in
// sessionStorage (or localStorage) and never baked into static build artifacts.

export function getApiKey(): string {
  if (typeof window !== 'undefined') {
    const sessionKey = window.sessionStorage.getItem('kingfisher_api_key')
    if (sessionKey) return sessionKey
    const localKey = window.localStorage.getItem('kingfisher_api_key')
    if (localKey) return localKey
  }
  return import.meta.env.VITE_API_KEY ?? ''
}

export function setApiKey(key: string, persist = false): void {
  if (typeof window !== 'undefined') {
    if (key.trim().length === 0) {
      window.sessionStorage.removeItem('kingfisher_api_key')
      window.localStorage.removeItem('kingfisher_api_key')
    } else if (persist) {
      window.localStorage.setItem('kingfisher_api_key', key.trim())
    } else {
      window.sessionStorage.setItem('kingfisher_api_key', key.trim())
    }
  }
}
