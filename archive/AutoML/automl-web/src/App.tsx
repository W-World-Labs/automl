import { BrowserRouter, Route, Routes } from 'react-router-dom';
import { TopFrame } from './components/TopFrame';
import { AccountPage } from './features/auth/routes/AccountPage';
import { PaperDetailPage } from './features/papers/routes/PaperDetailPage';
import { PapersPage } from './features/papers/routes/PapersPage';
import { SiliconScholarProfilePage } from './features/scholar/routes/SiliconScholarProfilePage';
import { SiliconScholarsPage } from './features/scholar/routes/SiliconScholarsPage';
import { SkillsPage } from './features/skills/routes/SkillsPage';
import { onScholarHost } from './lib/urls';
import { Dashboard } from './routes/Dashboard';
import { JournalsPage } from './routes/JournalsPage';

export default function App() {
  const scholarHost = onScholarHost();

  return (
    <BrowserRouter>
      <TopFrame scholarHost={scholarHost} />
      <div className="site-body">
        {scholarHost ? (
          <Routes>
            <Route path="/" element={<SiliconScholarsPage />} />
            <Route path="/:id" element={<SiliconScholarProfilePage />} />
          </Routes>
        ) : (
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/papers" element={<PapersPage />} />
            <Route path="/papers/:id" element={<PaperDetailPage />} />
            <Route path="/journals" element={<JournalsPage />} />
            <Route path="/scholars" element={<SiliconScholarsPage />} />
            <Route path="/scholars/:id" element={<SiliconScholarProfilePage />} />
            <Route path="/skills" element={<SkillsPage />} />
            <Route path="/account" element={<AccountPage />} />
          </Routes>
        )}
      </div>
    </BrowserRouter>
  );
}
