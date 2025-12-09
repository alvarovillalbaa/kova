import { ReactNode } from "react";

export function Panel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border border-slate-800 bg-slate-900/70 rounded-lg p-4 mb-4">
      <div className="flex items-center justify-between mb-2">
        <h2 className="text-lg font-semibold text-slate-100">{title}</h2>
      </div>
      <div className="text-sm text-slate-200 space-y-2">{children}</div>
    </section>
  );
}
