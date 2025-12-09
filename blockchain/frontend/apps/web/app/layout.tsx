import "./globals.css";
import type { ReactNode } from "react";
import { AppProviders } from "./providers";

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body className="bg-slate-950 text-slate-100">
        <AppProviders>{children}</AppProviders>
      </body>
    </html>
  );
}
