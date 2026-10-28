package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
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
	return "", fmt.Errorf("backend missing: run bun run install:local from the TjikOS checkout")
}

// Each progress event carries the next read so Bubble Tea keeps receiving
// updates without a shared file, polling process, or access to private mail.
type queryProgressMsg struct {
	Progress   QueryProgress
	Generation int
	Next       tea.Cmd
}

func nextBackendEvent(events <-chan tea.Msg, ctx context.Context) tea.Cmd {
	return func() tea.Msg {
		select {
		case event := <-events:
			return event
		case <-ctx.Done():
			return nil
		}
	}
}

func (b backend) call(kind string, values map[string]interface{}, generation int) tea.Cmd {
	return func() tea.Msg {
		req := map[string]interface{}{"op": kind, "config": b.Config, "demo": b.Demo}
		for k, v := range values {
			req[k] = v
		}
		if kind == "query" {
			req["progress"] = true
		}
		payload, err := json.Marshal(req)
		if err != nil {
			return response{Kind: kind, Err: err, Generation: generation}
		}
		ctx := b.Context
		if ctx == nil {
			ctx = context.Background()
		}
		cmd := exec.CommandContext(ctx, "python3", b.Path)
		cmd.Stdin = bytes.NewReader(payload)
		cmd.Env = append(os.Environ(), "PYTHONDONTWRITEBYTECODE=1")
		cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
		cmd.Cancel = func() error {
			if cmd.Process != nil {
				return syscall.Kill(-cmd.Process.Pid, syscall.SIGTERM)
			}
			return nil
		}
		output, err := cmd.StdoutPipe()
		if err == nil {
			err = cmd.Start()
		}
		if err != nil {
			return response{Kind: kind, Err: fmt.Errorf("backend did not complete; check your private setup"), Generation: generation}
		}
		events := make(chan tea.Msg)
		send := func(event tea.Msg) bool {
			select {
			case events <- event:
				return true
			case <-ctx.Done():
				return false
			}
		}
		go func() {
			defer close(events)
			decoder := json.NewDecoder(output)
			final := response{Kind: kind, Generation: generation, Err: fmt.Errorf("invalid backend response")}
			for {
				var result struct {
					OK       bool            `json:"ok"`
					Data     json.RawMessage `json:"data"`
					Error    string          `json:"error"`
					Progress *QueryProgress  `json:"progress"`
				}
				err := decoder.Decode(&result)
				if err == io.EOF {
					break
				}
				if err != nil {
					final.Err = fmt.Errorf("invalid backend response")
					_ = cmd.Cancel()
					break
				}
				if result.Progress != nil {
					if !send(queryProgressMsg{*result.Progress, generation, nextBackendEvent(events, ctx)}) {
						break
					}
				} else {
					final.Data, final.Err = result.Data, nil
					if !result.OK {
						final.Err = fmt.Errorf("%s", result.Error)
					}
				}
			}
			if err := cmd.Wait(); err != nil {
				final.Err = fmt.Errorf("backend did not complete; check your private setup")
			}
			send(final)
		}()
		return nextBackendEvent(events, ctx)()
	}
}
