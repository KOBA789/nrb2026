import { useEffect, useState } from "react";
import { apiGet, apiPostNoBody } from "../api";

// API には GET /saved_searches が無い (idea.md)。本画面は「作成」のみ。
// 上限 10 件で 409、削除 API なしも仕様。

export function SavedSearches() {
  const [allTags, setAllTags] = useState<string[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    apiGet<string[]>("/tags").then(setAllTags).catch((e) => setErr(String(e)));
  }, []);

  const toggleTag = (t: string) => {
    setTags((cur) => {
      if (cur.includes(t)) return cur.filter((x) => x !== t);
      if (cur.length >= 3) return cur;
      return [...cur, t];
    });
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (tags.length === 0 || tags.length > 3) {
      setErr("タグを 1〜3 個選んでください");
      return;
    }
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      // POST /saved_searches は 201 Created with empty body
      await apiPostNoBody<{ tags: string[] }>("/saved_searches", { tags });
      setInfo("作成しました");
      setTags([]);
    } catch (e2) {
      setErr(String(e2));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit}>
      <h2>保存された検索条件 (作成)</h2>
      <p className="muted">
        条件にマッチする campaign が残り 1 人で募集終了となったとき通知が届きます。
        ユーザー 1 人につき最大 10 件、削除 API はありません。
      </p>
      <fieldset>
        <legend>tags (1〜3, AND)</legend>
        {allTags.length === 0 ? (
          <p className="muted">(読み込み中)</p>
        ) : (
          allTags.map((t) => (
            <label key={t} style={{ display: "inline-block", marginRight: "0.5rem" }}>
              <input
                type="checkbox"
                checked={tags.includes(t)}
                onChange={() => toggleTag(t)}
                disabled={!tags.includes(t) && tags.length >= 3}
              />{" "}
              {t}
            </label>
          ))
        )}
      </fieldset>
      <p>
        <button type="submit" disabled={busy}>
          {busy ? "送信中…" : "作成"}
        </button>
      </p>
      {info && <p>{info}</p>}
      {err && <p className="error">{err}</p>}
    </form>
  );
}
