"use client";

import { ConnectButton } from "@rainbow-me/rainbowkit";
import { FormEvent, useState } from "react";
import { useAccount } from "wagmi";
import { Panel } from "../components/Panel";
import { requestFaucet, sendRawTx } from "../lib/api";

export default function Wallet() {
  const { address, isConnected } = useAccount();
  const [rawTx, setRawTx] = useState("");
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("0");
  const [status, setStatus] = useState<string | null>(null);
  const [faucetAmount, setFaucetAmount] = useState("100000");
  const [bridgeDomain, setBridgeDomain] = useState("");
  const [bridgePayload, setBridgePayload] = useState("{}");

  async function onSend(e: FormEvent) {
    e.preventDefault();
    setStatus("sending tx...");
    try {
      const parsed = rawTx ? JSON.parse(rawTx) : { to, amount: Number(amount) };
      const res = await sendRawTx(parsed);
      setStatus(`ok: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`error: ${(err as Error).message}`);
    }
  }

  async function onFaucet(e: FormEvent) {
    e.preventDefault();
    if (!address) return;
    setStatus("requesting faucet...");
    try {
      const res = await requestFaucet(address, Number(faucetAmount || "0"));
      setStatus(`faucet sent: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`faucet failed: ${(err as Error).message}`);
    }
  }

  async function onBridge(e: FormEvent) {
    e.preventDefault();
    setStatus("submitting cross-domain payload...");
    try {
      const payload = JSON.parse(bridgePayload || "{}");
      const tx = {
        payload: {
          CrossDomainSend: {
            from_domain: bridgeDomain,
            to_domain: bridgeDomain,
            payload,
            fee: 1,
          },
        },
      };
      const res = await sendRawTx(tx);
      setStatus(`bridge submitted: ${JSON.stringify(res)}`);
    } catch (err) {
      setStatus(`bridge failed: ${(err as Error).message}`);
    }
  }

  return (
    <main className="max-w-4xl mx-auto p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Wallet</h1>
          <p className="text-sm text-slate-300">Connect, fund, and submit raw txs.</p>
        </div>
        <ConnectButton />
      </div>

      <Panel title="Transfer / Raw Tx submit">
        <form className="space-y-3" onSubmit={onSend}>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-xs uppercase tracking-wide text-slate-400">To</span>
              <input
                className="bg-slate-800 rounded px-3 py-2 text-slate-100"
                value={to}
                onChange={(e) => setTo(e.target.value)}
                placeholder="0x..."
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs uppercase tracking-wide text-slate-400">Amount</span>
              <input
                className="bg-slate-800 rounded px-3 py-2 text-slate-100"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                type="number"
                min="0"
              />
            </label>
          </div>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Raw tx JSON (optional)</span>
            <textarea
              className="bg-slate-800 rounded px-3 py-2 text-slate-100 h-28"
              value={rawTx}
              onChange={(e) => setRawTx(e.target.value)}
              placeholder='{"tx":{...}}'
            />
          </label>
          <button
            type="submit"
            className="bg-emerald-500 hover:bg-emerald-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Send
          </button>
        </form>
      </Panel>

      <Panel title="Faucet">
        <form className="space-y-3" onSubmit={onFaucet}>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <div>
              <p className="text-xs text-slate-400">Connected address</p>
              <p className="font-mono text-slate-100 truncate">{address || "connect wallet"}</p>
            </div>
            <label className="flex flex-col gap-1">
              <span className="text-xs uppercase tracking-wide text-slate-400">Amount</span>
              <input
                className="bg-slate-800 rounded px-3 py-2 text-slate-100"
                value={faucetAmount}
                onChange={(e) => setFaucetAmount(e.target.value)}
                type="number"
                min="1"
              />
            </label>
          </div>
          <button
            type="submit"
            disabled={!isConnected}
            className="bg-blue-500 hover:bg-blue-600 disabled:opacity-50 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Request funds
          </button>
        </form>
      </Panel>

      <Panel title="Cross-domain send">
        <form className="space-y-3" onSubmit={onBridge}>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Domain ID (uuid)</span>
            <input
              className="bg-slate-800 rounded px-3 py-2 text-slate-100"
              value={bridgeDomain}
              onChange={(e) => setBridgeDomain(e.target.value)}
              placeholder="domain uuid"
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-400">Payload JSON</span>
            <textarea
              className="bg-slate-800 rounded px-3 py-2 text-slate-100 h-24"
              value={bridgePayload}
              onChange={(e) => setBridgePayload(e.target.value)}
              placeholder='{"hello": "world"}'
            />
          </label>
          <button
            type="submit"
            className="bg-indigo-500 hover:bg-indigo-600 text-slate-900 font-semibold px-4 py-2 rounded"
          >
            Submit cross-domain message
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
