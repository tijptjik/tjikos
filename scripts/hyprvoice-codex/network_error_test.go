package daemon

import (
	"errors"
	"testing"

	"github.com/leonardotrapani/hyprvoice/internal/pipeline"
)

func TestPipelineNotificationNetworkError(t *testing.T) {
	for _, tc := range []struct {
		name, title, cause string
		friendly           bool
	}{
		{"proxy DNS", "Transcription Error", "codex transcription: 502 Bad Gateway: Cannot connect to host chatgpt.com:443 ssl:default [Temporary failure in name resolution]", true},
		{"DNS", "Transcription Error", "lookup chatgpt.com: no such host", true},
		{"offline", "Transcription Error", "dial: network is unreachable", true},
		{"generic gateway", "Transcription Error", "502 Bad Gateway", false},
		{"authentication", "Transcription Error", "401 unauthorized", false},
		{"local proxy", "Transcription Error", "dial tcp 127.0.0.1:2455: connection refused", false},
		{"other stage", "Injection Error", "network is unreachable", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			failure := pipeline.PipelineError{Title: tc.title, Err: errors.New(tc.cause)}
			got := pipelineNotificationMessage(failure, tc.cause)
			want := tc.cause
			if tc.friendly {
				want = "Transcription couldn't reach the online service. Check your internet connection and try recording again."
			}
			if got != want {
				t.Fatalf("got %q, want %q", got, want)
			}
		})
	}
}
