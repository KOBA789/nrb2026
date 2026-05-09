// localStorage に保持する user_id (= X-User-ID ヘッダ値)。
// 補助 frontend なので 1 タブ想定。複数タブ間同期はしない。
const KEY = "nrb2026.user_id";

export function getUserId(): string | null {
  return localStorage.getItem(KEY);
}

export function setUserId(id: string): void {
  localStorage.setItem(KEY, id);
}

export function clearUserId(): void {
  localStorage.removeItem(KEY);
}
