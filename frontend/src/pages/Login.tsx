import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { apiPostJson } from "../api";
import { setUserId } from "../auth";
import type { UserRes } from "../types";

export function Login() {
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const useExisting = () => {
    const input = window.prompt("既存の user_id (UUID) を入力してください");
    if (!input) return;
    const id = input.trim();
    if (!id) return;
    setUserId(id);
    navigate("/", { replace: true });
  };

  const createNew = async () => {
    const name = window.prompt("新規ユーザー名 (1〜100 文字)") ?? "";
    if (!name.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      const u = await apiPostJson<{ name: string }, UserRes>("/users", { name: name.trim() });
      setUserId(u.id);
      navigate("/", { replace: true });
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="app-main">
      <h1>Isupon</h1>
      <p>X-User-ID で認証する単純な API サンプル UI です。</p>
      <p>
        <button onClick={createNew} disabled={busy}>
          新規ユーザー作成
        </button>
        {" / "}
        <button onClick={useExisting} disabled={busy}>
          既存 user_id でログイン
        </button>
      </p>
      {err && <p className="error">{err}</p>}
    </main>
  );
}
