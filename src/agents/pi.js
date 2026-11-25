// Llamancer session-scoped Pi provider. Does not edit Pi configuration.
export default function registerLlamancer(pi) {
  const endpoint = process.env.LLAMANCER_ENDPOINT;
  const model = process.env.LLAMANCER_MODEL;
  const context = Number(process.env.LLAMANCER_CONTEXT);
  const provider = process.env.LLAMANCER_PROVIDER;
  if (!endpoint || !model || !/^llamancer-local-[a-f0-9]{16}$/.test(provider || "") || !Number.isSafeInteger(context) || context < 128) {
    throw new Error("Missing or invalid Llamancer session configuration");
  }
  const base = new URL(endpoint);
  if (!["http:", "https:"].includes(base.protocol) || base.username || base.password) {
    throw new Error("Invalid Llamancer inference URL");
  }
  base.pathname = "/v1";
  pi.registerProvider(provider, {
    name: "Llamancer local",
    baseUrl: base.toString(),
    api: "openai-completions",
    apiKey: "$LLAMANCER_PI_KEY",
    models: [{
      id: model,
      name: model,
      reasoning: false,
      input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: context,
      maxTokens: Math.min(4096, Math.floor(context / 2)),
      compat: { supportsStore: false, supportsReasoningEffort: false, supportsStrictMode: false, maxTokensField: "max_tokens" }
    }]
  });
}
