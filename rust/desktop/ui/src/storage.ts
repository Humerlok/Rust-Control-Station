export function loadSetting<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function saveSetting<T>(key: string, value: T): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // storage unavailable (private mode, blocked): settings simply do not persist
  }
}
