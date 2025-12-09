"use client";

import { FormEvent, useState } from "react";
import { Panel } from "../components/Panel";
import { proposeGovernance, voteGovernance } from "../lib/api";

export default function Governance() {
  const [proposalPayload, setProposalPayload] = useState("{}");
  const [votePayload, setVotePayload] = useState("{}");
  const [status, setStatus] = useState<string | null>(null);

  async function onPropose(e: FormEvent) {
    e.preventDefault();
    setStatus("submitting proposal...");
    try {
      const payload = JSON.parse(proposalPayload || "{}");
      const res = await proposeGovernance(payload);
      setStatus(`proposal submitted: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`proposal failed: ${(err as Error).message}`);
    }
  }

  async function onVote(e: FormEvent) {
    e.preventDefault();
    setStatus("submitting vote...");
    try {
      const payload = JSON.parse(votePayload || "{}");
      const res = await voteGovernance(payload);
      setStatus(`vote submitted: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`vote failed: ${(err as Error).message}`);
    }
  }

  return (
    <main className="max-w-4xl mx-auto p-6 space-y-4">
      <div>
        <h1 className="text-2xl font-semibold">Governance</h1>
        <p className="text-sm text-slate-300">Propose, vote, and monitor execution.</p>
      </div>

      <Panel title="Create proposal">
        <form className="space-y-3" onSubmit={onPropose}>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Proposal payload JSON</span>
            <textarea
              className="bg-slate-800 rounded px-3 py-2 text-slate-100 h-28"
              value={proposalPayload}
              onChange={(e) => setProposalPayload(e.target.value)}
              placeholder='{"title":"Upgrade", "actions":[]}'
            />
          </label>
          <button
            type="submit"
            className="bg-emerald-500 hover:bg-emerald-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Submit proposal
          </button>
        </form>
      </Panel>

      <Panel title="Vote">
        <form className="space-y-3" onSubmit={onVote}>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Vote payload JSON</span>
            <textarea
              className="bg-slate-800 rounded px-3 py-2 text-slate-100 h-24"
              value={votePayload}
              onChange={(e) => setVotePayload(e.target.value)}
              placeholder='{"proposal_id":"...", "choice":"for"}'
            />
          </label>
          <button
            type="submit"
            className="bg-blue-500 hover:bg-blue-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Cast vote
          </button>
        </form>
      </Panel>

      {status && (
        <div className="text-xs text-slate-200 bg-slate-800 border border-slate-700 rounded p-3">
          {status}
        </div>
      )}
    </main>
  );
}
