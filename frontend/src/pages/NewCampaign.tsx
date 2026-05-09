import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { apiGet, apiPostJson } from "../api";
import { jpegFileToBase64, MAX_IMAGE_BYTES } from "../imageEncode";
import type { Campaign } from "../types";

interface Req {
  name: string;
  description: string;
  price: number;
  goal_count: number;
  tags: string[];
  image: string;
}

export function NewCampaign() {
  const navigate = useNavigate();
  const [allTags, setAllTags] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [price, setPrice] = useState(2000);
  const [goalCount, setGoalCount] = useState(2);
  const [tags, setTags] = useState<string[]>([]);
  const [file, setFile] = useState<File | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    apiGet<string[]>("/tags").then(setAllTags).catch((e) => setErr(String(e)));
  }, []);

  const toggleTag = (t: string) => {
    setTags((cur) => {
      if (cur.includes(t)) return cur.filter((x) => x !== t);
      if (cur.length >= 10) return cur;
      return [...cur, t];
    });
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!file) {
      setErr("画像 (JPEG) を選択してください");
      return;
    }
    if (tags.length > 10) {
      setErr("タグは最大 10 個までです");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const image = await jpegFileToBase64(file);
      const req: Req = {
        name,
        description,
        price,
        goal_count: goalCount,
        tags,
        image,
      };
      const c = await apiPostJson<Req, Campaign>("/campaigns", req);
      navigate(`/campaigns/${c.id}`, { replace: true });
    } catch (e2) {
      setErr(String(e2));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit}>
      <h2>新規キャンペーン</h2>
      <label>
        name (1〜100):
        <input
          type="text"
          maxLength={100}
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </label>
      <label>
        description (1〜1000):
        <textarea
          maxLength={1000}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          required
        />
      </label>
      <label>
        price (2000〜20000):
        <input
          type="number"
          min={2000}
          max={20000}
          value={price}
          onChange={(e) => setPrice(Number(e.target.value))}
          required
        />
      </label>
      <label>
        goal_count (2〜20):
        <input
          type="number"
          min={2}
          max={20}
          value={goalCount}
          onChange={(e) => setGoalCount(Number(e.target.value))}
          required
        />
      </label>
      <fieldset>
        <legend>tags (最大 10、既存タグから選択)</legend>
        {allTags.length === 0 ? (
          <p className="muted">(読み込み中)</p>
        ) : (
          allTags.map((t) => (
            <label key={t} style={{ display: "inline-block", marginRight: "0.5rem" }}>
              <input
                type="checkbox"
                checked={tags.includes(t)}
                onChange={() => toggleTag(t)}
                disabled={!tags.includes(t) && tags.length >= 10}
              />{" "}
              {t}
            </label>
          ))
        )}
      </fieldset>
      <label>
        image (JPEG, ≤ {MAX_IMAGE_BYTES} byte):
        <input
          type="file"
          accept="image/jpeg"
          onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          required
        />
      </label>
      <p>
        <button type="submit" disabled={busy}>
          {busy ? "送信中…" : "作成"}
        </button>
      </p>
      {err && <p className="error">{err}</p>}
    </form>
  );
}
