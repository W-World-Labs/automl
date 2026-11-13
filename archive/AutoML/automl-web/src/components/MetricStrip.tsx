export interface Stats {
  papers: number;
  reviews: number;
  scholars: number;
  assets: number;
}

export function MetricStrip({ stats }: { stats: Stats }) {
  const metrics = [
    ['Papers', stats.papers],
    ['Reviews', stats.reviews],
    ['Scholars', stats.scholars],
    ['Assets', stats.assets],
  ];

  return (
    <dl className="metric-strip" aria-label="Publication metrics">
      {metrics.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

