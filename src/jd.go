package justdown

import (
	"strings"

	"gopkg.in/yaml.v3"
)

type Node struct {
	Key            string
	Name           string
	Kind           string
	Description    string
	Purpose        string
	Tags           []string
	Path           string // path relative to root, including the lib dir prefix
	UseWhen        []string
	NotWhen        []string
	Danger         string
	SideEffects    []string
	Requires       []string
	Category       string
	Run            string
	HasFrontmatter bool
	NameGiven      bool
	Links          []string
}

type frontmatter struct {
	Name        *string  `yaml:"name"`
	Kind        string   `yaml:"kind"`
	Description string   `yaml:"description"`
	Tags        []string `yaml:"tags"`
	UseWhen     []string `yaml:"use_when"`
	NotWhen     []string `yaml:"not_when"`
	Danger      string   `yaml:"danger"`
	SideEffects []string `yaml:"side_effects"`
	Requires    []string `yaml:"requires"`
	Run         string   `yaml:"run"`
}

func splitFrontmatter(content string) (fm string, body string, has bool) {
	rest, ok := strings.CutPrefix(content, "---\n")
	if !ok {
		rest, ok = strings.CutPrefix(content, "---\r\n")
	}
	if !ok {
		return "", content, false
	}
	idx := 0
	for idx < len(rest) {
		end := strings.IndexByte(rest[idx:], '\n')
		var line string
		var next int
		if end < 0 {
			line = rest[idx:]
			next = len(rest)
		} else {
			line = rest[idx : idx+end]
			next = idx + end + 1
		}
		bare := strings.TrimSuffix(line, "\r")
		if bare == "---" {
			return rest[:idx], rest[next:], true
		}
		idx = next
	}
	return "", content, false
}

func isWordByte(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '-'
}

func scanLinks(line string, out *[]string) {
	b := line
	inCode := false
	i := 0
	for i < len(b) {
		if b[i] == '`' {
			inCode = !inCode
			i++
			continue
		}
		if inCode || b[i] != '@' {
			i++
			continue
		}
		j := i + 1
		fuzzy := j < len(b) && b[j] == '?'
		if fuzzy {
			j++
		}
		s1 := j
		for j < len(b) && isWordByte(b[j]) {
			j++
		}
		if j == s1 {
			i++ // bare `@` / `@?` with no word — not a link
			continue
		}
		end := j
		if !fuzzy && j < len(b) && b[j] == '/' {
			s2 := j + 1
			k := s2
			for k < len(b) && isWordByte(b[k]) {
				k++
			}
			if k > s2 {
				end = k // `dir/name`
			}
		}
		var token string
		if fuzzy {
			token = "?" + b[s1:end]
		} else {
			token = b[i+1 : end]
		}
		found := false
		for _, t := range *out {
			if t == token {
				found = true
				break
			}
		}
		if !found {
			*out = append(*out, token)
		}
		i = end
	}
}

func KeyAndCategory(rel string) (string, string) {
	p := strings.TrimSuffix(rel, ".jd")
	parts := strings.Split(p, "/")
	n := len(parts)
	if n >= 2 {
		return parts[n-2] + "/" + parts[n-1], parts[n-2]
	}
	return parts[n-1], ""
}

func Parse(rel, content string) Node {
	key, category := KeyAndCategory(rel)

	fmText, body, hasFrontmatter := splitFrontmatter(content)
	var fm frontmatter
	if hasFrontmatter && strings.TrimSpace(fmText) != "" {
		if err := yaml.Unmarshal([]byte(fmText), &fm); err != nil {
			fm = frontmatter{}
		}
	}

	var links []string
	inFence := false
	for _, line := range Lines(body) {
		if strings.HasPrefix(strings.TrimLeft(line, " \t"), "```") {
			inFence = !inFence
			continue
		}
		if !inFence {
			scanLinks(line, &links)
		}
	}

	nameGiven := fm.Name != nil && strings.TrimSpace(*fm.Name) != ""
	name := key
	if nameGiven {
		name = *fm.Name
	}
	purpose := name
	if fm.Description != "" {
		purpose = fm.Description
	}

	return Node{
		Key:            key,
		Name:           name,
		Kind:           fm.Kind,
		Description:    fm.Description,
		Purpose:        purpose,
		Tags:           fm.Tags,
		Path:           rel,
		UseWhen:        fm.UseWhen,
		NotWhen:        fm.NotWhen,
		Danger:         fm.Danger,
		SideEffects:    fm.SideEffects,
		Requires:       fm.Requires,
		Category:       category,
		Run:            fm.Run,
		HasFrontmatter: hasFrontmatter,
		NameGiven:      nameGiven,
		Links:          links,
	}
}
