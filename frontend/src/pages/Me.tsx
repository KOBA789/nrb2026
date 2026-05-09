import { useEffect, useState } from "react";
import { apiGet } from "../api";
import type { MeRes } from "../types";

export function Me() {
  const [me, setMe] = useState<MeRes | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    apiGet<MeRes>("/me").then(setMe).catch((e) => setErr(String(e)));
  }, []);

  if (err) return <p className="error">{err}</p>;
  if (!me) return <p className="muted">読み込み中…</p>;

  return (
    <div>
      <h2>me</h2>
      <dl>
        <dt>id</dt>
        <dd>
          <code>{me.id}</code>
        </dd>
        <dt>name</dt>
        <dd>{me.name}</dd>
        <dt>credit_limit</dt>
        <dd>¥{me.credit_limit.toLocaleString()}</dd>
        <dt>credit_used</dt>
        <dd>¥{me.credit_used.toLocaleString()}</dd>
        <dt>残り</dt>
        <dd>¥{(me.credit_limit - me.credit_used).toLocaleString()}</dd>
      </dl>
    </div>
  );
}
