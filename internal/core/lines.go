package core

import (
	"bufio"
	"io"
)

// ReadLineLimit reads one line without its trailing newline. A line longer than limit
// is discarded through its newline and not kept: skipped is true and line is nil.
// err is io.EOF at the end, including when the last line had no newline (line then
// holds it) or a discarded line ran to the end of the reader.
//
// Agent tools put a whole tool result on one line. Buffering that line without a cap
// holds it, the parsed JSON and every copy made while clipping it, and the process is
// killed. Callers skip a discarded line and keep reading.
func ReadLineLimit(r *bufio.Reader, limit int) (line []byte, skipped bool, err error) {
	var buf []byte
	over := false
	for {
		chunk, e := r.ReadSlice('\n')
		if len(chunk) == 0 && e == nil {
			e = io.EOF
		}
		if !over {
			if len(buf)+len(chunk) > limit {
				over = true
				buf = nil
			} else {
				buf = append(buf, chunk...)
			}
		}
		if e == bufio.ErrBufferFull {
			continue
		}
		if over {
			return nil, true, e
		}
		if len(buf) == 0 {
			return nil, false, e
		}
		if buf[len(buf)-1] == '\n' {
			buf = buf[:len(buf)-1]
			if len(buf) > 0 && buf[len(buf)-1] == '\r' {
				buf = buf[:len(buf)-1]
			}
		}
		return buf, false, e
	}
}
