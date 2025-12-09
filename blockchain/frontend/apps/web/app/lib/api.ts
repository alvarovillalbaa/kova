const rpcUrl = process.env.NEXT_PUBLIC_RPC_URL || "http://localhost:8545";
const indexerUrl = process.env.NEXT_PUBLIC_INDEXER_URL || "http://localhost:4000";
const sequencerUrl = process.env.NEXT_PUBLIC_SEQUENCER_URL || "http://localhost:7545";

async function postJson(url: string, body: unknown) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

export async function sendRawTx(tx: unknown) {
  return postJson(`${rpcUrl}/send_raw_tx`, { tx });
}

export async function requestFaucet(address: string, amount: number) {
  const faucet = process.env.NEXT_PUBLIC_FAUCET_URL || "";
  if (!faucet) {
    throw new Error("NEXT_PUBLIC_FAUCET_URL not set");
  }
  return postJson(`${faucet}/fund`, { address, amount });
}

export async function controlSequencer(action: "pause" | "resume" | "flush") {
  return postJson(`${sequencerUrl}/admin/${action}`, {});
}

export async function submitBatch(domainId: string, payload: unknown) {
  return postJson(`${sequencerUrl}/v1/submit_tx`, { domain_id: domainId, tx: payload });
}

export async function proposeGovernance(payload: unknown) {
  return postJson(`${indexerUrl}/governance/propose`, payload);
}

export async function voteGovernance(payload: unknown) {
  return postJson(`${indexerUrl}/governance/vote`, payload);
}
