package bouncer

import "core:encoding/json"
import "core:strings"

// A registered agent backend and the models it offers. "Agent" is deliberately
// broad: it may be a raw model endpoint (a classic OpenRouter target) or a full
// agent runtime that drives tools. From the router's point of view both are the
// same shape — an endpoint to reach and a set of models it accepts.
Agent :: struct {
	// The id callers name this agent by in a route.
	id:       AgentId,
	// Transport family this agent speaks, e.g. "anthropic" or
	// "openai-compatible". A Router dispatches to the Provider registered for
	// this kind, so many agents can share one transport. Empty means
	// "unspecified" — the router falls back to its default provider.
	kind:     string,
	// Where the agent is reached, e.g. a base URL. Opaque to the router.
	endpoint: string,
	// Models this agent will accept. Empty means "accepts any model".
	models:   [dynamic]ModelId,
	// Maps a caller-facing model name to this agent's provider-specific id,
	// e.g. "gpt-4" -> "openai/gpt-4". An alias key counts as offered, and the
	// mapped value is what gets forwarded.
	aliases:  map[ModelId]ModelId,
	// Request-body template; nil uses the kind default. {{model}} and {{prompt}}
	// are substituted as JSON string literals.
	body:     Maybe(string),
}

// Default body template for OpenAI-compatible chat endpoints.
OPENAI_BODY :: `{"model":{{model}},"messages":[{"role":"user","content":{{prompt}}}]}`
// Default body template for the Anthropic messages endpoint.
ANTHROPIC_BODY :: `{"model":{{model}},"max_tokens":1024,"messages":[{"role":"user","content":{{prompt}}}]}`

agent_new :: proc(id: AgentId, endpoint: string) -> Agent {
	return Agent{
		id       = id,
		endpoint = endpoint,
		models   = make([dynamic]ModelId),
		aliases  = make(map[ModelId]ModelId),
	}
}

agent_destroy :: proc(a: ^Agent) {
	delete(a.models)
	delete(a.aliases)
}

agent_with_kind :: proc(a: Agent, kind: string) -> Agent {
	a := a
	a.kind = kind
	return a
}

agent_with_model :: proc(a: Agent, model: ModelId) -> Agent {
	a := a
	append(&a.models, model)
	return a
}

agent_with_alias :: proc(a: Agent, public, actual: ModelId) -> Agent {
	a := a
	a.aliases[public] = actual
	return a
}

agent_with_body :: proc(a: Agent, template: string) -> Agent {
	a := a
	a.body = template
	return a
}

// agent_offers reports whether this agent accepts model. An alias key counts as
// offered; an agent with no declared models is open and accepts anything.
agent_offers :: proc(a: ^Agent, model: ModelId) -> bool {
	if model in a.aliases do return true
	if len(a.models) == 0 do return true
	for m in a.models do if m == model do return true
	return false
}

// agent_resolve_model translates a caller-facing model name into the id to
// actually forward: the alias target if one exists, otherwise unchanged.
agent_resolve_model :: proc(a: ^Agent, requested: ModelId) -> ModelId {
	if target, ok := a.aliases[requested]; ok do return target
	return requested
}

// agent_body_template returns the template to use: the configured one, else a
// built-in default for the agent's kind. ok is false when there is no template
// (the prompt is forwarded verbatim).
agent_body_template :: proc(a: ^Agent) -> (template: string, ok: bool) {
	if body, has := a.body.?; has do return body, true
	switch a.kind {
	case "openai-compatible":
		return OPENAI_BODY, true
	case "anthropic":
		return ANTHROPIC_BODY, true
	}
	return "", false
}

// agent_render_body renders a prompt into the wire body for this agent:
// substitute model and prompt into the template as JSON string literals.
// ok is false when the agent has no template, leaving the caller to forward the
// prompt bytes unchanged. The returned slice is owned by the caller.
agent_render_body :: proc(a: ^Agent, model: ModelId, prompt: []byte, allocator := context.allocator) -> (body: []byte, ok: bool) {
	template := agent_body_template(a) or_return

	model_lit := json_string(string(model))
	defer delete(model_lit)
	prompt_lit := json_string(strings.trim_space(string(prompt)))
	defer delete(prompt_lit)

	with_model, _ := strings.replace_all(template, "{{model}}", model_lit)
	defer delete(with_model)
	rendered, _ := strings.replace_all(with_model, "{{prompt}}", prompt_lit, allocator)
	return transmute([]byte)rendered, true
}

// json_string encodes s as a JSON string literal (quoted, escaped).
json_string :: proc(s: string, allocator := context.allocator) -> string {
	data, err := json.marshal(s, allocator = allocator)
	if err != nil do return strings.clone(`""`, allocator)
	return string(data)
}
