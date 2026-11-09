package pipeline

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestVoiceHistory(t *testing.T) {
	root := t.TempDir()
	t.Setenv("XDG_STATE_HOME", root)
	if err := saveVoiceHistory(" \n"); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(root, "hyprvoice", "history")
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatal("empty text created history")
	}
	for range 2 {
		if err := saveVoiceHistory("Hello 香港\nsecond line"); err != nil {
			t.Fatal(err)
		}
	}
	files, err := os.ReadDir(dir)
	if err != nil || len(files) != 2 {
		t.Fatalf("files: %v, %v", files, err)
	}
	for _, f := range files {
		info, _ := f.Info()
		if info.Mode().Perm() != 0600 {
			t.Fatal("history is not private")
		}
		data, _ := os.ReadFile(filepath.Join(dir, f.Name()))
		var row map[string]string
		if err := json.Unmarshal(data, &row); err != nil {
			t.Fatal(err)
		}
		if row["text"] != "Hello 香港\nsecond line" || row["created"] == "" {
			t.Fatal(row)
		}
	}
}
