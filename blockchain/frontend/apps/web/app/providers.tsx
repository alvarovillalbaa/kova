/* eslint-disable react-hooks/rules-of-hooks */
"use client";

import { ReactNode, useMemo } from "react";
import { RainbowKitProvider, getDefaultConfig } from "@rainbow-me/rainbowkit";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { WagmiProvider, createConfig, http } from "wagmi";
import { defineChain } from "viem";
import "@rainbow-me/rainbowkit/styles.css";

const queryClient = new QueryClient();

function buildConfig() {
  const rpcUrl = process.env.NEXT_PUBLIC_RPC_URL || "http://localhost:8545";
  const chainId = Number(process.env.NEXT_PUBLIC_CHAIN_ID || 1337);
  const indexerUrl = process.env.NEXT_PUBLIC_INDEXER_URL || "http://localhost:4000";

  const kovaChain = defineChain({
    id: chainId,
    name: "Kova Testnet",
    nativeCurrency: { name: "KOVA", symbol: "KOVA", decimals: 18 },
    rpcUrls: {
      default: { http: [rpcUrl] },
      public: { http: [rpcUrl] },
    },
    blockExplorers: {
      default: { name: "indexer", url: indexerUrl },
    },
    testnet: true,
  });

  const config = getDefaultConfig({
    appName: "Kova",
    projectId: process.env.NEXT_PUBLIC_WALLETCONNECT_ID || "kova-dev",
    chains: [kovaChain],
    ssr: true,
    transports: {
      [kovaChain.id]: http(rpcUrl),
    },
  });

  // Keep typing happy for future overrides.
  return createConfig(config as ReturnType<typeof getDefaultConfig>);
}

const wagmiConfig = buildConfig();

export function AppProviders({ children }: { children: ReactNode }) {
  const providers = useMemo(
    () => (
      <WagmiProvider config={wagmiConfig}>
        <QueryClientProvider client={queryClient}>
          <RainbowKitProvider>{children}</RainbowKitProvider>
        </QueryClientProvider>
      </WagmiProvider>
    ),
    [children]
  );
  return providers;
}
