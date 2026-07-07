package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
)

func cmdMCP(_ []string) int {
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 1024*1024), 16*1024*1024)
	out := bufio.NewWriter(os.Stdout)

	for in.Scan() {
		line := in.Bytes()
		var msg map[string]json.RawMessage
		if json.Unmarshal(line, &msg) != nil {
			continue // not JSON — ignore rather than crash the stream
		}
		id, hasID := msg["id"]
		var method string
		_ = json.Unmarshal(msg["method"], &method)
		params := msg["params"]

		switch method {
		case "initialize":
			respond(out, id, hasID, initialize(params), nil)
		case "tools/list":
			respond(out, id, hasID, map[string]any{"tools": toolCatalogue()}, nil)
		case "tools/call":
			respond(out, id, hasID, toolCall(params), nil)
		case "ping":
			respond(out, id, hasID, map[string]any{}, nil)
		default:
			if strings.HasPrefix(method, "notifications/") {
				continue
			}
			if hasID {
				respond(out, id, hasID, nil,
					&rpcError{Code: -32601, Message: "method not found: " + method})
			}
		}
	}
	return 0
}

type rpcError struct {
	Code    int64  `json:"code"`
	Message string `json:"message"`
}

func respond(out *bufio.Writer, id json.RawMessage, hasID bool, result any, rpcErr *rpcError) {
	if !hasID {
		return
	}
	body := map[string]any{"jsonrpc": "2.0", "id": id}
	if rpcErr != nil {
		body["error"] = rpcErr
	} else {
		body["result"] = result
	}
	b, _ := json.Marshal(body)
	out.Write(b)
	out.WriteByte('\n')
	out.Flush()
}

const mcpProtocol = "2024-11-05"

func initialize(params json.RawMessage) map[string]any {
	pv := mcpProtocol
	var p struct {
		ProtocolVersion string `json:"protocolVersion"`
	}
	if json.Unmarshal(params, &p) == nil && p.ProtocolVersion != "" {
		pv = p.ProtocolVersion
	}
	return map[string]any{
		"protocolVersion": pv,
		"capabilities":    map[string]any{"tools": map[string]any{}},
		"serverInfo":      map[string]any{"name": "justdown", "version": cliVersion},
	}
}

func toolCatalogue() []map[string]any {
	obj := func(props map[string]any, required ...string) map[string]any {
		s := map[string]any{"type": "object", "properties": props}
		if len(required) > 0 {
			s["required"] = required
		}
		return s
	}
	str := func(desc string) map[string]any {
		return map[string]any{"type": "string", "description": desc}
	}
	return []map[string]any{
		{
			"name":        "search",
			"description": "Rank library .jd files by need (graph-aware: name/use_when > tags > prose; not_when vetoes), then append files the rank missed whose file name (fuzzy) or content matches every term at score 0 — the explorer's search semantics. Returns the best matches with purpose, kind, source tier and safety.",
			"inputSchema": obj(map[string]any{
				"query":    str("what you need to do, in plain words"),
				"kind":     map[string]any{"type": "string", "enum": []string{"tool", "agent", "knowledge", "workflow"}, "description": "narrow to one kind"},
				"limit":    map[string]any{"type": "integer", "minimum": 1, "description": "max results (default 5)"},
				"category": str("narrow to one category"),
				"mode":     map[string]any{"type": "string", "enum": []string{"exact", "semantic"}, "description": "exact substring rank (default) or synonym/stem-widened semantic rank"},
			}, "query"),
		},
		{
			"name":        "get",
			"description": "Read one .jd file as ordered sections, or a single output profile. ref = name | key | path | @dir/name.",
			"inputSchema": obj(map[string]any{
				"ref":     str("name, key (dir/name), path, or @dir/name"),
				"profile": map[string]any{"type": "string", "enum": []string{"default", "frontmatter", "human", "agent", "justfile"}, "description": "default = all sections; justfile needs kind tool|workflow"},
				"vars":    map[string]any{"type": "object", "description": "host values for <<var>> injection, name->value"},
			}, "ref"),
		},
		{
			"name":        "ls",
			"description": "List every category and its member files.",
			"inputSchema": obj(map[string]any{}),
		},
		{
			"name":        "links",
			"description": "Inbound + outbound @links of a file (one hop of the graph).",
			"inputSchema": obj(map[string]any{"ref": str("name, key, path, or @dir/name")}, "ref"),
		},
		{
			"name":        "resolve",
			"description": "Live @link completion. Direct: ranked key/name/leaf prefix matches for a @name link (reports the unique canonical key when one resolves). Fuzzy: the field-weighted ranker for a @?term link (one-to-many).",
			"inputSchema": obj(map[string]any{
				"query": str("the term after @ (or @?)"),
				"fuzzy": map[string]any{"type": "boolean", "description": "true for @?term ranking; false (default) for @name prefix"},
				"limit": map[string]any{"type": "integer", "minimum": 1, "description": "max matches (default 10)"},
			}, "query"),
		},
		{
			"name":        "path",
			"description": "Shortest @link connection between two files (undirected BFS).",
			"inputSchema": obj(map[string]any{"from": str(""), "to": str("")}, "from", "to"),
		},
	}
}

func toolCall(params json.RawMessage) map[string]any {
	var p struct {
		Name      string         `json:"name"`
		Arguments map[string]any `json:"arguments"`
	}
	_ = json.Unmarshal(params, &p)
	a := p.Arguments
	strArg := func(k string) string {
		s, _ := a[k].(string)
		return s
	}
	intArg := func(k string, def int64) int64 {
		if f, ok := a[k].(float64); ok && f > 0 {
			return int64(f)
		}
		return def
	}

	var argv []string
	switch p.Name {
	case "search":
		mode, _ := a["mode"].(string)
		if mode == "" {
			mode = "exact"
		}
		argv = []string{"search", strArg("query"), strArg("kind"),
			fmt.Sprint(intArg("limit", 5)), strArg("category"), "--mode", mode, "--json"}
	case "get":
		argv = []string{"get", strArg("ref")}
		if prof, _ := a["profile"].(string); prof != "" && prof != "default" {
			argv = append(argv, "--"+prof)
		}
		if vars, ok := a["vars"].(map[string]any); ok {
			for k, val := range vars {
				s, ok := val.(string)
				if !ok {
					b, _ := json.Marshal(val)
					s = string(b)
				}
				argv = append(argv, "--var", k+"="+s)
			}
		}
		argv = append(argv, "--json")
	case "ls":
		argv = []string{"ls", "--json"}
	case "links":
		argv = []string{"links", strArg("ref"), "--json"}
	case "resolve":
		argv = []string{"resolve", strArg("query"), fmt.Sprint(intArg("limit", 10))}
		if fz, _ := a["fuzzy"].(bool); fz {
			argv = append(argv, "--fuzzy")
		}
		argv = append(argv, "--json")
	case "path":
		argv = []string{"path", strArg("from"), strArg("to"), "--json"}
	default:
		return map[string]any{
			"content": []map[string]any{{"type": "text", "text": "unknown tool: " + p.Name}},
			"isError": true,
		}
	}

	code, stdout, stderr := callJD(argv)
	if code == 0 {
		return map[string]any{"content": []map[string]any{{"type": "text", "text": stdout}}}
	}
	text := stderr
	if strings.TrimSpace(text) == "" {
		text = stdout
	}
	return map[string]any{
		"content": []map[string]any{{"type": "text", "text": text}},
		"isError": true,
	}
}

func callJD(args []string) (int, string, string) {
	exe, err := os.Executable()
	if err != nil {
		exe = "jd"
	}
	cmd := exec.Command(exe, args...)
	var stdout, stderr []byte
	stdout, err = cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			stderr = exitErr.Stderr
			return exitErr.ExitCode(), string(stdout), string(stderr)
		}
		return -1, "", err.Error()
	}
	return 0, string(stdout), ""
}
