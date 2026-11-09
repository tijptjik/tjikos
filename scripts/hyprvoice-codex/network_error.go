package daemon

import (
	"strings"

	"github.com/leonardotrapani/hyprvoice/internal/pipeline"
)

// The proxy wraps upstream network failures in HTTP errors, so inspect the
// underlying diagnostic rather than treating every 502 as a connectivity issue.
func pipelineNotificationMessage(failure pipeline.PipelineError, detail string) string {
	if failure.Title != "Transcription Error" || failure.Err == nil {
		return detail
	}
	cause := strings.ToLower(failure.Err.Error())
	for _, marker := range []string{
		"temporary failure in name resolution",
		"name or service not known",
		"no such host",
		"network is unreachable",
		"network is down",
		"cannot connect to host chatgpt.com:443",
	} {
		if strings.Contains(cause, marker) {
			return "Transcription couldn't reach the online service. Check your internet connection and try recording again."
		}
	}
	return detail
}
