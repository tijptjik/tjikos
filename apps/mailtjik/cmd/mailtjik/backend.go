package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"

	tea "github.com/charmbracelet/bubbletea"
)

type Partition struct {
	Account  string `json:"account"`
	Year     string `json:"year"`
	Messages int    `json:"messages"`
}
type Overview struct {
	Total      int                               `json:"total"`
	Accounts   map[string]int                    `json:"accounts"`
	Years      map[string]int                    `json:"years"`
	Partitions []Partition                       `json:"partitions"`
	Status     map[string]map[string]interface{} `json:"status"`
	Config     map[string]string                 `json:"config"`
	Published  bool                              `json:"published"`
}
type Filters struct {
	Participant string `json:"participant"`
	Subject     string `json:"subject"`
	Start       string `json:"start"`
	End         string `json:"end"`
	Role        string `json:"role"`
	Account     string `json:"account"`
}
type MailRow struct {
	SHA     string `json:"sha256"`
	Subject string `json:"subject"`
	Date    string `json:"normalized_utc"`
	From    string `json:"from_header"`
	To      string `json:"to_header"`
	Cc      string `json:"cc_header"`
	Bcc     string `json:"bcc_header"`
	Account string `json:"account"`
	Year    string `json:"year"`
	Bytes   int    `json:"byte_size"`
}
type Facet struct {
	Account   string `json:"account"`
	Year      string `json:"year"`
	Available int    `json:"available"`
	Matches   int    `json:"matches"`
}
type Results struct {
	Matches   int       `json:"matches"`
	Available int       `json:"available"`
	Rows      []MailRow `json:"rows"`
	Facets    []Facet   `json:"facets"`
	Offset    int       `json:"offset"`
	Limit     int       `json:"limit"`
}
type InlineImage struct {
	Name string `json:"name"`
	CID  string `json:"cid"`
	Data string `json:"data"`
}
type Attachment struct {
	Name string `json:"name"`
	Size int    `json:"size"`
	SHA  string `json:"sha256"`
}
type Message struct {
	Subject     string        `json:"subject"`
	From        string        `json:"from"`
	To          string        `json:"to"`
	Cc          string        `json:"cc"`
	Bcc         string        `json:"bcc"`
	Date        string        `json:"date"`
	Markdown    string        `json:"markdown"`
	HTML        string        `json:"html"`
	Images      []InlineImage `json:"images"`
	Attachments []Attachment  `json:"attachments"`
}
type response struct {
	Kind       string
	Data       json.RawMessage
	Err        error
	Generation int
}
type backend struct {
	Path    string
	Config  string
	Demo    bool
	Context context.Context
}

func findBackend() (string, error) {
	if path := os.Getenv("MAILTJIK_BACKEND"); path != "" {
		return path, nil
	}
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	exe, _ = filepath.EvalSymlinks(exe)
	for _, p := range []string{filepath.Join(filepath.Dir(exe), "backend/mailtjik_backend.py"), filepath.Join(filepath.Dir(exe), "../lib/mailtjik/mailtjik_backend.py")} {
		if _, err := os.Stat(p); err == nil {
			return p, nil
		}
	}
	return "", fmt.Errorf("backend missing: run bun run install:local from tooltjik")
}
func (b backend) call(kind string, values map[string]interface{}, generation int) tea.Cmd {
	return func() tea.Msg {
		req := map[string]interface{}{"op": kind, "config": b.Config, "demo": b.Demo}
		for k, v := range values {
			req[k] = v
		}
		payload, err := json.Marshal(req)
		if err != nil {
			return response{Kind: kind, Err: err, Generation: generation}
		}
		cmd := exec.CommandContext(b.Context, "python3", b.Path)
		cmd.Stdin = bytes.NewReader(payload)
		cmd.Env = append(os.Environ(), "PYTHONDONTWRITEBYTECODE=1")
		cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
		cmd.Cancel = func() error {
			if cmd.Process != nil {
				return syscall.Kill(-cmd.Process.Pid, syscall.SIGTERM)
			}
			return nil
		}
		output, err := cmd.Output()
		if err != nil {
			return response{Kind: kind, Err: fmt.Errorf("backend did not complete; check your private setup"), Generation: generation}
		}
		var result struct {
			OK    bool            `json:"ok"`
			Data  json.RawMessage `json:"data"`
			Error string          `json:"error"`
		}
		if err = json.Unmarshal(output, &result); err != nil {
			return response{Kind: kind, Err: fmt.Errorf("invalid backend response"), Generation: generation}
		}
		if !result.OK {
			err = fmt.Errorf("%s", result.Error)
		}
		return response{Kind: kind, Data: result.Data, Err: err, Generation: generation}
	}
}
