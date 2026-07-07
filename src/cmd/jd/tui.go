package main

import (
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	uv "github.com/charmbracelet/ultraviolet"
	justdown "github.com/yesitsfebreeze/justdown/src"
	"golang.org/x/term"
)

type app struct {
	lib  *library
	ed   *editor
	fnd  *finder
	pop  *linkpop
	fbar findBar
	rg   ripgrep

	cols, rows int
	scroll     int // first visible visual row

	status      string
	statusUntil time.Time

	pasting  bool
	pasteBuf strings.Builder

	confirmDelete bool

	clipboard string // OSC52 payload to emit on next paint

	quit  chan struct{}
	dirty chan struct{}
	fb    uv.ScreenBuffer
}

func cmdTUI(cfg *config, args []string) int {
	roots := tuiRoots(cfg, args)
	if !term.IsTerminal(int(os.Stdin.Fd())) {
		fmt.Fprintln(os.Stderr, "jd: the editor needs an interactive terminal")
		return 1
	}

	a := &app{
		lib:   newLibrary(cfg, roots),
		ed:    newEditor(),
		fnd:   &finder{},
		pop:   &linkpop{},
		quit:  make(chan struct{}),
		dirty: make(chan struct{}, 1),
	}
	a.lib.reindex()

	old, err := term.MakeRaw(int(os.Stdin.Fd()))
	if err != nil {
		fmt.Fprintln(os.Stderr, "jd: raw mode failed:", err)
		return 1
	}
	defer term.Restore(int(os.Stdin.Fd()), old)

	// alt screen, hide cursor, disable auto-wrap, enable bracketed paste,
	// then ask for disambiguated key reports (kitty CSI-u + xterm
	// modifyOtherKeys) so ctrl+shift+<key> is distinguishable from ctrl+<key>.
	fmt.Print("\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?2004h\x1b[>1u\x1b[>4;1m")
	restore := func() { fmt.Print("\x1b[>4;0m\x1b[<u\x1b[?2004l\x1b[?7h\x1b[?25h\x1b[?1049l") }
	defer restore()

	a.cols, a.rows = tuiSize()
	a.applyLayout()

	// open a file to start on: the most recently modified.
	if hits := a.lib.search("", 1); len(hits) > 0 {
		a.openFile(hits[0].path)
	} else {
		a.setStatus("no .jd files found — ctrl+k to create one")
	}

	r := uv.NewTerminalRenderer(os.Stdout, os.Environ())
	r.Resize(a.cols, a.rows)

	input := make(chan []byte, 16)
	go func() {
		buf := make([]byte, 4096)
		for {
			n, err := os.Stdin.Read(buf)
			if n > 0 {
				input <- append([]byte(nil), buf[:n]...)
			}
			if err != nil {
				close(input)
				return
			}
		}
	}()

	stopWatch := make(chan struct{})
	go a.lib.watch(stopWatch, a.markDirty)
	defer close(stopWatch)

	winch := make(chan os.Signal, 1)
	signal.Notify(winch, syscall.SIGWINCH)

	frameCap := time.NewTicker(8 * time.Millisecond)
	defer frameCap.Stop()
	clock := time.NewTicker(time.Second)
	defer clock.Stop()

	relayout := func() {
		c, rw := tuiSize()
		if c == a.cols && rw == a.rows {
			return
		}
		a.cols, a.rows = c, rw
		a.applyLayout()
		r.Resize(c, rw)
		r.Erase()
	}

	a.paint(r)
	pending := false
	for {
		select {
		case data, ok := <-input:
			if !ok {
				return 0
			}
			a.handleBytes(data)
			pending = true
		case <-winch:
			relayout()
			pending = true
		case <-clock.C:
			relayout()
			if !a.statusUntil.IsZero() && time.Now().After(a.statusUntil) {
				a.status = ""
				a.statusUntil = time.Time{}
			}
			pending = true
		case <-a.dirty:
			pending = true
		case <-a.quit:
			return 0
		case <-frameCap.C:
			if pending {
				a.paint(r)
				pending = false
			}
		}
	}
}

func (a *app) markDirty() {
	select {
	case a.dirty <- struct{}{}:
	default:
	}
}

func (a *app) applyLayout() {
	a.ed.width = a.editorW()
}

// layout: row 0 search/title bar, rows 1..rows-2 editor, last row status.
func (a *app) editorX() int { return 1 }
func (a *app) editorY() int { return 1 }
func (a *app) editorW() int {
	w := a.cols - 2
	if w < 1 {
		w = 1
	}
	return w
}
func (a *app) editorH() int {
	h := a.rows - 2
	if h < 1 {
		h = 1
	}
	return h
}

func (a *app) setStatus(s string) {
	a.status = s
	a.statusUntil = time.Now().Add(4 * time.Second)
}

func tuiSize() (int, int) {
	w, h, err := term.GetSize(int(os.Stdout.Fd()))
	if err != nil || w == 0 {
		return 80, 24
	}
	return w, h
}

func tuiRoots(cfg *config, args []string) []string {
	for _, a := range args {
		if r, ok := strings.CutPrefix(a, "--root="); ok && r != "" {
			return []string{r}
		}
	}
	if r := os.Getenv("JD_ROOT"); r != "" {
		return []string{r}
	}
	if cwd, err := os.Getwd(); err == nil {
		return []string{cwd}
	}
	return []string{"."}
}

// ---- input dispatch -----------------------------------------------------

func (a *app) handleBytes(data []byte) {
	for _, k := range parseKeys(data) {
		if a.pasting {
			switch k.kind {
			case kPasteEnd:
				a.pasting = false
				a.applyPaste(a.pasteBuf.String())
				a.pasteBuf.Reset()
			case kRune:
				a.pasteBuf.WriteRune(k.r)
			case kEnter:
				a.pasteBuf.WriteByte('\n')
			case kTab:
				a.pasteBuf.WriteByte('\t')
			}
			continue
		}
		if k.kind == kPasteStart {
			a.pasting = true
			a.pasteBuf.Reset()
			continue
		}
		a.dispatch(k)
	}
	a.markDirty()
}

func (a *app) overlayActive() bool {
	return a.confirmDelete || a.fnd.active || a.rg.active || a.fbar.active || a.pop.active
}

// applyPaste inserts pasted text into the editor, or routes it to whichever
// overlay owns the keyboard (so a paste into the finder/find/grep query lands
// there, not in the hidden document).
func (a *app) applyPaste(s string) {
	if s == "" {
		return
	}
	if a.overlayActive() {
		for _, r := range s {
			if r == '\n' || r == '\r' || r == '\t' {
				continue
			}
			a.dispatch(key{kind: kRune, r: r})
		}
		return
	}
	a.ed.insertText(s)
	a.afterEdit()
	a.updatePopup()
}

func (a *app) dispatch(k key) {
	switch {
	case a.confirmDelete:
		a.keyConfirm(k)
	case a.fnd.active:
		a.keyFinder(k)
	case a.rg.active:
		a.keyRg(k)
	case a.fbar.active:
		a.keyFind(k)
	case a.pop.active:
		a.keyPopup(k)
	default:
		a.keyEditor(k)
	}
}

func (a *app) keyConfirm(k key) {
	switch {
	case k.kind == kRune && (k.r == 'y' || k.r == 'Y'):
		a.doDelete()
	case k.kind == kEsc, k.kind == kRune && (k.r == 'n' || k.r == 'N'):
		a.confirmDelete = false
	case k.kind == kEnter:
		a.confirmDelete = false // default: no
	}
}

func (a *app) keyFinder(k key) {
	switch k.kind {
	case kEsc:
		a.fnd.close()
	case kUp:
		a.fnd.move(-1)
	case kDown:
		a.fnd.move(1)
	case kEnter:
		openPath, createPath := a.fnd.choose()
		a.fnd.close()
		if createPath != "" {
			a.createFile(createPath)
		} else if openPath != "" {
			a.openFile(openPath)
		}
	case kBackspace:
		a.fnd.backspace(a.lib)
	case kRune:
		if k.ctrl {
			switch k.r {
			case 'n':
				a.fnd.move(1)
			case 'p':
				a.fnd.move(-1)
			case 'k':
				a.fnd.close()
			}
			return
		}
		a.fnd.typeRune(k.r, a.lib)
	}
}

func (a *app) keyPopup(k key) {
	switch k.kind {
	case kEsc:
		a.pop.close()
	case kUp:
		a.pop.move(-1)
	case kDown:
		a.pop.move(1)
	case kEnter, kTab:
		a.acceptPopup()
	default:
		// any other key edits the buffer, then the token is re-evaluated.
		a.keyEditor(k)
		a.updatePopup()
	}
}

func (a *app) keyEditor(k key) {
	e := a.ed
	if k.kind == kRune && k.ctrl {
		a.editorCtrl(k)
		return
	}
	switch k.kind {
	case kRune:
		if k.alt {
			return // unbound alt-letter
		}
		e.insertRune(k.r)
		a.afterEdit()
		a.updatePopup()
	case kEnter:
		if e.inTable() && !e.selecting {
			e.tableEnter()
		} else {
			e.insertNewline()
		}
		a.afterEdit()
		a.pop.close()
	case kTab:
		if e.inTable() {
			e.tableTab(true)
		} else {
			e.insertText("  ")
		}
		a.afterEdit()
	case kBacktab:
		if e.inTable() {
			e.tableTab(false)
		}
	case kBackspace:
		e.backspace()
		a.afterEdit()
		a.updatePopup()
	case kDelete:
		e.deleteForward()
		a.afterEdit()
		a.updatePopup()
	case kLeft:
		if k.alt || k.ctrl {
			e.moveWordLeft(k.shift)
		} else {
			e.moveLeft(k.shift)
		}
		a.updatePopup()
	case kRight:
		if k.alt || k.ctrl {
			e.moveWordRight(k.shift)
		} else {
			e.moveRight(k.shift)
		}
		a.updatePopup()
	case kUp:
		e.moveUp(k.shift)
		a.pop.close()
	case kDown:
		e.moveDown(k.shift)
		a.pop.close()
	case kHome:
		if k.ctrl {
			e.moveDocStart(k.shift)
		} else {
			e.moveHome(k.shift)
		}
	case kEnd:
		if k.ctrl {
			e.moveDocEnd(k.shift)
		} else {
			e.moveEnd(k.shift)
		}
	case kPgUp:
		e.movePage(false, a.editorH()-1, k.shift)
	case kPgDn:
		e.movePage(true, a.editorH()-1, k.shift)
	case kEsc:
		e.selecting = false
	}
}

func (a *app) editorCtrl(k key) {
	switch k.r {
	case 'q':
		close(a.quit)
	case 'k':
		a.pop.close()
		a.fnd.open(a.lib, a.currentDir())
	case 's':
		a.saveFile()
	case 'z':
		a.ed.doUndo()
	case 'y':
		a.ed.doRedo()
	case 'c':
		a.copySelection()
	case 'x':
		a.cutSelection()
	case 'v':
		a.pasteRegister()
	case 'a':
		a.selectAll()
	case 'l':
		a.followLink()
	case 'f':
		if k.shift {
			a.openReplace()
		} else {
			a.openFind()
		}
	case 'g':
		a.openRg()
	case 'o':
		if a.ed.path != "" {
			revealInFileManager(a.ed.path)
			a.setStatus("revealed in file manager")
		}
	}
}

// ---- file operations ----------------------------------------------------

func (a *app) currentDir() string {
	if a.ed.path != "" {
		return filepath.Dir(a.ed.path)
	}
	if len(a.lib.roots) > 0 {
		return a.lib.roots[0]
	}
	return "."
}

func (a *app) openFile(path string) {
	b, err := os.ReadFile(path)
	if err != nil {
		a.setStatus("cannot open: " + filepath.Base(path))
		return
	}
	a.ed.setContent(string(b), path)
	a.pop.close()
	a.confirmDelete = false
}

func (a *app) createFile(path string) {
	base := strings.TrimSuffix(filepath.Base(path), ".jd")
	content := fmt.Sprintf(newFileTemplate, base, base)
	a.ed.setContent(content, path)
	a.ed.dirty = true
	a.pop.close()
}

func (a *app) withinRoots(p string) bool {
	clean := filepath.Clean(p)
	for _, r := range a.lib.roots {
		rc := filepath.Clean(r)
		if clean == rc || strings.HasPrefix(clean, rc+string(filepath.Separator)) {
			return true
		}
	}
	return false
}

func (a *app) saveFile() {
	if a.ed.path == "" {
		a.setStatus("no file to save")
		return
	}
	if !a.withinRoots(a.ed.path) {
		a.setStatus("refused: outside working roots")
		return
	}
	content := a.ed.contentToSave()
	if err := os.MkdirAll(filepath.Dir(a.ed.path), 0o755); err != nil {
		a.setStatus("save failed: " + err.Error())
		return
	}
	if err := os.WriteFile(a.ed.path, []byte(content), 0o644); err != nil {
		a.setStatus("save failed: " + err.Error())
		return
	}
	a.ed.loadedRaw = content
	a.ed.dirty = false
	a.setStatus("saved " + displayPath(a.ed.path))
	go func() {
		a.lib.reindex()
		a.markDirty()
	}()
}

func (a *app) afterEdit() {
	// delete-on-empty: clearing a non-empty file asks to delete it.
	if strings.TrimSpace(a.ed.loadedRaw) != "" && strings.TrimSpace(a.ed.text()) == "" && a.ed.path != "" {
		a.confirmDelete = true
	} else {
		a.confirmDelete = false
	}
}

func (a *app) doDelete() {
	a.confirmDelete = false
	if a.ed.path == "" || !a.withinRoots(a.ed.path) {
		return
	}
	_ = os.Remove(a.ed.path)
	a.setStatus("deleted " + displayPath(a.ed.path))
	go func() {
		a.lib.reindex()
		a.markDirty()
	}()
	if hits := a.lib.search("", 1); len(hits) > 0 {
		a.openFile(hits[0].path)
	} else {
		a.ed.setContent("", "")
	}
}

// ---- clipboard ----------------------------------------------------------

func (a *app) copySelection() {
	if s := a.ed.selectedText(); s != "" {
		a.ed.register = s
		a.clipboard = osc52(s)
		a.setStatus("copied")
	}
}

func (a *app) cutSelection() {
	if s := a.ed.selectedText(); s != "" {
		a.ed.register = s
		a.clipboard = osc52(s)
		a.ed.pushUndo("")
		a.ed.deleteSelection()
		a.ed.markDirty()
		a.afterEdit()
	}
}

func (a *app) pasteRegister() {
	if a.ed.register != "" {
		a.ed.insertText(a.ed.register)
		a.afterEdit()
	}
}

func (a *app) selectAll() {
	a.ed.anchor = pos{}
	a.ed.cur = pos{row: len(a.ed.lines) - 1, col: len(a.ed.lines[len(a.ed.lines)-1])}
	a.ed.selecting = true
	a.ed.lastKind = ""
}

// ---- links --------------------------------------------------------------

func (a *app) updatePopup() {
	needle, fuzzy, start, ok := tokenAtCaret(a.ed)
	if !ok {
		a.pop.close()
		return
	}
	matches, _ := a.lib.resolve(needle, fuzzy, 12)
	if len(matches) == 0 {
		a.pop.close()
		return
	}
	a.pop.active = true
	a.pop.fuzzy = fuzzy
	a.pop.needle = needle
	a.pop.matches = matches
	a.pop.tokRow = a.ed.cur.row
	a.pop.tokStart = start
	a.pop.tokEnd = a.ed.cur.col
	if a.pop.sel >= len(matches) {
		a.pop.sel = 0
	}
}

func (a *app) acceptPopup() {
	if a.pop.sel < 0 || a.pop.sel >= len(a.pop.matches) {
		a.pop.close()
		return
	}
	row := a.pop.matches[a.pop.sel]
	if a.pop.fuzzy {
		// fuzzy: navigate, leave the source text literal.
		a.pop.close()
		a.followRow(row)
		return
	}
	// direct: rewrite the token to @leaf.
	leaf := justdown.Leaf(row.Key)
	a.rewriteToken("@" + leaf)
	a.pop.close()
}

func (a *app) rewriteToken(replacement string) {
	e := a.ed
	if a.pop.tokRow != e.cur.row {
		return
	}
	line := e.lines[e.cur.row]
	start := a.pop.tokStart
	end := e.cur.col
	if start < 0 || start > len(line) || end > len(line) || start > end {
		return
	}
	e.pushUndo("")
	repl := []rune(replacement)
	nl := make([]rune, 0, len(line)-(end-start)+len(repl))
	nl = append(nl, line[:start]...)
	nl = append(nl, repl...)
	nl = append(nl, line[end:]...)
	e.lines[e.cur.row] = nl
	e.cur.col = start + len(repl)
	e.goalCol = e.cur.col
	e.markDirty()
}

func (a *app) followLink() {
	sp, ok := linkAtCaret(a.ed, a.lib.isTarget)
	if !ok {
		a.setStatus("no link under cursor")
		return
	}
	switch sp.kind {
	case linkMarkdown:
		a.followMarkdown(sp.token)
	default:
		matches, _ := a.lib.resolve(sp.token, sp.fuzzy, 1)
		if len(matches) == 0 {
			a.setStatus("unresolved @" + sp.token)
			return
		}
		a.followRow(matches[0])
	}
}

func (a *app) followRow(row *justdown.Row) {
	if p, ok := a.lib.localPathForRow(row); ok {
		a.openFile(p)
		return
	}
	a.setStatus(fmt.Sprintf("%s — remote capability (read-only)", row.Key))
}

func (a *app) followMarkdown(target string) {
	if strings.HasPrefix(target, "http://") || strings.HasPrefix(target, "https://") {
		openURL(target)
		a.setStatus("opened in browser")
		return
	}
	base := a.currentDir()
	p := filepath.Clean(filepath.Join(base, target))
	if _, err := os.Stat(p); err != nil {
		a.setStatus("not found: " + target)
		return
	}
	a.openFile(p)
}

func osc52(s string) string {
	return "\x1b]52;c;" + b64(s) + "\x07"
}
