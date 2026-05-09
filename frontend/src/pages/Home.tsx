import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { apiGet } from "../api";
import { CampaignImage } from "../components/CampaignImage";
import type { Campaign } from "../types";

type Sort = "new" | "active";

export function Home() {
  const [params, setParams] = useSearchParams();
  const sort: Sort = params.get("sort") === "active" ? "active" : "new";
  const tagsParam = params.get("tags") ?? "";

  const [allTags, setAllTags] = useState<string[]>([]);
  const [campaigns, setCampaigns] = useState<Campaign[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  // GET /tags は 1 度だけ
  useEffect(() => {
    apiGet<string[]>("/tags").then(setAllTags).catch((e) => setErr(String(e)));
  }, []);

  // GET /campaigns は param 変更ごとに naive に都度 fetch (キャッシュなし)
  useEffect(() => {
    setCampaigns(null);
    setErr(null);
    const qs = new URLSearchParams();
    qs.set("sort", sort);
    if (tagsParam) qs.set("tags", tagsParam);
    apiGet<Campaign[]>(`/campaigns?${qs.toString()}`)
      .then(setCampaigns)
      .catch((e) => setErr(String(e)));
  }, [sort, tagsParam]);

  // URL 直接編集で重複や 4 個以上が入ってきても、以後の操作で正規化する (dedup + 先頭 3 個)。
  const selected = (() => {
    const seen = new Set<string>();
    const acc: string[] = [];
    for (const t of tagsParam ? tagsParam.split(",") : []) {
      if (t && !seen.has(t) && acc.length < 3) {
        seen.add(t);
        acc.push(t);
      }
    }
    return new Set(acc);
  })();
  const toggleTag = (t: string) => {
    const next = new Set(selected);
    if (next.has(t)) next.delete(t);
    else if (next.size < 3) next.add(t);
    const q = new URLSearchParams(params);
    if (next.size === 0) q.delete("tags");
    else q.set("tags", [...next].join(","));
    setParams(q, { replace: true });
  };
  const setSort = (s: Sort) => {
    const q = new URLSearchParams(params);
    q.set("sort", s);
    setParams(q, { replace: true });
  };

  return (
    <div>
      <h2>キャンペーン一覧</h2>
      <fieldset>
        <legend>並び順</legend>
        <label>
          <input type="radio" checked={sort === "new"} onChange={() => setSort("new")} /> new
        </label>
        <label>
          <input type="radio" checked={sort === "active"} onChange={() => setSort("active")} /> active
        </label>
      </fieldset>
      <fieldset>
        <legend>タグ (AND, 最大 3)</legend>
        {allTags.length === 0 ? (
          <p className="muted">(タグ読み込み中)</p>
        ) : (
          <div>
            {allTags.map((t) => (
              <label key={t} style={{ display: "inline-block", marginRight: "0.5rem" }}>
                <input
                  type="checkbox"
                  checked={selected.has(t)}
                  onChange={() => toggleTag(t)}
                  disabled={!selected.has(t) && selected.size >= 3}
                />{" "}
                {t}
              </label>
            ))}
          </div>
        )}
      </fieldset>
      {err && <p className="error">{err}</p>}
      {campaigns === null ? (
        <p className="muted">読み込み中…</p>
      ) : campaigns.length === 0 ? (
        <p className="muted">該当するキャンペーンはありません</p>
      ) : (
        <ul className="campaign-list">
          {campaigns.map((c) => (
            <li key={c.id}>
              <CampaignImage id={c.id} className="campaign-thumb" />
              <div className="campaign-body">
                <div>
                  <Link to={`/campaigns/${c.id}`}>
                    <strong>{c.name}</strong>
                  </Link>{" "}
                  <span className="muted">
                    {c.current_count}/{c.goal_count} · ¥{c.price.toLocaleString()} ·{" "}
                    [{c.tags.join(", ")}]
                  </span>
                </div>
                <div className="muted">{c.description}</div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
