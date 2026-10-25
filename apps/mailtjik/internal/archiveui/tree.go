// email-archives-tree selects data; checked label boxes mean EXCLUDE.
package archiveui

import (
	"encoding/json"
	"fmt"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"os"
	"sort"
	"strings"
	"unicode"
)

type Label struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}
type Partition struct {
	Year     string  `json:"year"`
	Account  string  `json:"account"`
	Messages int     `json:"messages"`
	Labels   []Label `json:"labels"`
}
type Inventory struct {
	Version     int         `json:"version"`
	Mode        string      `json:"mode"`
	InventoryID string      `json:"inventory_id"`
	Connection  string      `json:"connection"`
	Partitions  []Partition `json:"partitions"`
}
type Selection struct {
	Year    string   `json:"year"`
	Account string   `json:"account"`
	Exclude []string `json:"exclude_label_ids"`
}
type Plan struct {
	Version     int         `json:"version"`
	Mode        string      `json:"mode"`
	InventoryID string      `json:"inventory_id"`
	Connection  string      `json:"connection"`
	Selections  []Selection `json:"selections"`
}
type Node struct {
	Name     string
	Kind     string
	Part     int
	LabelID  string
	Children []*Node
	Open     bool
	Parent   *Node
}
type Row struct {
	Node  *Node
	Depth int
}
type Model struct {
	Inventory Inventory
	Roots     []*Node
	Included  []bool
	Excluded  []map[string]bool
	Cursor    int
	Query     string
	Searching bool
	Preview   bool
	Output    string
	Error     string
	Saved     bool
	Height    int
	Width     int
}

var accent = lipgloss.NewStyle().Foreground(lipgloss.Color("86"))
var muted = lipgloss.NewStyle().Foreground(lipgloss.Color("243"))

func safe(s string) string {
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) {
			return ' '
		}
		return r
	}, s)
}
func add(parent, child *Node) {
	child.Parent = parent
	parent.Children = append(parent.Children, child)
}
func NewModel(inventory Inventory, output string) Model {
	m := Model{Inventory: inventory, Output: output, Height: 24, Width: 100}
	years := map[string]*Node{}
	for i, p := range inventory.Partitions {
		m.Included = append(m.Included, true)
		m.Excluded = append(m.Excluded, map[string]bool{})
		year := years[p.Year]
		if year == nil {
			year = &Node{Name: p.Year, Kind: "year", Open: true}
			years[p.Year] = year
			m.Roots = append(m.Roots, year)
		}
		account := &Node{Name: fmt.Sprintf("%s (%d candidates)", p.Account, p.Messages), Kind: "account", Part: i, Open: true}
		add(year, account)
		if len(p.Labels) > 0 {
			exclude := &Node{Name: "Exclude labels (checked = omitted)", Kind: "labels", Part: i}
			add(account, exclude)
			for _, label := range p.Labels {
				parent := exclude
				bits := strings.Split(label.Name, "/")
				for _, bit := range bits[:len(bits)-1] {
					var group *Node
					for _, c := range parent.Children {
						if c.Kind == "group" && c.Name == bit {
							group = c
							break
						}
					}
					if group == nil {
						group = &Node{Name: bit, Kind: "group", Part: i}
						add(parent, group)
					}
					parent = group
				}
				add(parent, &Node{Name: bits[len(bits)-1], Kind: "label", Part: i, LabelID: label.ID})
			}
		}
	}
	for _, root := range m.Roots {
		count := 0
		for _, child := range root.Children {
			count += inventory.Partitions[child.Part].Messages
		}
		root.Name = fmt.Sprintf("%s (%d emails)", root.Name, count)
	}
	sort.Slice(m.Roots, func(i, j int) bool { return m.Roots[i].Name > m.Roots[j].Name })
	return m
}
func (m Model) Init() tea.Cmd { return nil }
func (m Model) matches(n *Node) bool {
	if strings.Contains(strings.ToLower(safe(n.Name)), strings.ToLower(m.Query)) {
		return true
	}
	for _, c := range n.Children {
		if m.matches(c) {
			return true
		}
	}
	return false
}
func (m Model) rows() []Row {
	rows := []Row{}
	var walk func(*Node, int)
	walk = func(n *Node, d int) {
		if m.Query != "" && !m.matches(n) {
			return
		}
		rows = append(rows, Row{n, d})
		if n.Open || m.Query != "" {
			for _, c := range n.Children {
				walk(c, d+1)
			}
		}
	}
	for _, root := range m.Roots {
		walk(root, 0)
	}
	return rows
}
func (m Model) state(n *Node) (int, int) {
	if n.Kind == "account" {
		if m.Included[n.Part] {
			return 1, 1
		}
		return 0, 1
	}
	if n.Kind == "label" {
		if m.Excluded[n.Part][n.LabelID] {
			return 1, 1
		}
		return 0, 1
	}
	a, b := 0, 0
	for _, c := range n.Children {
		x, y := m.state(c)
		a += x
		b += y
	}
	return a, b
}
func (m *Model) set(n *Node, value bool) {
	if n.Kind == "account" {
		m.Included[n.Part] = value
		return
	}
	if n.Kind == "label" {
		m.Excluded[n.Part][n.LabelID] = value
		return
	}
	for _, c := range n.Children {
		m.set(c, value)
	}
}
func (m Model) plan() Plan {
	p := Plan{Version: 1, Mode: m.Inventory.Mode, InventoryID: m.Inventory.InventoryID, Connection: m.Inventory.Connection, Selections: []Selection{}}
	for i, part := range m.Inventory.Partitions {
		if !m.Included[i] {
			continue
		}
		ids := []string{}
		for id, excluded := range m.Excluded[i] {
			if excluded {
				ids = append(ids, id)
			}
		}
		sort.Strings(ids)
		p.Selections = append(p.Selections, Selection{part.Year, part.Account, ids})
	}
	return p
}
func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.Height = msg.Height
		m.Width = msg.Width
	case tea.KeyMsg:
		key := msg.String()
		if key == "ctrl+c" {
			return m, tea.Quit
		}
		if m.Searching {
			switch key {
			case "enter", "esc":
				m.Searching = false
			case "backspace":
				r := []rune(m.Query)
				if len(r) > 0 {
					m.Query = string(r[:len(r)-1])
				}
			default:
				if msg.Type == tea.KeyRunes {
					m.Query += string(msg.Runes)
				}
			}
			m.Cursor = 0
			return m, nil
		}
		if m.Preview {
			switch key {
			case "esc":
				m.Preview = false
			case "enter":
				f, err := os.OpenFile(m.Output, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
				if err == nil {
					err = json.NewEncoder(f).Encode(m.plan())
					if err == nil {
						err = f.Sync()
					}
					f.Close()
				}
				if err != nil {
					m.Error = "Could not save plan (output must be a new private file)"
					return m, nil
				}
				m.Saved = true
				return m, tea.Quit
			}
			return m, nil
		}
		rows := m.rows()
		if key == "q" {
			return m, tea.Quit
		}
		if key == "/" {
			m.Searching = true
			return m, nil
		}
		if key == "esc" {
			m.Query = ""
			m.Cursor = 0
			return m, nil
		}
		if key == "enter" {
			m.Preview = true
			return m, nil
		}
		if len(rows) == 0 {
			return m, nil
		}
		if m.Cursor >= len(rows) {
			m.Cursor = len(rows) - 1
		}
		node := rows[m.Cursor].Node
		switch key {
		case "up", "k":
			if m.Cursor > 0 {
				m.Cursor--
			}
		case "down", "j":
			if m.Cursor < len(rows)-1 {
				m.Cursor++
			}
		case "right", "l":
			node.Open = true
		case "left", "h":
			if node.Open {
				node.Open = false
			} else if node.Parent != nil {
				for i, r := range rows {
					if r.Node == node.Parent {
						m.Cursor = i
						break
					}
				}
			}
		case " ":
			a, b := m.state(node)
			m.set(node, a != b)
		case "a":
			for _, root := range m.Roots {
				m.set(root, true)
			}
		case "n":
			for _, root := range m.Roots {
				m.set(root, false)
			}
		}
	}
	return m, nil
}
func (m Model) View() string {
	if m.Saved {
		return "Selection plan saved.\n"
	}
	title := accent.Render("Annual email archives — message year / address / excluded labels") + "\n"
	if m.Preview {
		p := m.plan()
		labels := 0
		for _, s := range p.Selections {
			labels += len(s.Exclude)
		}
		return title + fmt.Sprintf("\n%d account/year collections selected; %d label exclusions.\n", len(p.Selections), labels) + "Exclusion wins across all labels, including All Mail.\nContinue to calculate the exact retained-message count.\n\nEnter: preview selected mail   Esc: return\n" + m.Error
	}
	rows := m.rows()
	var b strings.Builder
	b.WriteString(title)
	b.WriteString(muted.Render("↑↓ move  ←→ collapse/expand  Space toggle  / search  a all  n none  Enter preview  q quit") + "\n")
	if m.Query != "" || m.Searching {
		b.WriteString("Search: " + safe(m.Query) + "\n")
	}
	height := max(3, m.Height-6)
	start := max(0, m.Cursor-height/2)
	end := min(len(rows), start+height)
	for i := start; i < end; i++ {
		row := rows[i]
		a, total := m.state(row.Node)
		box := "[ ]"
		if a > 0 && a < total {
			box = "[-]"
		} else if a > 0 {
			box = "[x]"
		}
		arrow := " "
		if len(row.Node.Children) > 0 {
			arrow = "▸"
			if row.Node.Open {
				arrow = "▾"
			}
		}
		line := fmt.Sprintf("%s%s %s %s", strings.Repeat("  ", row.Depth), arrow, box, safe(row.Node.Name))
		line = lipgloss.NewStyle().MaxWidth(max(10, m.Width-3)).Render(line)
		if i == m.Cursor {
			b.WriteString(accent.Render("› " + line))
		} else {
			b.WriteString("  " + line)
		}
		b.WriteString("\n")
	}
	b.WriteString(muted.Render("Year/address boxes include mail. Label boxes EXCLUDE mail. No server messages are changed."))
	return b.String()
}
func (m Model) Plan() Plan { return m.plan() }
