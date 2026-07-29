// Package transport frames protobuf messages on the plugin's stdio pipes.
//
// Wire format is a varint byte length followed by the encoded message, the
// same framing the host and the official protobuf runtimes use.
package transport

import (
	"bufio"
	"io"
	"strings"
	"sync"
	"unicode/utf8"

	"google.golang.org/protobuf/encoding/protodelim"
	"google.golang.org/protobuf/encoding/protowire"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protoreflect"
)

// maxFrameBytes bounds a single inbound frame. Requests carry tool arguments,
// which are orders of magnitude smaller.
const maxFrameBytes = 4 << 20

var unmarshalOpts = protodelim.UnmarshalOptions{MaxSize: maxFrameBytes}

// Reader decodes frames from the host. Not safe for concurrent use; the
// service reads on a single goroutine.
type Reader struct {
	buf *bufio.Reader
}

func NewReader(r io.Reader) *Reader {
	// sized so real frames decode straight out of the buffer instead of
	// protodelim's allocate-and-copy fallback above the bufio default
	return &Reader{buf: bufio.NewReaderSize(r, 64<<10)}
}

// Read returns io.EOF once the host closes the pipe.
func (r *Reader) Read(m proto.Message) error {
	return unmarshalOpts.UnmarshalFrom(r.buf, m)
}

// Writer encodes frames back to the host. Every write is flushed and
// serialized, so concurrent tool invocations cannot interleave partial
// frames.
type Writer struct {
	mu    sync.Mutex
	buf   *bufio.Writer
	frame []byte
}

func NewWriter(w io.Writer) *Writer {
	return &Writer{buf: bufio.NewWriter(w)}
}

// Write frames m onto the pipe. The protobuf encoder rejects invalid UTF-8
// in string fields, and message content includes bytes fetched from the web
// — rather than drop the frame and leave the host waiting out its request
// timeout, invalid strings are replaced with U+FFFD and the marshal retried.
func (w *Writer) Write(m proto.Message) error {
	w.mu.Lock()
	defer w.mu.Unlock()

	var opts proto.MarshalOptions
	frame, err := opts.MarshalAppend(w.frame[:0], m)
	if err != nil {
		sanitizeStrings(m.ProtoReflect())
		if frame, err = opts.MarshalAppend(w.frame[:0], m); err != nil {
			return err
		}
	}
	w.frame = frame

	var varint [10]byte
	if _, err := w.buf.Write(protowire.AppendVarint(varint[:0], uint64(len(frame)))); err != nil {
		return err
	}
	if _, err := w.buf.Write(frame); err != nil {
		return err
	}
	return w.buf.Flush()
}

// sanitizeStrings rewrites every string in m to valid UTF-8, covering the
// field shapes the extension schema uses: strings, messages, and lists of
// either. It only runs after a marshal has already failed.
func sanitizeStrings(m protoreflect.Message) {
	m.Range(func(fd protoreflect.FieldDescriptor, v protoreflect.Value) bool {
		switch {
		case fd.IsList():
			list := v.List()
			for i := 0; i < list.Len(); i++ {
				switch fd.Kind() {
				case protoreflect.StringKind:
					list.Set(i, protoreflect.ValueOfString(validUTF8(list.Get(i).String())))
				case protoreflect.MessageKind:
					sanitizeStrings(list.Get(i).Message())
				}
			}
		case fd.Kind() == protoreflect.StringKind:
			m.Set(fd, protoreflect.ValueOfString(validUTF8(v.String())))
		case fd.Kind() == protoreflect.MessageKind:
			sanitizeStrings(v.Message())
		}
		return true
	})
}

func validUTF8(s string) string {
	if utf8.ValidString(s) {
		return s
	}
	return strings.ToValidUTF8(s, "�")
}
