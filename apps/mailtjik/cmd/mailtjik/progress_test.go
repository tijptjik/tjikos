package main

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/charmbracelet/x/ansi"
)

func TestProgressFitsPane(t *testing.T) {
	eta := 12.0
	for _, phase := range []string{"downloading", "verifying", "checking", "searching"} {
		m := model{queryProgress: &QueryProgress{Phase: phase, Total: 85, Ready: 20, DownloadTotal: 188000000, Downloaded: 10000000, ETA: &eta}}
		for _, width := range []int{12, 30, 70} {
			view := m.indexProgressView(width, 4)
			lines := strings.Split(view, "\n")
			if len(lines) > 4 {
				t.Fatalf("too tall: %q", view)
			}
			for _, line := range lines {
				if ansi.StringWidth(line) > width {
					t.Fatalf("overflow: %q", line)
				}
			}
		}
	}
	m := model{queryProgress: &QueryProgress{Phase: "downloading", Total: 85}}
	if !strings.Contains(m.indexProgressView(80, 10), "estimating") {
		t.Fatal("missing unknown ETA")
	}
}

func TestBackendStreamsBeforeCompletion(t *testing.T) {
	path := filepath.Join(t.TempDir(), "backend.py")
	script := `import json,sys,time
request=json.load(sys.stdin)
assert request['progress']
print(json.dumps({'progress': {'phase':'downloading','total_indexes':85}}), flush=True)
time.sleep(.2)
print(json.dumps({'ok':True,'data':{'matches':3}}), flush=True)
`
	if err := os.WriteFile(path, []byte(script), 0600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	event := (backend{Path: path, Context: ctx}).call("query", nil, 42)()
	progress, ok := event.(queryProgressMsg)
	if !ok || progress.Generation != 42 || progress.Progress.Total != 85 {
		t.Fatalf("unexpected progress: %#v", event)
	}
	final, ok := progress.Next().(response)
	if !ok || final.Err != nil || string(final.Data) != `{"matches": 3}` {
		t.Fatalf("unexpected final: %#v", final)
	}
}
