package bouncer

// What a caller asks for, and what it resolves to.

// A routing request, stated purely by name: "send this to agent, running
// model, under auth". It carries no secrets and no endpoint — just the names
// the Registry needs to look up. The agent is optional: a route with no agent
// is a direct model call — the registry picks the one agent that offers the
// model, or reports the choices when zero or several do.
Route :: struct {
	agent: Maybe(AgentId),
	model: ModelId,
	auth:  AuthId,
}

route_new :: proc(agent: AgentId, model: ModelId, auth: AuthId) -> Route {
	return Route{agent = agent, model = model, auth = auth}
}

// route_direct builds a direct model call: no agent named, so the registry
// picks the one agent that offers model.
route_direct :: proc(model: ModelId, auth: AuthId) -> Route {
	return Route{agent = nil, model = model, auth = auth}
}

// route_agent_label is the agent id for display, or "*" when none is named.
route_agent_label :: proc(r: ^Route) -> string {
	if id, ok := r.agent.?; ok do return string(id)
	return "*"
}

// A Route resolved against the registry into concrete targets. The agent and
// auth point into the Registry that produced it, so they must not outlive it.
// The model is carried by value — exactly the id to forward (alias-translated).
Resolved :: struct {
	agent: ^Agent,
	model: ModelId,
	auth:  ^AuthSet,
}
