// Command gofixture is the Go side of the compatibility checks in windows/Hover.Tests. It
// writes and reads a secret through the code the Go app uses (internal/core), so the tests
// can show that the Go app and the C# app open each other's secrets.dat and note.key.
// cmd/hover-data covers the settings and the history; it has nothing for secrets.
//
// The data folder is HOVER_DATA_DIR, as everywhere else.
//
//	gofixture secrets-set <name> <value>
//	gofixture secrets-get <name>          prints the value
package main

import (
	"fmt"
	"os"

	"github.com/4regab/Hover/internal/core"
)

func main() {
	a := os.Args
	switch {
	case len(a) == 4 && a[1] == "secrets-set":
		v := a[3]
		stored, err := core.SystemSecrets().Set(a[2], &v)
		if err != nil || stored != core.Saved {
			fmt.Fprintln(os.Stderr, "the secret was not saved", err)
			os.Exit(1)
		}
	case len(a) == 3 && a[1] == "secrets-get":
		v, ok := core.SystemSecrets().Get(a[2])
		if !ok {
			fmt.Fprintln(os.Stderr, "no such secret")
			os.Exit(1)
		}
		fmt.Println(v)
	default:
		fmt.Fprintln(os.Stderr, "gofixture secrets-set <name> <value> | secrets-get <name>")
		os.Exit(2)
	}
}
