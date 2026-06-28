package bouncer

// Stable string identifiers for the three things a route names. Distinct types
// so an agent id can never be passed where a model id is expected — the
// compiler keeps the three axes from being swapped.

// AgentId names a registered agent backend, e.g. "claude-code".
AgentId :: distinct string

// ModelId names a model offered by an agent, e.g. "claude-opus-4-8".
ModelId :: distinct string

// AuthId names a credential bundle, e.g. "work" or "personal".
AuthId :: distinct string
