package bouncer

// Named credential bundles. An AuthSet is one named set of secrets the router
// can forward under, e.g. "work" vs "personal". The caller selects a set by
// name via its AuthId; the secret values never appear in a Route, so logging a
// route can never leak credentials. Values are opaque to the router — it only
// ever attaches them, never inspects them.

AuthSet :: struct {
	id:      AuthId,
	secrets: map[string]string,
}

auth_new :: proc(id: AuthId) -> AuthSet {
	return AuthSet{id = id, secrets = make(map[string]string)}
}

// auth_with adds a secret, builder-style.
auth_with :: proc(auth: AuthSet, key, value: string) -> AuthSet {
	auth := auth
	auth.secrets[key] = value
	return auth
}

// auth_get reads a secret by key. ok is false when absent.
auth_get :: proc(auth: ^AuthSet, key: string) -> (value: string, ok: bool) {
	value, ok = auth.secrets[key]
	return
}

auth_destroy :: proc(auth: ^AuthSet) {
	delete(auth.secrets)
}
