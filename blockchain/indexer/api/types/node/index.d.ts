// Minimal Node globals used by the Fastify server. Extend when adding APIs.
declare const process: {
  env: Record<string, string | undefined>;
};
