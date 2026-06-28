package bouncer

import "core:fmt"
import "core:strings"

// Why a Route could not be resolved against the Registry. A nil RouteError
// means success.
RouteError :: union {
	Unknown_Agent,
	Model_Not_Offered,
	Unknown_Auth,
	No_Agent_For_Model,
	Ambiguous_Model,
}

// Unknown_Agent: the named agent is not registered.
Unknown_Agent :: struct {
	agent: AgentId,
}

// Model_Not_Offered: the agent exists but does not offer the model.
Model_Not_Offered :: struct {
	agent: AgentId,
	model: ModelId,
}

// Unknown_Auth: the named auth set is not registered.
Unknown_Auth :: struct {
	auth: AuthId,
}

// No_Agent_For_Model: a direct call named a model no registered agent offers.
No_Agent_For_Model :: struct {
	model: ModelId,
}

// Ambiguous_Model: a direct call named a model several agents offer; name one.
Ambiguous_Model :: struct {
	model:      ModelId,
	candidates: []AgentId,
}

route_error_string :: proc(err: RouteError, allocator := context.allocator) -> string {
	switch e in err {
	case Unknown_Agent:
		return fmt.aprintf("unknown agent '%s'", string(e.agent), allocator = allocator)
	case Model_Not_Offered:
		return fmt.aprintf("agent '%s' does not offer model '%s'", string(e.agent), string(e.model), allocator = allocator)
	case Unknown_Auth:
		return fmt.aprintf("unknown auth set '%s'", string(e.auth), allocator = allocator)
	case No_Agent_For_Model:
		return fmt.aprintf("no registered agent offers model '%s'", string(e.model), allocator = allocator)
	case Ambiguous_Model:
		names := make([]string, len(e.candidates), context.temp_allocator)
		for c, i in e.candidates do names[i] = string(c)
		joined := strings.join(names, ", ", context.temp_allocator)
		return fmt.aprintf("model '%s' is offered by several agents: %s", string(e.model), joined, allocator = allocator)
	}
	return strings.clone("ok", allocator)
}

// One failed resolution attempt inside a RoutePlan: the route tried and why it
// failed.
Attempt :: struct {
	route: Route,
	err:   RouteError,
}

// PlanError carries every attempt a RoutePlan made when all routes failed — the
// whole story, not just the last error. A nil PlanError means success.
PlanError :: struct {
	attempts: []Attempt,
}
