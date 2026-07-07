package editor

import "strings"

// GFM table editing: Tab / Shift+Tab move between cells, Enter inserts a data
// row (or exits below an empty one). A table row is a prose line whose trimmed
// text starts with '|'.

func isTableRow(line []rune) bool {
	s := strings.TrimSpace(string(line))
	return strings.HasPrefix(s, "|") && strings.Contains(s[1:], "|")
}

func isSeparatorRow(line []rune) bool {
	s := strings.TrimSpace(string(line))
	if !strings.HasPrefix(s, "|") {
		return false
	}
	for _, r := range s {
		if r != '|' && r != '-' && r != ':' && r != ' ' {
			return false
		}
	}
	return strings.Contains(s, "-")
}

func (e *editor) inTable() bool {
	if e.cur.row >= len(e.lines) {
		return false
	}
	return isTableRow(e.lines[e.cur.row])
}

// pipeCols returns the rune columns of every '|' on the line.
func pipeCols(line []rune) []int {
	var out []int
	for i, r := range line {
		if r == '|' {
			out = append(out, i)
		}
	}
	return out
}

// cellStart returns the caret column for cell index c on a table line (just
// past the pipe, skipping one padding space).
func cellStart(line []rune, pipes []int, c int) int {
	if c < 0 || c >= len(pipes)-1 {
		return -1
	}
	col := pipes[c] + 1
	if col < len(line) && line[col] == ' ' {
		col++
	}
	return col
}

// currentCell returns which cell the caret is in (region between pipe c and c+1).
func currentCell(pipes []int, col int) int {
	c := 0
	for i := 0; i < len(pipes)-1; i++ {
		if col > pipes[i] {
			c = i
		}
	}
	return c
}

// tableBounds returns the first and last row index of the table block at row.
func (e *editor) tableBounds(row int) (int, int) {
	start, end := row, row
	for start > 0 && isTableRow(e.lines[start-1]) {
		start--
	}
	for end < len(e.lines)-1 && isTableRow(e.lines[end+1]) {
		end++
	}
	return start, end
}

// reformatTable re-pads every column of the table block to canonical width. It
// is a no-op (no undo/dirty) when the block is already aligned.
func (e *editor) reformatTable(row int) {
	start, end := e.tableBounds(row)
	type rc struct {
		cells []string
		sep   bool
	}
	var rows []rc
	ncol := 0
	for r := start; r <= end; r++ {
		line := e.lines[r]
		pipes := pipeCols(line)
		n := len(pipes) - 1
		// the separator is canonically the block's second row; requiring the
		// position avoids mangling a data row whose cells are only dashes.
		sep := isSeparatorRow(line) && r == start+1
		cells := make([]string, n)
		for i := 0; i < n; i++ {
			cells[i] = strings.TrimSpace(string(line[pipes[i]+1 : pipes[i+1]]))
		}
		rows = append(rows, rc{cells, sep})
		if n > ncol {
			ncol = n
		}
	}
	width := make([]int, ncol)
	for _, r := range rows {
		if r.sep {
			continue
		}
		for i, c := range r.cells {
			if w := len([]rune(c)); w > width[i] {
				width[i] = w
			}
		}
	}
	for i := range width {
		if width[i] < 3 {
			width[i] = 3
		}
	}
	newLines := make([][]rune, len(rows))
	for ri, r := range rows {
		var b strings.Builder
		b.WriteByte('|')
		for i := 0; i < ncol; i++ {
			b.WriteByte(' ')
			if r.sep {
				b.WriteString(strings.Repeat("-", width[i]))
			} else {
				cell := ""
				if i < len(r.cells) {
					cell = r.cells[i]
				}
				b.WriteString(cell)
				b.WriteString(strings.Repeat(" ", width[i]-len([]rune(cell))))
			}
			b.WriteString(" |")
		}
		newLines[ri] = []rune(b.String())
	}
	changed := false
	for ri, r := 0, start; r <= end; ri, r = ri+1, r+1 {
		if string(newLines[ri]) != string(e.lines[r]) {
			changed = true
			break
		}
	}
	if !changed {
		return
	}
	e.pushUndo("table")
	for ri, r := 0, start; r <= end; ri, r = ri+1, r+1 {
		e.lines[r] = newLines[ri]
	}
	e.markDirty()
}

func (e *editor) tableTab(forward bool) {
	c := currentCell(pipeCols(e.lines[e.cur.row]), e.cur.col)
	e.reformatTable(e.cur.row)
	line := e.lines[e.cur.row]
	pipes := pipeCols(line)
	ncells := len(pipes) - 1
	if ncells < 1 {
		return
	}
	if forward {
		if c+1 < ncells {
			e.cur.col = cellStart(line, pipes, c+1)
		} else {
			e.tableToRow(e.cur.row+1, true)
		}
	} else {
		if c > 0 {
			e.cur.col = cellStart(line, pipes, c-1)
		} else {
			e.tableToRow(e.cur.row-1, false)
		}
	}
	e.goalCol = e.cur.col
	e.selecting = false
	e.lastKind = ""
}

// tableToRow moves into an adjacent table row, skipping the separator row, and
// lands on its first (or last) cell.
func (e *editor) tableToRow(row int, first bool) {
	for row >= 0 && row < len(e.lines) && isSeparatorRow(e.lines[row]) {
		if first {
			row++
		} else {
			row--
		}
	}
	if row < 0 || row >= len(e.lines) || !isTableRow(e.lines[row]) {
		return
	}
	e.cur.row = row
	line := e.lines[row]
	pipes := pipeCols(line)
	ncells := len(pipes) - 1
	if first {
		e.cur.col = cellStart(line, pipes, 0)
	} else {
		e.cur.col = cellStart(line, pipes, ncells-1)
	}
}

// tableEnter inserts an empty data row below the current one; if the current
// row is already empty, it is removed and the caret exits below the table.
func (e *editor) tableEnter() {
	line := e.lines[e.cur.row]
	pipes := pipeCols(line)
	ncells := len(pipes) - 1
	if ncells < 1 {
		e.insertNewline()
		return
	}
	if rowIsEmpty(line, pipes) && !isSeparatorRow(line) {
		// drop this empty row and move below the table.
		e.pushUndo("newline")
		e.lastKind = ""
		e.lines = append(e.lines[:e.cur.row], e.lines[e.cur.row+1:]...)
		if e.cur.row >= len(e.lines) {
			e.lines = append(e.lines, []rune{})
		}
		e.cur.col = 0
		e.goalCol = 0
		e.markDirty()
		return
	}
	e.pushUndo("newline")
	e.lastKind = ""
	var nb strings.Builder
	nb.WriteString("|")
	for i := 0; i < ncells; i++ {
		nb.WriteString("  |")
	}
	newRow := []rune(nb.String())
	rest := append([][]rune{newRow}, e.lines[e.cur.row+1:]...)
	e.lines = append(e.lines[:e.cur.row+1], rest...)
	e.cur.row++
	e.reformatTable(e.cur.row)
	line = e.lines[e.cur.row]
	np := pipeCols(line)
	e.cur.col = cellStart(line, np, 0)
	e.goalCol = e.cur.col
	e.selecting = false
	e.markDirty()
}

func rowIsEmpty(line []rune, pipes []int) bool {
	for i := 0; i < len(pipes)-1; i++ {
		seg := strings.TrimSpace(string(line[pipes[i]+1 : pipes[i+1]]))
		if seg != "" {
			return false
		}
	}
	return true
}
