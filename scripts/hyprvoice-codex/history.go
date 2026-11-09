package pipeline

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// One atomic, private file per result allows readers and deletion to coexist
// with new recordings without rewriting or locking the entire history.
func saveVoiceHistory(text string) error {
	if strings.TrimSpace(text) == "" {
		return nil
	}
	root := os.Getenv("XDG_STATE_HOME")
	if !filepath.IsAbs(root) {
		home, err := os.UserHomeDir()
		if err != nil {
			return err
		}
		root = filepath.Join(home, ".local", "state")
	}
	dir := filepath.Join(root, "hyprvoice", "history")
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	data, err := json.Marshal(struct {
		Created string `json:"created"`
		Text    string `json:"text"`
	}{time.Now().UTC().Format(time.RFC3339Nano), text})
	if err != nil {
		return err
	}
	file, err := os.CreateTemp(dir, ".pending-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if _, err = file.Write(data); err != nil {
		file.Close()
		return err
	}
	if err = file.Sync(); err != nil {
		file.Close()
		return err
	}
	if err = file.Close(); err != nil {
		return err
	}
	return os.Rename(file.Name(), filepath.Join(dir, time.Now().UTC().Format("20060102T150405.000000000")+"-"+strings.TrimPrefix(filepath.Base(file.Name()), ".pending-")+".json"))
}
