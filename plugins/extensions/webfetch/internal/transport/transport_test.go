package transport

import (
	"bytes"
	"errors"
	"io"
	"strings"
	"sync"
	"testing"
	"unicode/utf8"

	pb "scry/extensions/webfetch/internal/pb/schema/extension"
)

func handshake(id uint64) *pb.RequestEvent {
	return &pb.RequestEvent{
		EventId: id,
		Payload: &pb.RequestEvent_HandshakeRequest{
			HandshakeRequest: &pb.HandshakeRequest{},
		},
	}
}

func TestRoundTripPreservesFrames(t *testing.T) {
	var pipe bytes.Buffer
	w := NewWriter(&pipe)

	for id := uint64(1); id <= 3; id++ {
		if err := w.Write(handshake(id)); err != nil {
			t.Fatalf("write %d: %v", id, err)
		}
	}

	r := NewReader(&pipe)
	for want := uint64(1); want <= 3; want++ {
		var got pb.RequestEvent
		if err := r.Read(&got); err != nil {
			t.Fatalf("read %d: %v", want, err)
		}
		if got.GetEventId() != want {
			t.Fatalf("event id = %d, want %d", got.GetEventId(), want)
		}
	}
}

func TestReadReportsEOFOnClosedPipe(t *testing.T) {
	r := NewReader(bytes.NewReader(nil))

	err := r.Read(&pb.RequestEvent{})

	if !errors.Is(err, io.EOF) {
		t.Fatalf("err = %v, want io.EOF", err)
	}
}

func TestPartialFrameIsNotDecoded(t *testing.T) {
	var pipe bytes.Buffer
	if err := NewWriter(&pipe).Write(handshake(7)); err != nil {
		t.Fatalf("write: %v", err)
	}
	full := pipe.Bytes()

	r := NewReader(bytes.NewReader(full[:len(full)-1]))

	if err := r.Read(&pb.RequestEvent{}); err == nil {
		t.Fatal("truncated frame decoded without error")
	}
}

func TestConcurrentWritesDoNotInterleave(t *testing.T) {
	const writers = 8
	var pipe bytes.Buffer
	w := NewWriter(&pipe)

	var wg sync.WaitGroup
	for i := range writers {
		wg.Go(func() {
			if err := w.Write(handshake(uint64(i + 1))); err != nil {
				t.Errorf("write: %v", err)
			}
		})
	}
	wg.Wait()

	// every frame must still decode, and each id must appear exactly once
	seen := map[uint64]bool{}
	r := NewReader(&pipe)
	for range writers {
		var got pb.RequestEvent
		if err := r.Read(&got); err != nil {
			t.Fatalf("read: %v", err)
		}
		if seen[got.GetEventId()] {
			t.Fatalf("duplicate event id %d", got.GetEventId())
		}
		seen[got.GetEventId()] = true
	}
	if len(seen) != writers {
		t.Fatalf("decoded %d frames, want %d", len(seen), writers)
	}
}

func TestWriteSanitizesInvalidUTF8Strings(t *testing.T) {
	var pipe bytes.Buffer
	event := &pb.ResponseEvent{
		EventId: 5,
		Payload: &pb.ResponseEvent_ExtensionError{
			ExtensionError: &pb.ExtensionError{Error: "bad \xff byte"},
		},
	}

	if err := NewWriter(&pipe).Write(event); err != nil {
		t.Fatalf("write: %v", err)
	}

	got := &pb.ResponseEvent{}
	if err := NewReader(&pipe).Read(got); err != nil {
		t.Fatalf("read back: %v", err)
	}
	message := got.GetExtensionError().GetError()
	if !utf8.ValidString(message) {
		t.Errorf("message is not valid utf-8: %q", message)
	}
	if !strings.Contains(message, "bad") {
		t.Errorf("message text lost: %q", message)
	}
}

func TestOversizedInboundFrameIsRejected(t *testing.T) {
	// varint length prefix claiming 5 MiB, past maxFrameBytes
	raw := append([]byte{0x80, 0x80, 0xc0, 0x02}, make([]byte, 64)...)
	r := NewReader(bytes.NewReader(raw))

	err := r.Read(&pb.RequestEvent{})

	if err == nil || errors.Is(err, io.EOF) {
		t.Fatalf("err = %v, want a size rejection", err)
	}
}
