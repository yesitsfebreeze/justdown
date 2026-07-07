//go:build !windows

package main

import (
	"os"
	"os/signal"
	"syscall"

	"github.com/yesitsfebreeze/justdown/src/editor"
)

// watchResize forwards terminal size changes to the editor via SIGWINCH.
func watchResize(resize chan<- editor.Size) {
	winch := make(chan os.Signal, 1)
	signal.Notify(winch, syscall.SIGWINCH)
	go func() {
		for range winch {
			c, r := tuiSize()
			select {
			case resize <- editor.Size{W: c, H: r}:
			default:
			}
		}
	}()
}
