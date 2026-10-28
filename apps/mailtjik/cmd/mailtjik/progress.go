package main

import (
	"fmt"
	"math"
	"strings"

	"github.com/charmbracelet/x/ansi"
)

type QueryProgress struct {
	Phase         string   `json:"phase"`
	Total         int      `json:"total_indexes"`
	Ready         int      `json:"ready_indexes"`
	Cached        int      `json:"cached_indexes"`
	Searched      int      `json:"searched_indexes"`
	Downloaded    int64    `json:"downloaded_bytes"`
	DownloadTotal int64    `json:"total_download_bytes"`
	Elapsed       float64  `json:"elapsed_seconds"`
	ETA           *float64 `json:"eta_seconds"`
}

func (p QueryProgress) title() string {
	switch p.Phase {
	case "downloading":
		return "Downloading search indexes…"
	case "verifying":
		return "Decrypting and verifying indexes…"
	case "searching":
		return "Searching your archive…"
	default:
		return "Checking cached indexes…"
	}
}

func progressDuration(seconds float64) string {
	n := int(math.Ceil(math.Max(0, seconds)))
	if n >= 3600 {
		return fmt.Sprintf("%dh %dm", n/3600, n%3600/60)
	}
	if n >= 60 {
		return fmt.Sprintf("%dm %ds", n/60, n%60)
	}
	return fmt.Sprintf("%ds", n)
}

func (m model) indexProgressView(width, height int) string {
	if m.queryProgress == nil {
		return ansi.Truncate(dimStyle.Render("Preparing search: checking cached indexes…"), max(0, width), "")
	}
	p := *m.queryProgress
	done, noun := p.Ready, "ready"
	note := "Saved locally for faster searches next time."
	if p.Phase == "searching" {
		done, noun = p.Searched, "searched"
		note = "Finding matching messages in your indexes."
	} else if p.DownloadTotal == 0 {
		note = "Using your locally cached search indexes."
	}
	fraction := 0.
	if p.Total > 0 {
		fraction = math.Min(1, math.Max(0, float64(done)/float64(p.Total)))
	}
	barWidth := max(4, min(32, width-7))
	filled := int(fraction * float64(barWidth))
	bar := accentStyle.Render(strings.Repeat("━", filled)) + dimStyle.Render(strings.Repeat("─", barWidth-filled))
	rows := []string{
		titleStyle.Render(trim(p.title(), width)),
		dimStyle.Render(trim(note, width)),
		bar + fmt.Sprintf(" %3.0f%%", fraction*100),
		fmt.Sprintf("%d / %d indexes %s", done, p.Total, noun),
	}
	if p.Phase != "searching" && p.Cached > 0 {
		rows[3] += fmt.Sprintf(" · %d cached", p.Cached)
	}
	if p.DownloadTotal > 0 && p.Phase != "searching" {
		rows = append(rows, fmt.Sprintf("Downloaded %.1f / %.1f MiB", float64(p.Downloaded)/(1024*1024), float64(p.DownloadTotal)/(1024*1024)))
	}
	timing := "Elapsed " + progressDuration(p.Elapsed)
	if p.Phase == "downloading" {
		eta := "estimating…"
		if p.ETA != nil {
			eta = "~" + progressDuration(*p.ETA)
		}
		timing = "Download ETA " + eta + " · " + timing
	}
	rows = append(rows, dimStyle.Render(trim(timing, width)))
	for i := range rows {
		rows[i] = ansi.Truncate(strings.TrimRight(rows[i], " "), max(0, width), "")
	}
	// Retain ETA even when the results pane is very short.
	if height < len(rows) && height >= 2 {
		rows = append(rows[:height-1], rows[len(rows)-1])
	}
	return strings.Join(rows, "\n")
}
