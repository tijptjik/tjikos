package main

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"io"
	"math"
	"os"
	"strings"
	"sync"

	"github.com/charmbracelet/x/ansi"
	"github.com/charmbracelet/x/ansi/kitty"
	"golang.org/x/image/draw"
	_ "golang.org/x/image/webp"
	"golang.org/x/sys/unix"
)

// The renderer writes the text frame first, then this writer paints the inline
// image in its reserved cells. This avoids racing Bubble Tea's renderer from a
// background command. Only this process's graphics ID is ever removed.
type graphicsWriter struct {
	mu        sync.Mutex
	out       io.Writer
	paint     string
	active    bool
	supported bool
	id        int
}

func (g *graphicsWriter) Fd() uintptr                { return os.Stdout.Fd() }
func (g *graphicsWriter) Read(p []byte) (int, error) { return os.Stdout.Read(p) }
func (g *graphicsWriter) Close() error               { return nil }
func (g *graphicsWriter) Write(p []byte) (int, error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	n, err := g.out.Write(p)
	if err != nil {
		return n, err
	}
	if g.supported {
		if g.active {
			_, err = io.WriteString(g.out, g.clear())
			g.active = false
		}
		if g.paint != "" {
			_, err = io.WriteString(g.out, g.paint)
			g.active = true
		}
	}
	return n, err
}
func (g *graphicsWriter) clear() string {
	return ansi.KittyGraphics(nil, fmt.Sprintf("a=d,d=I,i=%d,q=2", g.id))
}
func (g *graphicsWriter) set(paint string) { g.mu.Lock(); g.paint = paint; g.mu.Unlock() }
func (g *graphicsWriter) cleanup() {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.supported {
		io.WriteString(g.out, g.clear())
	}
	g.paint = ""
	g.active = false
}
func newGraphics() *graphicsWriter {
	return &graphicsWriter{out: os.Stdout, supported: (strings.Contains(os.Getenv("TERM"), "kitty") || os.Getenv("KITTY_WINDOW_ID") != "") && os.Getenv("TMUX") == "", id: 19000000 + os.Getpid()%1000000}
}
func (g *graphicsWriter) prepare(item InlineImage, cols, rows, row, column int) (string, error) {
	if !g.supported || cols < 1 || rows < 1 {
		return "", nil
	}
	data, err := base64.StdEncoding.DecodeString(item.Data)
	if err != nil {
		return "", err
	}
	dimensions, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return "", err
	}
	if dimensions.Width < 1 || dimensions.Height < 1 || int64(dimensions.Width)*int64(dimensions.Height) > 40_000_000 {
		return "", fmt.Errorf("image exceeds the safe display size; original is retained")
	}
	src, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return "", err
	}
	cellW, cellH := 8, 16
	if size, err := unix.IoctlGetWinsize(int(os.Stdout.Fd()), unix.TIOCGWINSZ); err == nil && size.Col > 0 && size.Row > 0 && size.Xpixel > 0 && size.Ypixel > 0 {
		cellW = max(1, int(size.Xpixel)/int(size.Col))
		cellH = max(1, int(size.Ypixel)/int(size.Row))
	}
	scale := math.Min(1, math.Min(float64(cols*cellW)/float64(dimensions.Width), float64(rows*cellH)/float64(dimensions.Height)))
	width, height := max(1, int(float64(dimensions.Width)*scale)), max(1, int(float64(dimensions.Height)*scale))
	dst := image.NewRGBA(image.Rect(0, 0, width, height))
	draw.ApproxBiLinear.Scale(dst, dst.Bounds(), src, src.Bounds(), draw.Over, nil)
	var buffer bytes.Buffer
	// Cursor coordinates are bounded by the allocated panel and terminal cells.
	fmt.Fprintf(&buffer, "\x1b7\x1b[%d;%dH", row, column)
	err = kitty.EncodeGraphics(&buffer, dst, &kitty.Options{Action: kitty.TransmitAndPut, ID: g.id, Format: kitty.PNG, Transmission: kitty.Direct, Quite: 2, Chunk: true, DoNotMoveCursor: true, Columns: min(cols, (width+cellW-1)/cellW), Rows: min(rows, (height+cellH-1)/cellH)})
	buffer.WriteString("\x1b8")
	return buffer.String(), err
}

// A small ANSI thumbnail keeps non-Kitty terminals (including VHS) useful.
// Kitty receives the higher-resolution image through its graphics protocol.
func thumbnail(item InlineImage, cols, rows int) (string, error) {
	if os.Getenv("NO_COLOR") != "" {
		return "", nil
	}
	data, err := base64.StdEncoding.DecodeString(item.Data)
	if err != nil {
		return "", err
	}
	size, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return "", err
	}
	if size.Width < 1 || size.Height < 1 || int64(size.Width)*int64(size.Height) > 40_000_000 {
		return "", fmt.Errorf("image exceeds display limit")
	}
	src, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return "", err
	}
	scale := math.Min(float64(cols)/float64(size.Width), float64(rows*2)/float64(size.Height))
	width, height := max(1, int(float64(size.Width)*scale)), max(2, int(float64(size.Height)*scale))
	dst := image.NewRGBA(image.Rect(0, 0, width, height))
	draw.ApproxBiLinear.Scale(dst, dst.Bounds(), src, src.Bounds(), draw.Over, nil)
	var result strings.Builder
	for y := 0; y < height; y += 2 {
		for x := 0; x < width; x++ {
			top := dst.RGBAAt(x, y)
			bottom := dst.RGBAAt(x, min(y+1, height-1))
			fmt.Fprintf(&result, "\x1b[38;2;%d;%d;%dm\x1b[48;2;%d;%d;%dm▀", top.R, top.G, top.B, bottom.R, bottom.G, bottom.B)
		}
		result.WriteString("\x1b[0m")
		if y+2 < height {
			result.WriteByte('\n')
		}
	}
	return result.String(), nil
}
