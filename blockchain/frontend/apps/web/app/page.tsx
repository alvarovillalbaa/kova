type ChainStats = { tps: number; block_time_ms: number };
type DaStats = { blobs_per_block: number };
type DomainStats = { domains: { id: string; type: string }[] };

async function fetchJson<T>(path: string): Promise<T | null> {
  const base = process.env.NEXT_PUBLIC_INDEXER_URL || "http://localhost:4000";
  try {
    const res = await fetch(`${base}${path}`, { cache: "no-store" });
    if (!res.ok) return null;
    return (await res.json()) as T;
  } catch {
    return null;
  }
}

export default async function Home() {
  const [chain, da, domains] = await Promise.all([
    fetchJson<ChainStats>("/stats/chain"),
    fetchJson<DaStats>("/stats/da"),
    fetchJson<DomainStats>("/stats/domains"),
  ]);

  return (
    <main className="space-y-4 p-4">
      <h1 className="text-2xl font-bold">Kova Dashboard</h1>
      <section>
        <h2 className="text-xl font-semibold">L1</h2>
        <p>TPS: {chain?.tps ?? "n/a"}</p>
        <p>Block time (ms): {chain?.block_time_ms ?? "n/a"}</p>
      </section>
      <section>
        <h2 className="text-xl font-semibold">Data Availability</h2>
        <p>Blobs per block: {da?.blobs_per_block ?? "n/a"}</p>
      </section>
      <section>
        <h2 className="text-xl font-semibold">Domains</h2>
        <ul className="list-disc pl-5">
          {(domains?.domains ?? []).map((d) => (
            <li key={d.id}>
              {d.id} — {d.type}
            </li>
          ))}
        </ul>
        {(!domains || domains.domains.length === 0) && <p>No domains indexed yet.</p>}
      </section>
      <section>
        <h2 className="text-xl font-semibold">Testnet</h2>
        <p>Faucet: coming soon</p>
      </section>
    </main>
  );
}
