package bouncer

import "core:slice"

// The catalog the router resolves against. Holds every registered Agent and
// AuthSet. Resolution is a pure lookup — no I/O — so it is cheap and testable.
Registry :: struct {
	agents: map[AgentId]Agent,
	auth:   map[AuthId]AuthSet,
}

registry_new :: proc() -> Registry {
	return Registry{agents = make(map[AgentId]Agent), auth = make(map[AuthId]AuthSet)}
}

registry_destroy :: proc(reg: ^Registry) {
	for _, &a in reg.agents do agent_destroy(&a)
	for _, &s in reg.auth do auth_destroy(&s)
	delete(reg.agents)
	delete(reg.auth)
}

// registry_register_agent registers an agent, replacing any prior one of the
// same id.
registry_register_agent :: proc(reg: ^Registry, agent: Agent) {
	if existing, ok := &reg.agents[agent.id]; ok do agent_destroy(existing)
	reg.agents[agent.id] = agent
}

// registry_register_auth registers an auth set, replacing any prior set of the
// same id.
registry_register_auth :: proc(reg: ^Registry, auth: AuthSet) {
	if existing, ok := &reg.auth[auth.id]; ok do auth_destroy(existing)
	reg.auth[auth.id] = auth
}

registry_agent :: proc(reg: ^Registry, id: AgentId) -> (^Agent, bool) {
	a, ok := &reg.agents[id]
	return a, ok
}

registry_auth :: proc(reg: ^Registry, id: AuthId) -> (^AuthSet, bool) {
	s, ok := &reg.auth[id]
	return s, ok
}

// registry_agents_offering returns every agent that offers model, sorted by id
// for deterministic output. Includes open agents (no declared models). The
// returned slice is owned by the caller.
registry_agents_offering :: proc(reg: ^Registry, model: ModelId, allocator := context.allocator) -> []^Agent {
	found := make([dynamic]^Agent, allocator)
	for _, &a in reg.agents {
		if agent_offers(&a, model) do append(&found, &a)
	}
	slice.sort_by(found[:], proc(x, y: ^Agent) -> bool {
		return x.id < y.id
	})
	return found[:]
}

// registry_resolve resolves a Route into concrete targets, or explains why it
// cannot. Checks, in order: the agent exists, it offers the model, and the auth
// set exists. A nil RouteError means success.
registry_resolve :: proc(reg: ^Registry, route: Route) -> (Resolved, RouteError) {
	agent: ^Agent
	if id, named := route.agent.?; named {
		// A named agent: it must exist and offer the model.
		found, ok := &reg.agents[id]
		if !ok do return {}, Unknown_Agent{id}
		if !agent_offers(found, route.model) {
			return {}, Model_Not_Offered{id, route.model}
		}
		agent = found
	} else {
		// A direct model call: the sole agent offering the model, or an error
		// naming what to disambiguate.
		candidates := registry_agents_offering(reg, route.model, context.temp_allocator)
		switch len(candidates) {
		case 0:
			return {}, No_Agent_For_Model{route.model}
		case 1:
			agent = candidates[0]
		case:
			ids := make([]AgentId, len(candidates))
			for c, i in candidates do ids[i] = c.id
			return {}, Ambiguous_Model{route.model, ids}
		}
	}

	auth, ok := &reg.auth[route.auth]
	if !ok do return {}, Unknown_Auth{route.auth}

	// The model is validated above. Translate it through the agent's aliases so
	// the provider forwards the backend's own id.
	return Resolved{agent = agent, model = agent_resolve_model(agent, route.model), auth = auth}, nil
}

// registry_resolve_plan tries the primary, then each fallback in order, and
// returns the first route that resolves. If every route fails, returns a
// PlanError carrying each attempt and why it failed. ok is true on success.
registry_resolve_plan :: proc(reg: ^Registry, plan: ^RoutePlan, allocator := context.allocator) -> (Resolved, PlanError, bool) {
	attempts := make([dynamic]Attempt, allocator)
	for route in plan.routes {
		resolved, err := registry_resolve(reg, route)
		if err == nil {
			delete(attempts)
			return resolved, {}, true
		}
		append(&attempts, Attempt{route, err})
	}
	return {}, PlanError{attempts[:]}, false
}
