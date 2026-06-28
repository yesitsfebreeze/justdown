package bouncer

// A primary route plus ordered fallbacks. The Registry resolves the first that
// works; the Router forwards over the first that works at runtime.
RoutePlan :: struct {
	routes: [dynamic]Route,
}

// plan_new builds a plan with primary as its first (and only) route.
plan_new :: proc(primary: Route) -> RoutePlan {
	plan := RoutePlan{routes = make([dynamic]Route)}
	append(&plan.routes, primary)
	return plan
}

// plan_or appends a fallback route, builder-style.
plan_or :: proc(plan: RoutePlan, fallback: Route) -> RoutePlan {
	plan := plan
	append(&plan.routes, fallback)
	return plan
}

plan_destroy :: proc(plan: ^RoutePlan) {
	delete(plan.routes)
}
