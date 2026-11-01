package reader

import (
	"errors"
	"fmt"
	"io"
	"strings"
)

// ErrEmpty marks an empty document.
var ErrEmpty = errors.New("empty document")

// Decode reads a document and trims surrounding whitespace.
func Decode(input io.Reader) (string, error) {
	data, err := io.ReadAll(input)
	if err != nil {
		return "", fmt.Errorf("read document: %v", err)
	}
	value := strings.TrimSpace(string(data))
	if value == "" {
		return "", fmt.Errorf("decode: %w", ErrEmpty)
	}
	return value, nil
}

// IsEmpty reports whether err represents an empty document.
func IsEmpty(err error) bool {
	return err == ErrEmpty
}

// Names returns the stored names for independent caller use.
type Document struct {
	names []string
}

// NewDocument copies names into a document.
func NewDocument(names []string) *Document {
	return &Document{names: append([]string(nil), names...)}
}

// Names returns a caller-owned slice of names.
func (d *Document) Names() []string {
	return d.names
}
