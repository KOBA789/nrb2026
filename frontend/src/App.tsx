import { Navigate, Route, Routes } from "react-router-dom";
import { getUserId } from "./auth";
import { Layout } from "./components/Layout";
import { Login } from "./pages/Login";
import { Home } from "./pages/Home";
import { CampaignDetail } from "./pages/CampaignDetail";
import { NewCampaign } from "./pages/NewCampaign";
import { Me } from "./pages/Me";
import { Charges } from "./pages/Charges";
import { SavedSearches } from "./pages/SavedSearches";

// 補助 frontend なので auth state は React state に乗せず localStorage を直接見る。
// Login → 各画面遷移時は navigate で十分 (CLAUDE.md "API first" 補助物として割り切り)。
function RequireAuth({ children }: { children: React.ReactNode }) {
  if (!getUserId()) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

export function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        element={
          <RequireAuth>
            <Layout />
          </RequireAuth>
        }
      >
        <Route index element={<Home />} />
        <Route path="campaigns/new" element={<NewCampaign />} />
        <Route path="campaigns/:id" element={<CampaignDetail />} />
        <Route path="me" element={<Me />} />
        <Route path="charges" element={<Charges />} />
        <Route path="saved_searches" element={<SavedSearches />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
