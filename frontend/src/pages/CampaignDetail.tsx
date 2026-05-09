import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { apiGet, apiPostJson } from "../api";
import { CampaignImage } from "../components/CampaignImage";
import type { Campaign } from "../types";

export function CampaignDetail() {
  const { id } = useParams<{ id: string }>();
  const [campaign, setCampaign] = useState<Campaign | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [joining, setJoining] = useState(false);

  // id 切替時の race を避ける: cleanup で alive=false にして stale response の setState を捨てる。
  useEffect(() => {
    if (!id) return;
    let alive = true;
    setCampaign(null);
    setErr(null);
    apiGet<Campaign>(`/campaigns/${id}`)
      .then((c) => {
        if (alive) setCampaign(c);
      })
      .catch((e) => {
        if (alive) setErr(String(e));
      });
    return () => {
      alive = false;
    };
  }, [id]);

  const join = async () => {
    if (!id) return;
    setJoining(true);
    setErr(null);
    try {
      // body は {} 必須 (webapp 側 JsonReq<JoinReq> で empty body は 400)
      // join response 自体が CampaignRes なので追加 GET は不要。
      const next = await apiPostJson<Record<string, never>, Campaign>(
        `/campaigns/${id}/join`,
        {},
      );
      setCampaign(next);
    } catch (e) {
      setErr(String(e));
    } finally {
      setJoining(false);
    }
  };

  if (!id) return <p className="error">invalid id</p>;
  if (err && !campaign) return <p className="error">{err}</p>;
  if (!campaign) return <p className="muted">読み込み中…</p>;

  return (
    <div>
      <h2>{campaign.name}</h2>
      <CampaignImage id={campaign.id} className="campaign-image" />
      <p>{campaign.description}</p>
      <p className="muted">
        ¥{campaign.price.toLocaleString()} · {campaign.current_count}/{campaign.goal_count} ·{" "}
        status: {campaign.status}
      </p>
      <p>
        <span className="muted">tags:</span> {campaign.tags.join(", ")}
      </p>
      <p>
        <button onClick={join} disabled={joining || campaign.status !== "open"}>
          {joining ? "送信中…" : "参加する"}
        </button>
      </p>
      {err && <p className="error">{err}</p>}
      <h3>参加者 ({campaign.participants.length})</h3>
      <ul>
        {campaign.participants.map((p) => (
          <li key={p.user_id}>
            {p.name} <span className="muted">({p.joined_at})</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
