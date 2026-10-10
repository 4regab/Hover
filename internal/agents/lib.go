// Package agents is hover-agents: the agents, ported from Services/AcpHost.cs,
// KiroRunner.cs, Agents.cs, Owl/KiroSession.cs and KiroPage's state. No UI, shared by
// every view. One package, as the crate is one: its modules lean on each other.
package agents

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/4regab/Hover/internal/core"
)

// KiroModels is KiroRunner.Models: Settings → Kiro's models until a run has listed Kiro's own.
var KiroModels = [14][2]string{
	{"auto", "Auto"}, {"claude-opus-5.5", "Claude Opus 5.5"}, {"claude-opus-5", "Claude Opus 5"},
	{"claude-sonnet-5", "Claude Sonnet 5"}, {"claude-opus-4.8", "Claude Opus 4.8"}, {"claude-sonnet-4.6", "Claude Sonnet 4.6"},
	{"claude-haiku-4.5", "Claude Haiku 4.5"}, {"gpt-5.6-sol", "GPT-5.6 Sol"}, {"gpt-5.6-terra", "GPT-5.6 Terra"},
	{"gpt-5.6-luna", "GPT-5.6 Luna"}, {"deepseek-3.2", "DeepSeek 3.2"}, {"minimax-m2.5", "MiniMax M2.5"},
	{"glm-5", "GLM-5"}, {"qwen3-coder-next", "Qwen3 Coder Next"},
}

// UsableFolder is KiroRunner.UsableFolder: a full path to a directory that is there
// now. "" stands for none.
func UsableFolder(path string) bool {
	if strings.TrimSpace(path) == "" {
		return false
	}
	return FullyQualified(path) && isDir(path)
}

// FullyQualified is Path.IsPathFullyQualified: on Windows a drive with a separator
// (C:\) or a UNC path, not C:x or \x; on Unix a leading /.
func FullyQualified(p string) bool {
	if runtime.GOOS == "windows" {
		sep := func(c byte) bool { return c == '\\' || c == '/' }
		alpha := func(c byte) bool { return 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' }
		return len(p) >= 3 && alpha(p[0]) && p[1] == ':' && sep(p[2]) || len(p) >= 2 && sep(p[0]) && sep(p[1])
	}
	return strings.HasPrefix(p, "/")
}

// Names from files on disk are only offered when they are plain.
func plainName(s string) bool {
	if s == "" {
		return false
	}
	for _, c := range s {
		if !('a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9' || c == '-' || c == '_' || c == '.') {
			return false
		}
	}
	return true
}

// KiroAgents is KiroRunner.Agents: the user's (~/.kiro/agents) and the project's
// (<folder>/.kiro/agents) agents, by the name in each file.
func KiroAgents(folder string) []string {
	var names []string
	dirs := []string{filepath.Join(Home(), ".kiro", "agents")}
	if UsableFolder(folder) {
		dirs = append(dirs, filepath.Join(folder, ".kiro", "agents"))
	}
	for _, dir := range dirs {
		if !isDir(dir) {
			continue
		}
		entries, err := os.ReadDir(dir)
		if err != nil {
			continue
		}
		var files []string
		for _, e := range entries {
			p := filepath.Join(dir, e.Name())
			if extension(p) == "json" && isFile(p) {
				files = append(files, p)
			}
		}
		slices.Sort(files)
		for _, f := range files {
			name := fileStem(f)
			if text, err := os.ReadFile(f); err == nil {
				if v, err := core.ParseJSON(core.TextOf(text)); err == nil {
					if n, ok := v.Get("name"); ok {
						if s, ok := n.AsStr(); ok && s != "" {
							name = s
						}
					}
				}
			}
			if plainName(name) && !slices.Contains(names, name) {
				names = append(names, name)
			}
		}
	}
	return names
}

// isDir is Path::is_dir: there, and a folder (through links).
func isDir(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.IsDir()
}

// isFile is Path::is_file: there, and a plain file (through links).
func isFile(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.Mode().IsRegular()
}
