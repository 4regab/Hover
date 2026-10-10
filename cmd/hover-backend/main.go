// Command hover-backend is the Mac app's agent backend: the process its Swift UI starts with
// HOVER_DATA_DIR set, speaking JSON lines on stdin and stdout (crates/hover-backend).
package main

import (
	"os"

	"github.com/4regab/Hover/internal/backend"
)

func main() {
	backend.Run(os.Stdin, backend.NewOut(os.Stdout))
}
