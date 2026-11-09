package pipeline

import (
	"context"
	"testing"

	"github.com/leonardotrapani/hyprvoice/internal/config"
	"github.com/leonardotrapani/hyprvoice/internal/testutil"
)

func TestPipeline_WithMocks_EmptyText(t *testing.T) {
	for _, tc := range []struct {
		name, raw, processed string
	}{
		{"empty transcription", "", "unexpected"},
		{"whitespace transcription", " \t\n\u2003", "unexpected"},
		{"empty cleanup", "hello", ""},
		{"whitespace cleanup", "hello", " \t\n\u2003"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg := &config.Config{
				LLM:       config.LLMConfig{Enabled: true, Provider: "openai", Model: "test"},
				Providers: map[string]config.ProviderConfig{"openai": {APIKey: "test-key"}},
			}
			recorder := testutil.NewMockRecorder()
			transcriber := testutil.NewMockTranscriber(tc.raw)
			injector := testutil.NewMockInjector()
			adapter := testutil.NewMockLLMAdapter(tc.processed)
			p := New(cfg,
				WithInjectorFactory(testutil.MockInjectorFactory(injector)),
				WithLLMAdapterFactory(testutil.MockLLMAdapterFactory(adapter)),
			).(*pipeline)
			p.setStatus(Transcribing)
			p.handleInjectAction(context.Background(), recorder, transcriber)
			if p.Status() != Idle {
				t.Fatalf("status = %s, want idle", p.Status())
			}
			if len(injector.GetInjectedTexts()) != 0 {
				t.Fatal("empty result reached injector")
			}
			select {
			case err := <-p.GetErrorCh():
				t.Fatalf("unexpected error notification: %v", err)
			default:
			}
			if tc.processed == "unexpected" && adapter.ProcessCalled {
				t.Fatal("empty transcription reached cleanup")
			}
		})
	}
}
