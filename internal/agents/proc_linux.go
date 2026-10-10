package agents

import "syscall"

func deathSignal(a *syscall.SysProcAttr) { a.Pdeathsig = syscall.SIGKILL }
