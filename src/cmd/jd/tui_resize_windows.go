//go:build windows

package main

import (
	"time"

	"github.com/yesitsfebreeze/justdown/src/editor"
)

// watchResize polls the terminal size — Windows has no SIGWINCH.
func watchResize(resize chan<- editor.Size) {
	go func() {
		w, h := tuiSize()
		for range time.Tick(250 * time.Millisecond) {
			c, r := tuiSize()
			if c == w && r == h {
				continue
			}
			w, h = c, r
			select {
			case resize <- editor.Size{W: c, H: r}:
			default:
			}
		}
	}()
}
