package agents

import (
	"os/exec"
	"syscall"
	"unsafe"

	"github.com/4regab/Hover/go/internal/core"
	"golang.org/x/sys/windows"
)

func hideWindow(c *exec.Cmd) {
	c.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
}

func prepare(*exec.Cmd) {}

func spawn(cmd *exec.Cmd) error { return cmd.Start() }

// groupImp is one job per tool, killing everything in it when its last handle closes:
// when Hover exits, however it exits (ChildJob, which C# keeps as one job for all).
type groupImp struct{ job windows.Handle }

func attach(pid int) (*groupImp, error) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	var info windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION
	info.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&info)), uint32(unsafe.Sizeof(info))); err != nil {
		windows.CloseHandle(job)
		return nil, err
	}
	// Rust has the child's own handle; Go opens one with the rights a job needs.
	p, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(pid))
	if err == nil {
		err = windows.AssignProcessToJobObject(job, p)
		windows.CloseHandle(p)
	}
	if err != nil {
		core.Logf("child job: couldn't add pid %d (%v)", pid, err)
	}
	return &groupImp{job}, nil
}

func (g *groupImp) kill() { windows.TerminateJobObject(g.job, 1) }

// release: the job stays open (it ends with Hover); only a Mac's Spaces release a group.
func (g *groupImp) release() {}

func (g *groupImp) close() { windows.CloseHandle(g.job) }
