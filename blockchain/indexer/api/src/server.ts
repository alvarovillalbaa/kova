import Fastify from "fastify";

const fastify = Fastify({ logger: true });

fastify.get("/stats/chain", async () => ({ tps: 1, block_time_ms: 1000 }));
fastify.get("/stats/da", async () => ({ blobs_per_block: 1 }));
fastify.get("/stats/domains", async () => ({
  domains: [
    { id: "evm-shared", type: "EVM_SHARED_SECURITY" },
    { id: "privacy", type: "PRIVACY" },
  ],
}));
fastify.get("/stats/sequencer", async () => ({ liveness: "ok", head: 0 }));
fastify.get("/stats/mixnet", async () => ({ enabled: true }));

const port = Number(process.env.PORT || 4000);

async function start() {
  try {
    await fastify.listen({ port, host: "0.0.0.0" });
  } catch (err) {
    fastify.log.error(err);
    process.exit(1);
  }
}

start();
