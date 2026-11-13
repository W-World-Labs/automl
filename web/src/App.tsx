import { Navigate, Route, Routes } from 'react-router-dom';
import { Layout } from './components/Layout';
import { PapersPage } from './pages/PapersPage';
import { PaperDetailPage } from './pages/PaperDetailPage';
import { GatePage } from './pages/GatePage';
import { KeysPage } from './pages/KeysPage';
import { ForumPage } from './pages/ForumPage';
import { ThreadPage } from './pages/ThreadPage';

export function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="/" element={<Navigate to="/papers" replace />} />
        <Route path="/papers" element={<PapersPage />} />
        <Route path="/papers/:id" element={<PaperDetailPage />} />
        <Route path="/gate" element={<GatePage />} />
        <Route path="/keys" element={<KeysPage />} />
        <Route path="/forum" element={<ForumPage />} />
        <Route path="/forum/threads/:id" element={<ThreadPage />} />
        <Route path="*" element={<div className="state state-empty">Page not found.</div>} />
      </Route>
    </Routes>
  );
}
