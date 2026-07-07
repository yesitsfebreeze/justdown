package editor

import (
	"path/filepath"
	"regexp"
	"strings"
)

var createNameRE = regexp.MustCompile(`^[\w .\-/]+$`)

type finder struct {
	active     bool
	query      []rune
	hits       []finderHit
	sel        int
	total      int
	canCreate  bool
	createName string
	createDir  string
}

func (f *finder) open(lib *library, curDir string) {
	f.active = true
	f.query = nil
	f.sel = 0
	f.createDir = curDir
	f.refresh(lib)
}

func (f *finder) close() {
	f.active = false
	f.query = nil
	f.hits = nil
	f.sel = 0
}

func (f *finder) refresh(lib *library) {
	q := string(f.query)
	f.hits = lib.search(q, 50)
	f.total = len(f.hits)

	trimmed := strings.TrimSpace(q)
	f.canCreate = false
	if trimmed != "" && createNameRE.MatchString(trimmed) {
		base := strings.TrimSuffix(trimmed, ".jd")
		want := base + ".jd"
		exists := false
		for _, h := range f.hits {
			if h.name == want {
				exists = true
				break
			}
		}
		if !exists {
			f.canCreate = true
			f.createName = base
		}
	}
	if f.sel > f.maxIndex() {
		f.sel = f.maxIndex()
	}
	if f.sel < 0 {
		f.sel = 0
	}
}

func (f *finder) maxIndex() int {
	n := len(f.hits) - 1
	if f.canCreate {
		n = len(f.hits)
	}
	return n
}

func (f *finder) move(d int) {
	f.sel += d
	if f.sel > f.maxIndex() {
		f.sel = f.maxIndex()
	}
	if f.sel < 0 {
		f.sel = 0
	}
}

// choose reports the selection: either an existing hit path, or a request to
// create a new file (createPath set, its parent dirs made on save).
func (f *finder) choose() (openPath string, createPath string) {
	if f.canCreate && f.sel == len(f.hits) {
		name := f.createName
		if !strings.HasSuffix(name, ".jd") {
			name += ".jd"
		}
		if f.createDir != "" {
			return "", filepath.Join(f.createDir, name)
		}
		return "", name
	}
	if f.sel >= 0 && f.sel < len(f.hits) {
		return f.hits[f.sel].path, ""
	}
	return "", ""
}

func (f *finder) typeRune(r rune, lib *library) {
	f.query = append(f.query, r)
	f.refresh(lib)
}

func (f *finder) backspace(lib *library) {
	if len(f.query) > 0 {
		f.query = f.query[:len(f.query)-1]
		f.refresh(lib)
	}
}

const newFileTemplate = "---\nname: %s\nkind: tool\ndescription: \n---\n\n# %s\n\n"
