// Runs synthetic audio through the configured adapters without recording or typing.
package main

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/leonardotrapani/hyprvoice/internal/config"
	"github.com/leonardotrapani/hyprvoice/internal/llm"
	"github.com/leonardotrapani/hyprvoice/internal/recording"
	"github.com/leonardotrapani/hyprvoice/internal/transcriber"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	c, err := config.Load()
	if err != nil {
		return err
	}
	if err := c.Validate(); err != nil {
		return err
	}
	if c.Transcription.Provider != "codex" || c.LLM.Provider != "codex" || !c.LLM.Enabled {
		return fmt.Errorf("both stages must select codex")
	}
	if len(os.Args) != 2 {
		return fmt.Errorf("usage: codex-smoke synthetic-16khz-mono-s16le.pcm")
	}
	audio, err := os.ReadFile(os.Args[1])
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	t, err := transcriber.NewTranscriber(c.ToTranscriberConfig())
	if err != nil {
		return err
	}
	frames := make(chan recording.AudioFrame, 1)
	frames <- recording.AudioFrame{Data: audio}
	close(frames)
	errs, err := t.Start(ctx, frames)
	if err != nil {
		return err
	}
	if err := t.Stop(ctx); err != nil {
		return err
	}
	for err := range errs {
		if err != nil {
			return err
		}
	}
	text, err := t.GetFinalTranscription()
	if err != nil {
		return err
	}
	if text == "" {
		return fmt.Errorf("empty transcription")
	}
	cfg := c.ToLLMConfig()
	a, err := llm.NewAdapter(llm.Config{Provider: cfg.Provider, Model: cfg.Model, APIKey: cfg.APIKey,
		RemoveStutters: cfg.RemoveStutters, AddPunctuation: cfg.AddPunctuation,
		FixGrammar: cfg.FixGrammar, RemoveFillerWords: cfg.RemoveFillerWords})
	if err != nil {
		return err
	}
	result, err := a.Process(ctx, text)
	if err != nil {
		return err
	}
	if result == "" {
		return fmt.Errorf("empty cleanup")
	}
	fmt.Printf("Subscription adapter smoke passed: %s\n", result)
	return nil
}
