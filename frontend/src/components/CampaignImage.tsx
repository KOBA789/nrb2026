import { useEffect, useState } from "react";
import { apiGetBlob } from "../api";

// /campaigns/:id/image は X-User-ID 必須なので <img src> ではなく fetch → Blob → createObjectURL。
// 一覧でも詳細でも使う。一覧は最大 30 件なので campaign 数ぶん並列に走る (naive 方針: 遅さが見える)。

interface Props {
  id: string;
  className?: string;
}

export function CampaignImage({ id, className }: Props) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    let objUrl: string | null = null;
    setUrl(null);
    apiGetBlob(`/campaigns/${id}/image`)
      .then((blob) => {
        if (!alive) return;
        objUrl = URL.createObjectURL(blob);
        setUrl(objUrl);
      })
      .catch((e) => console.error("image load", id, e));
    return () => {
      alive = false;
      if (objUrl) URL.revokeObjectURL(objUrl);
    };
  }, [id]);

  if (!url) return <div className={className} aria-hidden />;
  return <img className={className} src={url} alt="" />;
}
