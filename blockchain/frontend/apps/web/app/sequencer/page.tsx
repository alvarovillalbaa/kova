"use client";

import { FormEvent, useState } from "react";
import { Panel } from "../components/Panel";
import { controlSequencer, submitBatch } from "../lib/api";

export default function Sequencer() {
  const [domainId, setDomainId] = useState("");
  const [batchPayload, setBatchPayload] = useState("{}");
  const [status, setStatus] = useState<string | null>(null);

  async function onAction(action: "pause" | "resume" | "flush") {
    setStatus(`${action} requested...`);
    try {
      const res = await controlSequencer(action);
      setStatus(`${action} ok: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`${action} failed: ${(err as Error).message}`);
    }
  }

  async function onSubmitBatch(e: FormEvent) {
    e.preventDefault();
    setStatus("submitting batch...");
    try {
      const payload = JSON.parse(batchPayload || "{}");
      const res = await submitBatch(domainId, payload);
      setStatus(`batch submitted: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`batch failed: ${(err as Error).message}`);
    }
  }

  return (
    <main className="max-w-4xl mx-auto p-6 space-y-4">
      <div>
        <h1 className="text-2xl font-semibold">Sequencer</h1>
        <p className="text-sm text-slate-300">Control and push batches.</p>
      </div>

      <Panel title="Controls">
        <div className="flex flex-wrap gap-3">
          <button
            onClick={() => onAction("pause")}
            className="bg-amber-500 hover:bg-amber-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Pause
          </button>
          <button
            onClick={() => onAction("resume")}
            className="bg-emerald-500 hover:bg-emerald-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Resume
          </button>
          <button
            onClick={() => onAction("flush")}
            className="bg-blue-500 hover:bg-blue-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Flush pending
          </button>
        </div>
      </Panel>

      <Panel title="Submit batch">
        <form className="space-y-3" onSubmit={onSubmitBatch}>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Domain ID</span>
            <input
              className="bg-slate-800 rounded px-3 py-2 text-slate-100"
              value={domainId}
              onChange={(e) => setDomainId(e.target.value)}
              placeholder="domain uuid"
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Batch payload JSON</span>
            <textarea
              className="bg-slate-800 rounded px-3 py-2 text-slate-100 h-28"
              value={batchPayload}
              onChange={(e) => setBatchPayload(e.target.value)}
              placeholder='{"txs": []}'
            />
          </label>
          <button
            type="submit"
            className="bg-indigo-500 hover:bg-indigo-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Submit to sequencer
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
