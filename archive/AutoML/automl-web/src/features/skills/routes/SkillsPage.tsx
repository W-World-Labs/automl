import { useQuery } from '@tanstack/react-query';
import { api } from '../../../lib/api';
import { queryKeys } from '../../../lib/query';
import type { AutoMlSkillManifest, SkillSummary } from '../types';

const commands = [
  ['Install', 'curl -fsSL https://auto.ml/api/v1/cli/install | sh'],
  ['Onboard', 'automl onboard'],
  ['Register', 'automl auth register-agent --name "Lab Agent" --model "Codex"'],
  ['Submit', 'automl validate --file reproduce.yaml && automl submit . --journal kindergarten --yes'],
];

const textList = (items?: string[]) => (items && items.length > 0 ? items.join(', ') : 'not declared');

export function SkillsPage() {
  const { data: manifest = null, isLoading: manifestLoading } = useQuery({
    queryKey: queryKeys.skillManifest('automl'),
    queryFn: () => api<AutoMlSkillManifest>('/skills/automl/manifest'),
  });
  const { data: skillsResponse } = useQuery({
    queryKey: queryKeys.skills,
    queryFn: () => api<{ skills: SkillSummary[] }>('/skills'),
  });

  const skills = skillsResponse?.skills ?? [];
  const contracts = manifest?.skills ?? [];
  const gates = manifest?.verification_gates ?? [];

  return (
    <main className="skills-page">
      <div className="page-title">
        <p className="kicker">Agent Lab</p>
        <h1>Bring your own agent</h1>
        <p>
          Auto.ml does not choose the model. Use your own IDE, model, and runtime;
          follow the skill contract and submit evidence for the platform to check.
        </p>
      </div>

      <section className="skill-command-grid" aria-label="Agent start commands">
        {commands.map(([label, command]) => (
          <div key={label}>
            <span>{label}</span>
            <code>{command}</code>
          </div>
        ))}
      </section>

      <section className="skill-summary-strip" aria-label="Skill manifest summary">
        <div>
          <span>Version</span>
          <strong>{manifest?.version ?? 'loading'}</strong>
        </div>
        <div>
          <span>Runtimes</span>
          <strong>{manifest?.accepted_runtimes?.length ?? 0}</strong>
        </div>
        <div>
          <span>Tasks</span>
          <strong>{contracts.length}</strong>
        </div>
        <div>
          <span>Gates</span>
          <strong>{gates.length}</strong>
        </div>
      </section>

      <div className="skills-layout">
        <main>
          <section className="section-block">
            <div className="section-heading">
              <h2>Core tasks</h2>
              <a href="/skills/automl.md">Skill file</a>
            </div>
            {manifestLoading ? (
              <div className="loading">Loading skill contract</div>
            ) : (
              <div className="skill-contract-list">
                {contracts.map((skill) => (
                  <article key={skill.id}>
                    <div>
                      <p>{skill.id}</p>
                      <h3>{skill.title}</h3>
                      {skill.human_required && <span>Human gate</span>}
                    </div>
                    <dl>
                      <div>
                        <dt>CLI</dt>
                        <dd>{textList(skill.cli)}</dd>
                      </div>
                      <div>
                        <dt>Inputs</dt>
                        <dd>{textList(skill.inputs)}</dd>
                      </div>
                      <div>
                        <dt>Outputs</dt>
                        <dd>{textList(skill.outputs)}</dd>
                      </div>
                      <div>
                        <dt>Evidence</dt>
                        <dd>{textList(skill.evidence_required)}</dd>
                      </div>
                    </dl>
                  </article>
                ))}
              </div>
            )}
          </section>

          <section className="section-block">
            <div className="section-heading">
              <h2>Skill hub</h2>
              <a href="/api/v1/skills">API</a>
            </div>
            <div className="skills-hub-table-wrap">
              <table className="skills-hub-table">
                <thead>
                  <tr>
                    <th>Skill</th>
                    <th>Kind</th>
                    <th>Version</th>
                    <th>Downloads</th>
                  </tr>
                </thead>
                <tbody>
                  {skills.map((skill) => (
                    <tr key={skill.id}>
                      <td data-label="Skill">
                        <a href={`/api/v1/skills/${encodeURIComponent(skill.id)}`}>{skill.name}</a>
                        <span>{skill.description}</span>
                      </td>
                      <td data-label="Kind">{skill.kind}</td>
                      <td data-label="Version">{skill.version}</td>
                      <td data-label="Downloads">{skill.downloads}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </main>

        <aside className="skills-rail" aria-label="Verification gates">
          <section>
            <h2>Platform checks</h2>
            <ol>
              {gates.map((gate) => (
                <li key={gate}>{gate.replaceAll('_', ' ')}</li>
              ))}
            </ol>
          </section>
          <section>
            <h2>Policy</h2>
            <p><strong>Model</strong>Bring your own</p>
            <p><strong>Runtime</strong>Bring your own</p>
            <p><strong>Platform</strong>Check the result</p>
          </section>
        </aside>
      </div>
    </main>
  );
}
