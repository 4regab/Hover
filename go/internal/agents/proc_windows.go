package agents

import (
	"errors"
	"os/exec"
	"strings"
	"syscall"
	"unicode"
	"unsafe"

	"github.com/4regab/Hover/go/internal/core"
	"golang.org/x/sys/windows"
)

func hideWindow(c *exec.Cmd) {
	c.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
}

// batCommand runs a .cmd or .bat as Rust's std does (make_bat_command_line): through
// System32's cmd.exe, each argument quoted and escaped for cmd, and refused (the command's
// Err) when it can't be passed as plain data. Go itself would hand cmd the arguments as
// they are.
func batCommand(script string, args []string) *exec.Cmd {
	sys, _ := windows.GetSystemDirectory()
	c := exec.Command(sys + `\cmd.exe`)
	hideWindow(c)
	var b strings.Builder
	b.WriteString(`cmd.exe /e:ON /v:OFF /d /c "`)
	if strings.Contains(script, `"`) || strings.HasSuffix(script, `\`) {
		c.Err = errors.New("Windows file names may not contain `\"` or end with `\\`")
		return c
	}
	b.WriteString(`"` + script + `"`)
	for _, a := range args {
		b.WriteByte(' ')
		if strings.ContainsAny(a, "\r\n") {
			c.Err = errors.New("batch file arguments are invalid")
			return c
		}
		if strings.ContainsRune(a, 0) {
			c.Err = errors.New("nul byte found in provided data")
			return c
		}
		appendBatArg(&b, a)
	}
	b.WriteByte('"')
	c.SysProcAttr.CmdLine = b.String()
	return c
}

// appendBatArg is Rust's append_bat_arg: quoted when anything in it but letters, digits
// and #$*+-./:?@\_ (or a control character) could mean something to cmd; a quote doubled,
// and % made one cmd won't expand.
func appendBatArg(b *strings.Builder, arg string) {
	quote := arg == "" || strings.HasSuffix(arg, `\`)
	for _, c := range arg {
		if c < 0x80 && !(asciiAlnum(c) || strings.ContainsRune(`#$*+-./:?@\_`, c)) || unicode.IsControl(c) {
			quote = true
		}
	}
	if quote {
		b.WriteByte('"')
	}
	backslashes := 0
	for _, c := range arg {
		if c == '\\' {
			backslashes++
		} else {
			if c == '"' {
				// n backslashes to total 2n before an inner ", and the " doubled.
				b.WriteString(strings.Repeat(`\`, backslashes))
				b.WriteByte('"')
			} else if c == '%' || c == '\r' {
				// %%cd:~,% expands to nothing, which keeps cmd from expanding %VAR%.
				b.WriteString("%%cd:~,")
			}
			backslashes = 0
		}
		b.WriteRune(c)
	}
	if quote {
		b.WriteString(strings.Repeat(`\`, backslashes))
		b.WriteByte('"')
	}
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
