import { Link, Outlet, useNavigate } from "react-router-dom";
import { clearUserId, getUserId } from "../auth";

export function Layout() {
  const navigate = useNavigate();
  const userId = getUserId();
  const logout = () => {
    clearUserId();
    navigate("/login");
  };
  return (
    <>
      <header className="app-header">
        <strong>Isupon</strong>
        <nav>
          <Link to="/">campaigns</Link>
          <Link to="/campaigns/new">new</Link>
          <Link to="/me">me</Link>
          <Link to="/charges">charges</Link>
          <Link to="/saved_searches">saved_searches</Link>
        </nav>
        <span className="spacer" />
        <span className="muted">user: {userId ?? "(未ログイン)"}</span>
        <button onClick={logout}>Logout</button>
      </header>
      <main className="app-main">
        <Outlet />
      </main>
    </>
  );
}
