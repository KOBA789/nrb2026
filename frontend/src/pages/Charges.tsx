import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { apiGet } from "../api";
import type { ChargeRes } from "../types";

export function Charges() {
  const [charges, setCharges] = useState<ChargeRes[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    apiGet<ChargeRes[]>("/charges").then(setCharges).catch((e) => setErr(String(e)));
  }, []);

  if (err) return <p className="error">{err}</p>;
  if (!charges) return <p className="muted">読み込み中…</p>;

  return (
    <div>
      <h2>charges</h2>
      {charges.length === 0 ? (
        <p className="muted">課金はまだありません</p>
      ) : (
        <table className="charges">
          <thead>
            <tr>
              <th>created_at</th>
              <th>amount</th>
              <th>campaign</th>
            </tr>
          </thead>
          <tbody>
            {charges.map((c) => (
              <tr key={c.id}>
                <td>{c.created_at}</td>
                <td>¥{c.amount.toLocaleString()}</td>
                <td>
                  <Link to={`/campaigns/${c.campaign.id}`}>{c.campaign.name}</Link>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
