package voice

import (
	"math"
	"os"
)

func sqrt(v float64) float64 { return math.Sqrt(v) }

func isDir(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.IsDir()
}
