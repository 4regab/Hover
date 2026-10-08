//go:build unix && !linux

package agents

import "syscall"

func deathSignal(*syscall.SysProcAttr) {}
