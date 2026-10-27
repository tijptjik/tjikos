package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"

	md "github.com/JohannesKaufmann/html-to-markdown"
	"github.com/charmbracelet/bubbles/spinner"
	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
	"tjikos/mailtjik/internal/archiveui"
)

var (
	ink           = lipgloss.Color("#cdd6f4")
	dim           = lipgloss.Color("#7f849c")
	violet        = lipgloss.Color("#cba6f7")
	cyan          = lipgloss.Color("#89dceb")
	rose          = lipgloss.Color("#f5c2e7")
	surface       = lipgloss.Color("#242438")
	green         = lipgloss.Color("#a6e3a1")
	titleStyle    = lipgloss.NewStyle().Bold(true).Foreground(violet)
	dimStyle      = lipgloss.NewStyle().Foreground(dim)
	accentStyle   = lipgloss.NewStyle().Foreground(cyan)
	selectedStyle = lipgloss.NewStyle().Foreground(ink).Background(surface).Bold(true)
)
var roles = []string{"any", "from", "to", "cc", "bcc"}
var actions = []struct{ Title, Detail, Action string }{
	{"Prepare existing backups", "Index retained sources without changing the originals", "prepare"},
	{"Publish annual archive", "Encrypt, upload and verify account/year collections", "export"},
	{"Connect a Gmail mailbox", "Authorize read-only access in your browser", "authenticate"},
	{"Inventory live mail", "Discover years, addresses and labels before downloading bodies", "inventory"},
	{"Select and capture", "Choose years, addresses and excluded labels", "tree"},
	{"Publish captured mail", "Append a new generation using the published archive ledger", "export_capture"},
	{"Fetch latest catalogue", "Refresh available accounts and years from Drive", "refresh"},
	{"Setup", "Choose private archive, source and connection paths", "setup"},
}
var setupFields = []struct{ Key, Label string }{
	{"workdir", "Archive directory (private catalogue, cache and worker state)"},
	{"import_workdir", "Prepared import directory"}, {"source_config", "Private source / address-priority JSON"},
	{"policy", "Encryption policy JSON (age recipient and identity path)"}, {"remote", "Archive remote"},
	{"live_workdir", "Live capture directory"}, {"connection", "Private mailbox connection JSON"},
	{"client", "Google Desktop OAuth client JSON"}, {"base_ledger", "Previous published ledger (optional)"},
}

type tickMsg time.Time

func tick() tea.Cmd { return tea.Tick(3*time.Second, func(t time.Time) tea.Msg { return tickMsg(t) }) }
func clean(value string) string {
	return strings.Map(func(r rune) rune {
		if r == '\n' || r == '\t' {
			return r
		}
		if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) {
			return -1
		}
		return r
	}, value)
}
func line(value string) string            { return strings.Join(strings.Fields(clean(value)), " ") }
func trim(value string, width int) string { return ansi.Truncate(line(value), max(1, width), "…") }
func badge(text string, color lipgloss.Color) string {
	return lipgloss.NewStyle().Foreground(lipgloss.Color("#11111b")).Background(color).Padding(0, 1).Bold(true).Render(text)
}

type model struct {
	imageFallback                                string
	backend                                      backend
	graphics                                     *graphicsWriter
	page                                         string
	mode                                         string
	width, height                                int
	menu, action, focus, role, account, selected int
	inputs                                       []textinput.Model
	setup                                        []textinput.Model
	setupFocus                                   int
	overview                                     Overview
	results                                      *Results
	message                                      *Message
	selectedRow                                  MailRow
	spinner                                      spinner.Model
	viewport                                     viewport.Model
	busy                                         bool
	loading                                      string
	notice                                       string
	failure                                      bool
	generation                                   int
	dirty                                        bool
	tree                                         *archiveui.Model
	plan                                         string
	planCount                                    int
	planBytes                                    int64
	imageIndex                                   int
	showImages                                   bool
	imageRows                                    int
	paint                                        string
	imageError                                   string
}

func newModel(mode string, b backend, g *graphicsWriter) model {
	m := model{backend: b, graphics: g, page: mode, mode: mode, width: 110, height: 36, focus: 6, showImages: true, viewport: viewport.New(80, 15)}
	if mode == "" {
		m.page = "choose"
		m.mode = "read"
	}
	for _, placeholder := range []string{"Name or email address", "Words in the subject", "First year", "Last year"} {
		field := textinput.New()
		field.Placeholder = placeholder
		field.CharLimit = 160
		field.Prompt = ""
		field.TextStyle = lipgloss.NewStyle().Foreground(ink)
		field.PlaceholderStyle = dimStyle
		field.Cursor.Style = accentStyle
		m.inputs = append(m.inputs, field)
	}
	m.inputs[2].CharLimit = 4
	m.inputs[3].CharLimit = 4
	m.spinner = spinner.New()
	m.spinner.Spinner = spinner.Dot
	m.spinner.Style = accentStyle
	m.busy = true
	m.loading = "overview"
	return m
}
func (m model) Init() tea.Cmd {
	return tea.Batch(m.backend.call("overview", nil, 0), m.spinner.Tick, tick())
}
func (m model) accounts() []string {
	result := []string{""}
	for a := range m.overview.Accounts {
		result = append(result, a)
	}
	sort.Strings(result[1:])
	return result
}
func (m model) filters() Filters {
	accounts := m.accounts()
	account := ""
	if m.account < len(accounts) {
		account = accounts[m.account]
	}
	return Filters{strings.TrimSpace(m.inputs[0].Value()), strings.TrimSpace(m.inputs[1].Value()), strings.TrimSpace(m.inputs[2].Value()), strings.TrimSpace(m.inputs[3].Value()), roles[m.role], account}
}
func (m *model) setFocus(i int) tea.Cmd {
	m.focus = i
	var cmd tea.Cmd
	for n := range m.inputs {
		if n == i {
			cmd = m.inputs[n].Focus()
		} else {
			m.inputs[n].Blur()
		}
	}
	return cmd
}
func (m *model) search(offset int) tea.Cmd {
	m.generation++
	m.busy = true
	m.loading = "query"
	m.notice = ""
	m.failure = false
	m.dirty = false
	return m.backend.call("query", map[string]interface{}{"filters": m.filters(), "offset": offset}, m.generation)
}
func (m *model) archive(action string) tea.Cmd {
	m.busy = true
	m.loading = action
	m.notice = ""
	m.failure = false
	return m.backend.call("archive", map[string]interface{}{"action": action, "plan": m.plan}, 0)
}
func (m *model) enterSetup() {
	m.page = "setup"
	m.setup = nil
	m.setupFocus = 0
	for _, f := range setupFields {
		input := textinput.New()
		input.Prompt = ""
		input.CharLimit = 4096
		input.SetValue(m.overview.Config[f.Key])
		input.Width = max(30, m.width-12)
		input.TextStyle = accentStyle
		input.Cursor.Style = accentStyle
		m.setup = append(m.setup, input)
	}
	m.setup[0].Focus()
}
func (m *model) switchMode(mode string) tea.Cmd {
	m.page = mode
	m.mode = mode
	m.notice = ""
	m.graphics.set("")
	if mode == "read" && m.results == nil && m.overview.Published {
		return m.search(0)
	}
	return nil
}
func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		for i := range m.inputs {
			m.inputs[i].Width = max(8, (m.width-40)/2)
		}
		for i := range m.setup {
			m.setup[i].Width = max(20, m.width-12)
		}
		if m.message != nil {
			m.renderMessage()
		}
		if m.tree != nil {
			m.tree.Width = m.width - 4
			m.tree.Height = m.height - 7
		}
		return m, nil
	case tickMsg:
		if !m.busy && (m.page == "archive" || !m.overview.Published) {
			return m, tea.Batch(tick(), m.backend.call("overview", nil, 0))
		}
		return m, tick()
	case spinner.TickMsg:
		m.spinner, cmd = m.spinner.Update(msg)
		return m, cmd
	case response:
		if msg.Kind == "query" && msg.Generation != m.generation {
			return m, nil
		}
		if msg.Err != nil {
			m.busy = false
			m.failure = true
			m.notice = clean(msg.Err.Error())
			return m, nil
		}
		switch msg.Kind {
		case "overview":
			if err := json.Unmarshal(msg.Data, &m.overview); err != nil {
				m.notice = "Could not read archive overview"
				m.failure = true
			}
			if m.loading == "overview" {
				m.busy = false
				m.loading = ""
			}
			if m.page == "read" && m.results == nil && !m.busy && m.overview.Published {
				return m, m.search(0)
			}
		case "query":
			var results Results
			if err := json.Unmarshal(msg.Data, &results); err != nil {
				m.notice = "Could not read search response"
				m.failure = true
			}
			m.results = &results
			m.selected = 0
			m.busy = false
			m.setFocus(6)
		case "message":
			var message Message
			if err := json.Unmarshal(msg.Data, &message); err != nil {
				m.notice = "Could not decode message"
				m.failure = true
				break
			}
			m.message = &message
			m.busy = false
			m.page = "message"
			m.imageIndex = 0
			m.viewport.GotoTop()
			m.renderMessage()
		case "save_config":
			m.busy = false
			m.page = "archive"
			m.notice = "Private setup saved"
			return m, m.backend.call("overview", nil, 0)
		case "export_message":
			var result struct {
				Notice string `json:"notice"`
			}
			json.Unmarshal(msg.Data, &result)
			m.notice = result.Notice
			m.busy = false
		case "archive":
			action := m.loading
			m.busy = false
			m.loading = ""
			if action == "tree" {
				var result struct {
					Inventory archiveui.Inventory `json:"inventory"`
					Output    string              `json:"output"`
				}
				if err := json.Unmarshal(msg.Data, &result); err != nil {
					m.notice = "Could not load selection tree"
					m.failure = true
					break
				}
				tree := archiveui.NewModel(result.Inventory, result.Output)
				tree.Width = m.width - 4
				tree.Height = m.height - 7
				m.tree = &tree
				m.plan = result.Output
				m.page = "tree"
			} else if action == "preview" {
				var result struct {
					Count  int    `json:"selected_provider_ids"`
					Bytes  int64  `json:"estimated_bytes"`
					Notice string `json:"notice"`
				}
				json.Unmarshal(msg.Data, &result)
				m.planCount = result.Count
				m.planBytes = result.Bytes
				m.notice = result.Notice
				m.page = "plan"
			} else {
				var result struct {
					Notice string `json:"notice"`
				}
				json.Unmarshal(msg.Data, &result)
				m.notice = result.Notice
				m.page = "archive"
				return m, m.backend.call("overview", nil, 0)
			}
		}
		return m, nil
	case tea.KeyMsg:
		key := msg.String()
		if key == "ctrl+c" {
			m.graphics.set("")
			return m, tea.Quit
		}
		if m.busy {
			if key == "esc" {
				m.notice = "Working… Ctrl-C closes this view; durable archive workers continue."
			}
			return m, nil
		}
		if m.page == "tree" {
			if key == "esc" && !m.tree.Preview && !m.tree.Searching || key == "q" && !m.tree.Searching {
				m.tree = nil
				m.page = "archive"
				return m, nil
			}
			updated, command := m.tree.Update(msg)
			tree := updated.(archiveui.Model)
			m.tree = &tree
			if tree.Saved {
				m.tree = nil
				m.page = "plan"
				return m, m.archive("preview")
			}
			return m, command
		}
		if m.page == "setup" {
			if key == "esc" {
				m.page = "archive"
				return m, nil
			}
			if key == "ctrl+s" {
				values := map[string]string{}
				for i, f := range setupFields {
					values[f.Key] = m.setup[i].Value()
				}
				m.busy = true
				m.loading = "save_config"
				return m, m.backend.call("save_config", map[string]interface{}{"values": values}, 0)
			}
			if key == "tab" || key == "shift+tab" || key == "enter" {
				m.setup[m.setupFocus].Blur()
				delta := 1
				if key == "shift+tab" {
					delta = -1
				}
				m.setupFocus = (m.setupFocus + delta + len(m.setup)) % len(m.setup)
				return m, m.setup[m.setupFocus].Focus()
			}
			m.setup[m.setupFocus], cmd = m.setup[m.setupFocus].Update(msg)
			return m, cmd
		}
		if m.page == "choose" {
			switch key {
			case "up", "down", "tab", "j", "k":
				m.menu = 1 - m.menu
			case "enter":
				if m.menu == 0 {
					return m, m.switchMode("read")
				}
				return m, m.switchMode("archive")
			case "q", "esc":
				return m, tea.Quit
			}
			return m, nil
		}
		if m.page == "plan" {
			switch key {
			case "enter":
				if m.planCount > 0 {
					return m, m.archive("capture")
				}
			case "esc":
				m.page = "archive"
			}
			return m, nil
		}
		if m.page == "coverage" {
			if key == "esc" || key == "q" {
				m.page = m.mode
				return m, nil
			}
			m.viewport, cmd = m.viewport.Update(msg)
			return m, cmd
		}
		if m.page == "message" {
			switch key {
			case "esc", "q":
				m.page = "read"
				m.graphics.set("")
				m.notice = ""
				return m, nil
			case "i":
				m.showImages = !m.showImages
				m.renderMessage()
				return m, nil
			case "]", "[":
				if len(m.message.Images) > 0 {
					delta := 1
					if key == "[" {
						delta = -1
					}
					m.imageIndex = (m.imageIndex + delta + len(m.message.Images)) % len(m.message.Images)
					m.renderMessage()
				}
				return m, nil
			case "e", "a":
				m.busy = true
				m.loading = "export_message"
				return m, m.backend.call("export_message", map[string]interface{}{"row": m.selectedRow, "attachments": key == "a"}, 0)
			}
			m.viewport, cmd = m.viewport.Update(msg)
			return m, cmd
		}
		if m.page == "archive" {
			switch key {
			case "up", "k":
				m.action = max(0, m.action-1)
			case "down", "j":
				m.action = min(len(actions)-1, m.action+1)
			case "enter":
				if actions[m.action].Action == "setup" {
					m.enterSetup()
					return m, textinput.Blink
				}
				return m, m.archive(actions[m.action].Action)
			case "r":
				return m, m.backend.call("overview", nil, 0)
			case "tab":
				return m, m.switchMode("read")
			case "c":
				m.coverage()
				return m, nil
			case "q", "esc":
				m.page = "choose"
			}
			return m, nil
		}
		// Read mode. Input fields deliberately submit on Enter: counts always refer
		// to the visible applied filters, with an explicit pending state while editing.
		if m.page == "read" {
			if key == "tab" {
				return m, m.setFocus((m.focus + 1) % 7)
			}
			if key == "shift+tab" {
				return m, m.setFocus((m.focus + 6) % 7)
			}
			if key == "ctrl+a" {
				return m, m.switchMode("archive")
			}
			if key == "esc" {
				if m.focus < 6 {
					return m, m.setFocus(6)
				}
				m.page = "choose"
				return m, nil
			}
			if key == "enter" {
				if m.focus == 6 && m.results != nil && len(m.results.Rows) > 0 && !m.dirty {
					m.selectedRow = m.results.Rows[m.selected]
					m.busy = true
					m.loading = "message"
					m.notice = ""
					return m, m.backend.call("message", map[string]interface{}{"row": m.selectedRow}, 0)
				}
				return m, m.search(0)
			}
			if m.focus < 4 {
				before := m.inputs[m.focus].Value()
				m.inputs[m.focus], cmd = m.inputs[m.focus].Update(msg)
				if before != m.inputs[m.focus].Value() {
					m.dirty = true
					m.results = nil
				}
				return m, cmd
			}
			if m.focus == 4 || m.focus == 5 {
				if key == " " || key == "right" || key == "left" {
					delta := 1
					if key == "left" {
						delta = -1
					}
					if m.focus == 4 {
						m.role = (m.role + delta + len(roles)) % len(roles)
					} else {
						m.account = (m.account + delta + len(m.accounts())) % len(m.accounts())
					}
					m.dirty = true
					m.results = nil
				}
				return m, nil
			}
			switch key {
			case "/":
				return m, m.setFocus(0)
			case "s":
				return m, m.search(0)
			case "c":
				m.coverage()
			case "q":
				m.page = "choose"
			case "up", "k":
				m.selected = max(0, m.selected-1)
			case "down", "j":
				if m.results != nil {
					m.selected = min(len(m.results.Rows)-1, m.selected+1)
				}
			case "pgdown", "n":
				if m.results != nil && m.results.Offset+len(m.results.Rows) < m.results.Matches {
					return m, m.search(m.results.Offset + 40)
				}
			case "pgup", "p":
				if m.results != nil {
					return m, m.search(max(0, m.results.Offset-40))
				}
			}
		}
	}
	return m, nil
}

func (m *model) coverage() {
	lines := []string{titleStyle.Render("Account / year coverage"), dimStyle.Render("Available counts come from the catalogue. Search counts reflect the last submitted filters."), ""}
	lines = append(lines, dimStyle.Render("Account                      Year         Available   Matches"))
	facets := map[string]int{}
	if m.results != nil {
		for _, f := range m.results.Facets {
			facets[f.Account+"/"+f.Year] = f.Matches
		}
	}
	parts := append([]Partition(nil), m.overview.Partitions...)
	sort.Slice(parts, func(i, j int) bool {
		if parts[i].Account == parts[j].Account {
			return parts[i].Year > parts[j].Year
		}
		return parts[i].Account < parts[j].Account
	})
	for _, p := range parts {
		matches := "—"
		if n, ok := facets[p.Account+"/"+p.Year]; ok {
			matches = strconv.Itoa(n)
		}
		accountWidth := max(12, min(28, m.width-40))
		lines = append(lines, fmt.Sprintf("%-*s  %-12s %9d %9s", accountWidth, trim(p.Account, accountWidth), p.Year, p.Messages, matches))
	}
	m.viewport = viewport.New(max(30, m.width-4), max(5, m.height-8))
	m.viewport.SetContent(strings.Join(lines, "\n"))
	m.page = "coverage"
}
func (m model) messageTop() string {
	msg := m.message
	width := m.width - 4
	metadata := []string{titleStyle.Render(trim(msg.Subject, width)), accentStyle.Render("From ") + trim(msg.From, width-5), accentStyle.Render("To   ") + trim(msg.To, width-5)}
	if msg.Cc != "" {
		metadata = append(metadata, accentStyle.Render("Cc   ")+trim(msg.Cc, width-5))
	}
	if msg.Bcc != "" {
		metadata = append(metadata, accentStyle.Render("Bcc  ")+trim(msg.Bcc, width-5))
	}
	metadata = append(metadata, dimStyle.Render(trim(fmt.Sprintf("%s  ·  %d attachments  ·  %d inline images", msg.Date, len(msg.Attachments), len(msg.Images)), width)))
	return strings.Join(metadata, "\n")
}
func (m *model) renderMessage() {
	if m.message == nil {
		return
	}
	width := max(20, m.width-4)
	m.imageRows = 0
	m.paint = ""
	m.imageError = ""
	m.imageFallback = ""
	if m.showImages && len(m.message.Images) > 0 && m.width >= 70 && m.height >= 24 {
		m.imageRows = max(3, min(8, (m.height-14)/3))
	}
	topHeight := lipgloss.Height(m.messageTop())
	bodyHeight := max(3, m.height-7-topHeight)
	if m.imageRows > 0 {
		bodyHeight -= m.imageRows + 1
	}
	m.viewport.Width = width
	m.viewport.Height = max(3, bodyHeight)
	content := m.message.Markdown
	if m.message.HTML != "" {
		converter := md.NewConverter("", true, nil)
		if converted, err := converter.ConvertString(m.message.HTML); err == nil {
			content = converted
		} else {
			content = "HTML could not be converted. Export EML to read the original in a mail client."
		}
	}
	content = clean(content)
	renderer, err := glamour.NewTermRenderer(glamour.WithStandardStyle("dark"), glamour.WithWordWrap(max(20, width-4)))
	if err == nil {
		if rendered, err := renderer.Render(content); err == nil {
			content = rendered
		}
	}
	m.viewport.SetContent(content)
	if m.imageRows > 0 {
		row := 1 + 3 + topHeight + m.viewport.Height + 1 + 1
		paint, err := m.graphics.prepare(m.message.Images[m.imageIndex], width-2, m.imageRows, row, 3)
		if err != nil {
			m.imageError = err.Error()
		} else {
			m.paint = paint
		}
		if !m.graphics.supported {
			preview, err := thumbnail(m.message.Images[m.imageIndex], width-2, m.imageRows)
			if err != nil {
				m.imageError = err.Error()
			} else {
				m.imageFallback = preview
			}
		}
	}
}
func (m model) header() string {
	mode := strings.ToUpper(m.mode)
	if m.page == "choose" {
		mode = "MAIL, KEPT CLOSE"
	}
	brand := titleStyle.Render("✉  mailtjik") + "  " + badge(mode, cyan)
	if m.backend.Demo {
		brand += "  " + badge("DEMO", rose)
	}
	summary := fmt.Sprintf("%d preserved emails  ·  %d accounts  ·  %d years", m.overview.Total, len(m.overview.Accounts), len(m.overview.Years))
	if !m.overview.Published && !m.backend.Demo {
		summary = "No published catalogue yet  ·  open Archive for preparation and progress"
	}
	return brand + "\n" + dimStyle.Render(trim(summary, m.width-4)) + "\n"
}
func (m model) noticeLine() string {
	if m.busy {
		labels := map[string]string{"query": "Searching selected indexes…", "message": "Fetching and verifying this message…", "authenticate": "Complete the read-only login in your browser…", "overview": "Opening your archive…"}
		label := labels[m.loading]
		if label == "" {
			label = "Working…"
		}
		return m.spinner.View() + " " + label
	}
	if m.notice != "" {
		style := accentStyle
		if m.failure {
			style = lipgloss.NewStyle().Foreground(lipgloss.Color("#f38ba8"))
		}
		return style.Render(trim(m.notice, m.width-4))
	}
	return ""
}
func (m model) chooseView() string {
	width := min(72, m.width-12)
	choices := []struct{ title, description string }{{"Read your mail", "Find a conversation, revisit a subject, open the original."}, {"Archive your mail", "Prepare, select and preserve encrypted annual collections."}}
	lines := []string{"", titleStyle.Render("What would you like to do?"), ""}
	for i, c := range choices {
		style := lipgloss.NewStyle().Border(lipgloss.RoundedBorder()).BorderForeground(dim).Padding(1, 2).Width(width)
		pointer := "  "
		if i == m.menu {
			style = style.BorderForeground(violet)
			pointer = "› "
		}
		lines = append(lines, style.Render(titleStyle.Render(pointer+c.title)+"\n"+dimStyle.Render(c.description)), "")
	}
	return strings.Join(lines, "\n")
}
func (m model) filterView() string {
	labels := []string{"PARTICIPANT", "SUBJECT", "START YEAR", "END YEAR"}
	field := func(i, width int) string {
		style := dimStyle
		if m.focus == i {
			style = accentStyle
		}
		input := m.inputs[i]
		input.Width = max(6, width-2)
		return style.Render(labels[i]) + "\n" + lipgloss.NewStyle().Width(width).Render(input.View())
	}
	width := max(20, (m.width-8)/2)
	first := lipgloss.JoinHorizontal(lipgloss.Top, field(0, width), "  ", field(1, width))
	years := lipgloss.JoinHorizontal(lipgloss.Top, field(2, 13), "  ", field(3, 13))
	roleLabel := "Role: " + roles[m.role]
	if m.focus == 4 {
		roleLabel = selectedStyle.Render("‹ " + roleLabel + " ›")
	} else {
		roleLabel = accentStyle.Render(roleLabel)
	}
	account := "All accounts"
	if m.account > 0 && m.account < len(m.accounts()) {
		account = m.accounts()[m.account]
	}
	accountLabel := "Account: " + account
	if m.focus == 5 {
		accountLabel = selectedStyle.Render("‹ " + accountLabel + " ›")
	} else {
		accountLabel = accentStyle.Render(accountLabel)
	}
	return first + "\n" + years + "\n" + roleLabel + "   " + ansi.Truncate(accountLabel, max(1, m.width-30), "…")
}
func (m model) sideView(height int) string {
	lines := []string{titleStyle.Render("COLLECTIONS"), fmt.Sprintf("All accounts  %d", m.overview.Total), ""}
	for _, account := range m.accounts()[1:] {
		style := dimStyle
		if account == m.filters().Account {
			style = accentStyle
		}
		lines = append(lines, style.Render(fmt.Sprintf("%-19s %5d", trim(account, 19), m.overview.Accounts[account])))
	}
	lines = append(lines, "", titleStyle.Render("YEARS"))
	years := []string{}
	counts := CounterYears(m.overview.Partitions, m.filters().Account)
	for y := range counts {
		years = append(years, y)
	}
	sort.Sort(sort.Reverse(sort.StringSlice(years)))
	for _, year := range years {
		lines = append(lines, fmt.Sprintf("%-17s %7d", year, counts[year]))
	}
	if len(lines) > height-1 {
		lines = append(lines[:max(1, height-2)], dimStyle.Render("c  all account/year counts"))
	}
	return lipgloss.NewStyle().Width(27).Height(height).Render(strings.Join(lines, "\n"))
}
func CounterYears(parts []Partition, account string) map[string]int {
	counts := map[string]int{}
	for _, p := range parts {
		if account == "" || p.Account == account {
			counts[p.Year] += p.Messages
		}
	}
	return counts
}
func (m model) resultsView(width, height int) string {
	lines := []string{}
	if m.results == nil {
		label := "Enter to search your archive"
		if m.dirty {
			label = "Filters changed · Enter to count matches"
		}
		if m.busy {
			label = "Searching…"
		}
		if !m.overview.Published && !m.backend.Demo {
			label = "Prepare or fetch a catalogue in Archive mode"
		}
		return dimStyle.Render(label)
	}
	r := m.results
	lines = append(lines, titleStyle.Render(fmt.Sprintf("%d matches", r.Matches))+dimStyle.Render(fmt.Sprintf(" / %d available", r.Available)))
	if r.Matches == 0 {
		return strings.Join(append(lines, "", "No emails match these filters."), "\n")
	}
	visible := max(1, (height-2)/2)
	start := max(0, m.selected-visible+1)
	end := min(len(r.Rows), start+visible)
	for i := start; i < end; i++ {
		row := r.Rows[i]
		date := row.Date
		if len(date) > 10 {
			date = date[:10]
		}
		subject := row.Subject
		if subject == "" {
			subject = "(no subject)"
		}
		lead := "  "
		style := lipgloss.NewStyle().Foreground(ink)
		if i == m.selected {
			lead = "› "
			style = selectedStyle
		}
		lines = append(lines, style.Width(width).Render(trim(lead+date+"  "+subject, width)))
		lines = append(lines, dimStyle.Render(trim("  "+row.From+"  ·  "+row.Account, width)))
	}
	lines = append(lines, dimStyle.Render(fmt.Sprintf("Showing %d–%d of %d  ·  n/p next/previous page", r.Offset+1, r.Offset+len(r.Rows), r.Matches)))
	return strings.Join(lines, "\n")
}
func (m model) readView() string {
	filter := m.filterView()
	height := max(4, m.height-8-lipgloss.Height(filter))
	width := max(25, m.width-34)
	return filter + "\n\n" + lipgloss.JoinHorizontal(lipgloss.Top, m.sideView(height), " │ ", m.resultsView(width, height))
}
func (m model) archiveView() string {
	lines := []string{titleStyle.Render("Preserve once. Revisit by year."), ""}
	available := max(2, (m.height-17)/2)
	start := max(0, m.action-available+1)
	end := min(len(actions), start+available)
	for i := start; i < end; i++ {
		a := actions[i]
		prefix := "  "
		style := lipgloss.NewStyle().Foreground(ink)
		if i == m.action {
			prefix = "› "
			style = selectedStyle
		}
		lines = append(lines, style.Width(m.width-4).Render(prefix+a.Title), dimStyle.Render(trim("  "+a.Detail, m.width-4)))
	}
	lines = append(lines, "", titleStyle.Render("WORKERS"))
	keys := []string{"workdir", "import_workdir", "live_workdir"}
	names := []string{"Archive", "Preparation", "Live capture"}
	for i, k := range keys {
		if state, ok := m.overview.Status[k]; ok {
			stage := fmt.Sprint(state["stage"])
			extra := ""
			if n, ok := state["processed_occurrences"]; ok {
				extra = fmt.Sprintf(" · %.0f occurrences", n)
			}
			lines = append(lines, accentStyle.Render("● ")+names[i]+": "+line(stage)+extra)
		}
	}
	if len(m.overview.Status) == 0 {
		lines = append(lines, dimStyle.Render("No worker is running yet. Setup → prepare → publish."))
	}
	lines = append(lines, dimStyle.Render("Workers continue after you close the TUI. Originals are retained."))
	return strings.Join(lines, "\n")
}
func (m model) setupView() string {
	lines := []string{titleStyle.Render("Private setup"), dimStyle.Render("Paths and credentials stay outside this repository. Ctrl-S saves; Esc returns."), ""}
	visible := max(1, (m.height-13)/3)
	start := max(0, m.setupFocus-visible+1)
	end := min(len(m.setup), start+visible)
	for i := start; i < end; i++ {
		label := dimStyle.Render(trim(setupFields[i].Label, m.width-6))
		if i == m.setupFocus {
			label = accentStyle.Render("› " + trim(setupFields[i].Label, m.width-8))
		}
		lines = append(lines, label, m.setup[i].View(), "")
	}
	return strings.Join(lines, "\n")
}
func (m model) messageView() string {
	body := m.messageTop() + "\n" + m.viewport.View()
	if m.imageRows > 0 {
		caption := fmt.Sprintf("Inline image %d/%d · %s · [ ] switch · i hide", m.imageIndex+1, len(m.message.Images), line(m.message.Images[m.imageIndex].Name))
		if !m.graphics.supported {
			caption += " · ANSI preview"
		}
		if m.imageError != "" {
			caption += " · " + m.imageError
		}
		body += "\n" + dimStyle.Render(trim(caption, m.width-4)) + "\n" + lipgloss.NewStyle().Height(m.imageRows).Render(m.imageFallback)
	}
	return body
}
func (m model) View() string {
	if m.width < 66 || m.height < 22 {
		m.graphics.set("")
		return "mailtjik\nResize the terminal to at least 66 columns × 22 rows.\nCtrl-C to quit."
	}
	content, help := "", "↑↓ move  Enter select  q back  Ctrl-C quit"
	switch m.page {
	case "choose":
		content = m.chooseView()
	case "read":
		content = m.readView()
		help = "Tab fields · / participant · Enter search/open · c counts · Ctrl-A archive · Esc back"
	case "archive":
		content = m.archiveView()
		help = "↑↓ choose · Enter launch · Tab read · c counts · r refresh · q modes"
	case "setup":
		content = m.setupView()
		help = "Tab next field · Shift-Tab previous · Ctrl-S save · Esc cancel"
	case "tree":
		if m.tree != nil {
			content = m.tree.View()
		}
		help = "Year/address boxes include mail. Checked labels exclude matching mail."
	case "plan":
		content = titleStyle.Render("Capture preview") + fmt.Sprintf("\n\n%d distinct messages selected\n%.2f MiB estimated before exact deduplication\n\nExclusion wins across every label, including All Mail.\n\n", m.planCount, float64(m.planBytes)/(1024*1024)) + accentStyle.Render("Enter starts the durable capture worker. Esc returns.")
	case "message":
		content = m.messageView()
		help = "↑↓ scroll · e export EML · a save attachments · i images · [ ] switch image · Esc results"
	case "coverage":
		content = m.viewport.View()
		help = "↑↓ scroll · Esc back"
	}
	if m.page == "message" && !m.busy {
		m.graphics.set(m.paint)
	} else {
		m.graphics.set("")
	}
	notice := m.noticeLine()
	if notice != "" {
		help = notice
	}
	// Bound every page to the current terminal to keep the status line and
	// reserved Kitty image rectangle fully visible after a resize.
	content = lipgloss.NewStyle().MaxHeight(m.height - 7).Width(m.width - 4).Render(content)
	frame := m.header() + "\n" + content + "\n" + dimStyle.Render(ansi.Truncate(help, max(1, m.width-4), "…"))
	return lipgloss.NewStyle().Padding(1, 2).Foreground(ink).Render(frame)
}
func main() {
	mode := ""
	args := os.Args[1:]
	if len(args) > 0 && (args[0] == "read" || args[0] == "archive") {
		mode = args[0]
		args = args[1:]
	}
	flags := flag.NewFlagSet("mailtjik", flag.ExitOnError)
	config := flags.String("config", "", "private configuration JSON")
	demo := flags.Bool("demo", false, "use synthetic demo emails only")
	flags.Usage = func() {
		fmt.Fprintln(flags.Output(), "mailtjik [read|archive] [--config PATH] [--demo]\n\nWithout a command, choose a mode interactively.")
		flags.PrintDefaults()
	}
	flags.Parse(args)
	if flags.NArg() != 0 {
		flags.Usage()
		os.Exit(2)
	}
	path, err := findBackend()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	graphics := newGraphics()
	defer graphics.cleanup()
	app := newModel(mode, backend{Path: path, Config: *config, Demo: *demo, Context: ctx}, graphics)
	if _, err := tea.NewProgram(app, tea.WithAltScreen(), tea.WithOutput(graphics)).Run(); err != nil {
		fmt.Fprintln(os.Stderr, "mailtjik could not start the terminal interface:", err)
		os.Exit(1)
	}
}
