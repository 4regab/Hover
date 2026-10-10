package shell

import (
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/chat"
	"github.com/4regab/Hover/internal/core"
)

// net.rs: where the chat's images come from, as WebView2 served them to 2.x's page:
//   - https://hover.images/<name>: a prompt's pasted or attached image, in Hover's
//     kiro-images folder;
//   - https://f<key12>.hover/<path>: a file in the session's own folder (FilesHost);
//   - any other http(s) address: the web.
//
// Everything is read on worker goroutines (the UI never waits), started the first time an
// image is asked for. When the bytes (or the failure) are in, the UI is told the URL. The
// bytes are handed over once and not kept: the chat's own cache holds the decoded picture.

const (
	maxImageBytes = 32 << 20
	imageWorkers  = 2
)

type netSt uint8

const (
	netPending netSt = iota
	netDone
	netFailed
)

type netEntry struct {
	st    netSt
	bytes []byte
}

type imgNet struct {
	mu      sync.Mutex
	state   map[string]*netEntry
	jobs    chan string
	arrived func(url string)
	client  *http.Client
}

var (
	hostsMu sync.Mutex
	hosts   = map[string]string{}
	netOnce sync.Once
	theNet  *imgNet
)

// serveHosts keeps the virtual hosts, apart from the loader: naming a session's folder
// host (every chat laid out does) doesn't start the loader; only an image asked for does.
func serveHost(host, root string) {
	hostsMu.Lock()
	hosts[host] = root
	hostsMu.Unlock()
}

// filesHost is the session's files host (f<key12>.hover), serving its folder, when the
// folder is one a session may use.
func filesHost(key, folder string) string {
	if !agents.UsableFolder(folder) || len(key) < 12 {
		return ""
	}
	host := "f" + strings.ToLower(key[:12]) + ".hover"
	serveHost(host, folder)
	return host
}

// newImages is a new cache for one chat, loading through the process's loader.
func (s *Shell) newImages() *chat.Images {
	netOnce.Do(func() {
		serveHost("hover.images", core.ImagesFolder(core.Support()))
		theNet = newImgNet(func(u string) { s.env.UIDo(func() { s.imageArrived(u) }) })
	})
	return chat.NewImages(theNet.fetch)
}

func newImgNet(arrived func(string)) *imgNet {
	n := &imgNet{state: map[string]*netEntry{}, jobs: make(chan string, 256), arrived: arrived, client: &http.Client{Timeout: 30 * time.Second}}
	for i := 0; i < imageWorkers; i++ {
		go func() {
			for u := range n.jobs {
				got, err := n.load(u)
				if err != nil {
					core.Logf("image %s: %v", u, err)
				}
				n.mu.Lock()
				if err != nil {
					n.state[u] = &netEntry{st: netFailed}
				} else {
					n.state[u] = &netEntry{st: netDone, bytes: got}
				}
				n.mu.Unlock()
				n.arrived(u)
			}
		}()
	}
	return n
}

// fetch is what there is for a URL; the first ask starts its load. Bytes are given once.
func (n *imgNet) fetch(u string) chat.Fetch {
	n.mu.Lock()
	defer n.mu.Unlock()
	e := n.state[u]
	switch {
	case e == nil:
		n.state[u] = &netEntry{st: netPending}
		select {
		case n.jobs <- u:
		default:
			delete(n.state, u)
			return chat.Fetch{Kind: chat.FetchFailed}
		}
		return chat.Fetch{Kind: chat.FetchPending}
	case e.st == netDone:
		delete(n.state, u)
		return chat.Fetch{Kind: chat.FetchBytes, Bytes: e.bytes}
	case e.st == netFailed:
		delete(n.state, u)
		return chat.Fetch{Kind: chat.FetchFailed}
	}
	return chat.Fetch{Kind: chat.FetchPending}
}

func (n *imgNet) load(u string) ([]byte, error) {
	rest, ok := strings.CutPrefix(u, "https://")
	if !ok {
		rest, ok = strings.CutPrefix(u, "http://")
	}
	if !ok {
		return nil, errors.New("not http(s)")
	}
	host, path, _ := strings.Cut(rest, "/")
	hostsMu.Lock()
	root, served := hosts[strings.ToLower(host)]
	hostsMu.Unlock()
	if served {
		p, err := localFile(root, path)
		if err != nil {
			return nil, err
		}
		return os.ReadFile(p)
	}
	// A virtual host that isn't served (a session no longer open) never goes to the web.
	if strings.EqualFold(host, "hover.images") || strings.HasSuffix(strings.ToLower(host), ".hover") {
		return nil, errors.New("not served")
	}
	r, err := n.client.Get(u)
	if err != nil {
		return nil, err
	}
	defer r.Body.Close()
	if r.StatusCode >= 400 {
		return nil, errors.New(r.Status)
	}
	b, err := io.ReadAll(io.LimitReader(r.Body, maxImageBytes+1))
	if err != nil {
		return nil, err
	}
	if len(b) > maxImageBytes {
		return nil, errors.New("too large")
	}
	return b, nil
}

// localFile is a virtual host's path as a file under its folder: percent-decoded per
// segment, with no way out of the folder (no "..", and no link that resolves outside it).
func localFile(root, path string) (string, error) {
	if i := strings.IndexAny(path, "?#"); i >= 0 {
		path = path[:i]
	}
	p := root
	for _, seg := range strings.Split(path, "/") {
		if seg == "" {
			continue
		}
		seg, err := url.PathUnescape(seg)
		if err != nil {
			return "", errors.New("bad escape")
		}
		if seg == ".." || seg == "." || strings.ContainsAny(seg, `/\:`) {
			return "", errors.New("outside the folder")
		}
		p = filepath.Join(p, seg)
	}
	full, err := filepath.EvalSymlinks(p)
	if err != nil {
		return "", err
	}
	base, err := filepath.EvalSymlinks(root)
	if err != nil {
		return "", err
	}
	if rel, err := filepath.Rel(base, full); err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", errors.New("outside the folder")
	}
	return full, nil
}
