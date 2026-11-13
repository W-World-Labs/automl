export interface Health {
  status: string;
  version: string;
  env: string;
  services?: {
    database?: string;
    obscuraid?: string;
    radar?: string;
    storage?: {
      status?: string;
      bucket?: string;
    };
  };
}

export function HealthPanel({ health }: { health: Health | null }) {
  const database = health?.services?.database ?? 'unknown';
  const storage = health?.services?.storage?.status ?? 'unknown';
  const storageBucket = health?.services?.storage?.bucket ?? 'not configured';
  const obscuraid = health?.services?.obscuraid ?? 'unknown';
  const radar = health?.services?.radar ?? 'unknown';

  const row = (label: string, value: string) => (
    <div className="health-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );

  return (
    <aside className="right-rail" aria-label="Journal operations">
      <section className="rail-block">
        <h2>System status</h2>
        {row('Database', database)}
        {row('Object store', storage)}
        {row('Bucket', storageBucket)}
        {row('W-ID', obscuraid)}
        {row('Radar', radar)}
        <p className="rail-note">
          {health ? `Version ${health.version} in ${health.env}` : 'Loading current runtime status'}
        </p>
      </section>
      <section className="rail-block">
        <h2>For authors</h2>
        <a className="text-link" href="/auth/login">Submit a manuscript</a>
        <a className="text-link" href="/api/v1/health" target="_blank" rel="noreferrer">Read API health</a>
      </section>
      <section className="rail-block">
        <h2>For agents</h2>
        <a className="text-link" href="/skills">Open Agent Lab</a>
        <a className="text-link" href="/skills/automl.md">Read Auto.ml skill</a>
        <a className="text-link" href="/api/v1/skills/automl/manifest" target="_blank" rel="noreferrer">Inspect manifest</a>
      </section>
    </aside>
  );
}
