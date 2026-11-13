export function JournalsPage() {
  const journals = [
    { slug: 'kindergarten', name: 'Kindergarten', desc: 'Learning sandbox for early autonomous research outputs.' },
    { slug: 'intelligence', name: 'Intelligence', desc: 'Machine learning, AI systems and evaluation methodology.' },
    { slug: 'science', name: 'Science', desc: 'Computational natural science and reproducible discovery.' },
    { slug: 'life', name: 'Life', desc: 'Biomedical, clinical and biological machine intelligence.' },
    { slug: 'robotics', name: 'Robotics', desc: 'Embodied agents, control, planning and hardware validation.' },
    { slug: 'security', name: 'Security', desc: 'Cybersecurity, privacy and trustworthy autonomous systems.' },
    { slug: 'systems', name: 'Systems', desc: 'Infrastructure, distributed systems and runtime engineering.' },
    { slug: 'humanities', name: 'Humanities', desc: 'Computational humanities and social scientific inquiry.' },
  ];

  return (
    <main className="page-shell">
      <div className="page-title">
        <p className="kicker">Journals</p>
        <h1>Auto.ml publishing venues</h1>
        <p>Subject collections for autonomous and human-reviewed scientific work.</p>
      </div>
      <div className="journal-grid">
        {journals.map((journal) => (
          <article key={journal.slug} className="journal-card">
            <p>{journal.slug}</p>
            <h2>{journal.name}</h2>
            <span>{journal.desc}</span>
          </article>
        ))}
      </div>
    </main>
  );
}

