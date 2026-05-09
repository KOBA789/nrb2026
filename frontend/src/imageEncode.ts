// File → base64 (data URL の prefix を削除した純 base64 文字列)。
// `btoa(String.fromCharCode(...))` は spread の引数数上限に引っかかるので readAsDataURL 経由。
// webapp 側は decode 後 ≤ 200 KiB / JPEG magic 必須 (docs/idea.md)。size check は事前に弾く。

export const MAX_IMAGE_BYTES = 204_800; // 200 KiB

export async function jpegFileToBase64(file: File): Promise<string> {
  if (file.size === 0) throw new Error("画像が空です");
  if (file.size > MAX_IMAGE_BYTES) {
    throw new Error(`画像は ${MAX_IMAGE_BYTES} byte 以下にしてください (現在: ${file.size} byte)`);
  }
  const dataUrl = await new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error ?? new Error("FileReader error"));
    r.readAsDataURL(file);
  });
  const comma = dataUrl.indexOf(",");
  if (comma < 0) throw new Error("invalid data URL");
  return dataUrl.slice(comma + 1);
}
