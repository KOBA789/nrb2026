// webapp API ラッパ。
//
// 規律:
//   * caller は API prefix 抜きの resource path (`/me` `/campaigns/...`) を渡す。
//     `/api` prefix は `request()` 内で apiUrl() が付与する (二重 prefix / 相対 path は早期 throw)。
//   * X-User-ID は localStorage から自動付与。
//   * !res.ok は console.error して throw。各画面でエラー UI は最低限 (CLAUDE.md API first)。

import { getUserId } from "./auth";

const API_PREFIX = "/api";
// `/api`, `/api/...`, `/api?x=1`, `/api#x` を全部拒否するための境界 regex。
const API_PREFIX_RE = /^\/api(?:[/?#]|$)/;

export class ApiError extends Error {
  status: number;
  constructor(status: number, msg: string) {
    super(msg);
    this.status = status;
  }
}

function apiUrl(path: string): string {
  if (!path.startsWith("/")) {
    throw new Error(`API path must start with "/": ${path}`);
  }
  if (API_PREFIX_RE.test(path)) {
    throw new Error(`API path must not include ${API_PREFIX}: ${path}`);
  }
  return `${API_PREFIX}${path}`;
}

function buildHeaders(init: RequestInit | undefined, hasJsonBody: boolean): Headers {
  const h = new Headers(init?.headers);
  if (hasJsonBody && !h.has("Content-Type")) {
    h.set("Content-Type", "application/json");
  }
  const userId = getUserId();
  if (userId && !h.has("X-User-ID")) {
    h.set("X-User-ID", userId);
  }
  return h;
}

async function request(path: string, init: RequestInit = {}): Promise<Response> {
  const url = apiUrl(path);
  const hasBody = init.body !== undefined && init.body !== null;
  const res = await fetch(url, { ...init, headers: buildHeaders(init, hasBody) });
  if (!res.ok) {
    console.error("API error", init.method ?? "GET", url, res.status);
    throw new ApiError(res.status, `${init.method ?? "GET"} ${url} -> ${res.status}`);
  }
  return res;
}

/** GET → JSON */
export async function apiGet<T>(path: string): Promise<T> {
  const res = await request(path, { method: "GET" });
  return (await res.json()) as T;
}

/** POST JSON → JSON (response が空 body のケースは apiPostNoBody を使う) */
export async function apiPostJson<TReq, TRes>(path: string, body: TReq): Promise<TRes> {
  const res = await request(path, { method: "POST", body: JSON.stringify(body) });
  return (await res.json()) as TRes;
}

/** POST JSON → 空 body (例: POST /saved_searches は 201 Created with empty body) */
export async function apiPostNoBody<TReq>(path: string, body: TReq): Promise<void> {
  await request(path, { method: "POST", body: JSON.stringify(body) });
}

/** GET → Blob (画像取得用)。`/campaigns/:id/image` は認証必須なので <img src> ではなく fetch 経由。 */
export async function apiGetBlob(path: string): Promise<Blob> {
  const res = await request(path, { method: "GET" });
  return await res.blob();
}
