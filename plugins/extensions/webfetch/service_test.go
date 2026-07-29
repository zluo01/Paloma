package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode/utf8"

	pb "scry/extensions/webfetch/internal/pb/schema/extension"
	"scry/extensions/webfetch/internal/transport"
)

// stubTool blocks until released, so a test can observe an invocation that is
// still in flight.
type stubTool struct {
	started  chan struct{}
	release  chan struct{}
	lastArgs chan string
}

func newStubTool() *stubTool {
	return &stubTool{
		started:  make(chan struct{}, 1),
		release:  make(chan struct{}),
		lastArgs: make(chan string, 1),
	}
}

func (s *stubTool) facet() *pb.ToolFacet {
	return &pb.ToolFacet{Description: "stub", Parameters: `{"type":"object"}`}
}

func (s *stubTool) invoke(ctx context.Context, callID, arguments string) (*pb.ToolContent, error) {
	s.started <- struct{}{}
	s.lastArgs <- arguments

	select {
	case <-s.release:
		return &pb.ToolContent{
			Tag:  "web_fetch",
			Body: &pb.ToolContent_Text{Text: "done:" + callID},
		}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

type harness struct {
	stdin  *io.PipeWriter
	writer *transport.Writer
	out    *lockedBuffer
	served chan error
}

// lockedBuffer serializes reads against the service's concurrent writes.
type lockedBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *lockedBuffer) snapshot() []byte {
	b.mu.Lock()
	defer b.mu.Unlock()
	return bytes.Clone(b.buf.Bytes())
}

func start(t *testing.T, tl tool) *harness {
	t.Helper()

	reader, writer := io.Pipe()
	out := &lockedBuffer{}
	svc := newService(tl, transport.NewWriter(out))

	h := &harness{stdin: writer, writer: transport.NewWriter(writer), out: out, served: make(chan error, 1)}
	go func() { h.served <- svc.serve(transport.NewReader(reader)) }()

	t.Cleanup(func() { _ = writer.Close() })
	return h
}

func (h *harness) request(t *testing.T, request *pb.RequestEvent) {
	t.Helper()
	if err := h.writer.Write(request); err != nil {
		t.Fatalf("write request: %v", err)
	}
}

func (h *harness) awaitResponses(t *testing.T, n int) []*pb.ResponseEvent {
	t.Helper()

	deadline := time.After(2 * time.Second)
	for {
		responses := decodeAll(h.out.snapshot())
		if len(responses) >= n {
			return responses
		}
		select {
		case <-deadline:
			t.Fatalf("got %d responses, want %d", len(responses), n)
		case <-time.After(time.Millisecond):
		}
	}
}

func decodeAll(raw []byte) []*pb.ResponseEvent {
	var responses []*pb.ResponseEvent
	reader := transport.NewReader(bytes.NewReader(raw))
	for {
		response := &pb.ResponseEvent{}
		if err := reader.Read(response); err != nil {
			return responses
		}
		responses = append(responses, response)
	}
}

func invokeRequest(eventID uint64, sessionID, callID, arguments string) *pb.RequestEvent {
	return &pb.RequestEvent{
		EventId: eventID,
		Payload: &pb.RequestEvent_InvokeToolRequest{
			InvokeToolRequest: &pb.InvokeToolRequest{
				SessionId: sessionID,
				CallId:    callID,
				Arguments: arguments,
			},
		},
	}
}

func cancelRequest(eventID uint64, sessionID string) *pb.RequestEvent {
	return &pb.RequestEvent{
		EventId: eventID,
		Payload: &pb.RequestEvent_CancelToolRequest{
			CancelToolRequest: &pb.CancelToolRequest{SessionId: sessionID},
		},
	}
}

func TestHandshakeAdvertisesToolOnlyCapability(t *testing.T) {
	h := start(t, newStubTool())

	h.request(t, &pb.RequestEvent{
		EventId: 1,
		Payload: &pb.RequestEvent_HandshakeRequest{HandshakeRequest: &pb.HandshakeRequest{}},
	})

	got := h.awaitResponses(t, 1)[0].GetHandshakeResponse()
	if got.GetVersion() != protocolVersion {
		t.Errorf("version = %d, want %d", got.GetVersion(), protocolVersion)
	}
	if got.GetExtensionId() != extensionID {
		t.Errorf("extension id = %q, want %q", got.GetExtensionId(), extensionID)
	}
	if len(got.GetCapabilities()) != 1 {
		t.Fatalf("capabilities = %d, want 1", len(got.GetCapabilities()))
	}
	capability := got.GetCapabilities()[0]
	if capability.GetTool() == nil {
		t.Error("capability has no tool facet")
	}
	if capability.GetSearch() != nil {
		t.Error("tool-only capability must not advertise search")
	}
}

func TestInvokeReturnsToolContent(t *testing.T) {
	stub := newStubTool()
	h := start(t, stub)
	close(stub.release)

	h.request(t, invokeRequest(2, "session-a", "call-a", `{"url":"https://example.com"}`))

	got := h.awaitResponses(t, 1)[0]
	if got.GetEventId() != 2 {
		t.Errorf("event id = %d, want 2", got.GetEventId())
	}
	content := got.GetInvokeToolResponse().GetContent()
	if content.GetText() != "done:call-a" {
		t.Errorf("text = %q, want %q", content.GetText(), "done:call-a")
	}
	if args := <-stub.lastArgs; args != `{"url":"https://example.com"}` {
		t.Errorf("arguments = %q", args)
	}
}

func TestCancelAbortsInFlightInvocation(t *testing.T) {
	stub := newStubTool()
	h := start(t, stub)

	h.request(t, invokeRequest(3, "session-b", "call-b", "{}"))
	<-stub.started // the invocation is now blocked inside the tool

	h.request(t, cancelRequest(4, "session-b"))

	// the cancel ack and the aborted invocation both come back
	responses := h.awaitResponses(t, 2)
	byEvent := map[uint64]*pb.ResponseEvent{}
	for _, response := range responses {
		byEvent[response.GetEventId()] = response
	}
	if byEvent[4].GetCancelToolResponse() == nil {
		t.Error("cancel was not acknowledged")
	}
	if byEvent[3].GetExtensionError() == nil {
		t.Error("cancelled invocation did not report an error")
	}
}

func TestCancelLeavesOtherSessionsRunning(t *testing.T) {
	stub := newStubTool()
	h := start(t, stub)

	h.request(t, invokeRequest(5, "session-c", "call-c", "{}"))
	<-stub.started

	h.request(t, cancelRequest(6, "other-session"))

	responses := h.awaitResponses(t, 1)
	if len(responses) != 1 || responses[0].GetEventId() != 6 {
		t.Fatalf("expected only the cancel ack, got %d responses", len(responses))
	}

	close(stub.release)
	for _, response := range h.awaitResponses(t, 2) {
		if response.GetEventId() == 5 && response.GetInvokeToolResponse() == nil {
			t.Error("untouched session should have completed normally")
		}
	}
}

func TestSearchRequestIsRejected(t *testing.T) {
	h := start(t, newStubTool())

	h.request(t, &pb.RequestEvent{
		EventId: 7,
		Payload: &pb.RequestEvent_SearchRequest{
			SearchRequest: &pb.SearchRequest{Input: "anything"},
		},
	})

	if got := h.awaitResponses(t, 1)[0]; got.GetExtensionError() == nil {
		t.Error("search against a tool-only capability should error")
	}
}

func TestUnknownCapabilityIsRejected(t *testing.T) {
	h := start(t, newStubTool())

	other := "SomethingElse"
	h.request(t, &pb.RequestEvent{
		EventId:      8,
		CapabilityId: &other,
		Payload:      &pb.RequestEvent_HandshakeRequest{HandshakeRequest: &pb.HandshakeRequest{}},
	})

	if got := h.awaitResponses(t, 1)[0]; got.GetExtensionError() == nil {
		t.Error("unknown capability id should error")
	}
}

// panickingTool simulates a converter blowing up on pathological content.
type panickingTool struct{}

func (panickingTool) facet() *pb.ToolFacet {
	return &pb.ToolFacet{Description: "boom", Parameters: "{}"}
}

func (panickingTool) invoke(context.Context, string, string) (*pb.ToolContent, error) {
	panic("render exploded")
}

func TestWorkerPanicBecomesAnErrorResponse(t *testing.T) {
	h := start(t, panickingTool{})

	h.request(t, invokeRequest(50, "session-p", "call-p", "{}"))

	got := h.awaitResponses(t, 1)[0].GetExtensionError()
	if got == nil {
		t.Fatal("panic did not come back as an error response")
	}
	if !strings.Contains(got.GetError(), "panic") {
		t.Errorf("error = %q, want the panic named", got.GetError())
	}

	// one poisoned page must not take the service down for everyone else
	h.request(t, &pb.RequestEvent{
		EventId: 51,
		Payload: &pb.RequestEvent_HandshakeRequest{HandshakeRequest: &pb.HandshakeRequest{}},
	})
	if h.awaitResponses(t, 2)[1].GetHandshakeResponse() == nil {
		t.Error("service stopped serving after a worker panic")
	}
}

func TestServeCancelsInFlightWorkOnEOF(t *testing.T) {
	stub := newStubTool()
	h := start(t, stub)

	h.request(t, invokeRequest(9, "session-d", "call-d", "{}"))
	<-stub.started

	_ = h.stdin.Close() // host shut down while the tool is still running

	// the shutdown must cancel the invocation rather than wait out its
	// timeout against a host that is gone
	select {
	case err := <-h.served:
		if err != nil {
			t.Fatalf("serve: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("serve did not cancel in-flight work on shutdown")
	}

	if got := h.awaitResponses(t, 1)[0]; got.GetExtensionError() == nil {
		t.Error("cancelled invocation should still have reported before exit")
	}
}

// errorTool fails every invocation with a fixed error.
type errorTool struct{ err error }

func (e errorTool) facet() *pb.ToolFacet {
	return &pb.ToolFacet{Description: "stub", Parameters: "{}"}
}

func (e errorTool) invoke(context.Context, string, string) (*pb.ToolContent, error) {
	return nil, e.err
}

func TestInvokeErrorWithInvalidUTF8StillResponds(t *testing.T) {
	h := start(t, errorTool{err: errors.New("bad \xff byte in a wrapped error")})

	h.request(t, invokeRequest(30, "session-f", "call-f", "{}"))

	got := h.awaitResponses(t, 1)[0].GetExtensionError()
	if got == nil {
		t.Fatal("invalid utf-8 in the error dropped the response entirely")
	}
	if !utf8.ValidString(got.GetError()) {
		t.Errorf("error message is not valid utf-8: %q", got.GetError())
	}
	if !strings.Contains(got.GetError(), "bad") {
		t.Errorf("error text lost: %q", got.GetError())
	}
}

func TestServeTreatsMidFrameEOFAsShutdown(t *testing.T) {
	h := start(t, newStubTool())

	// a length prefix promising 16 bytes, then only one before the pipe
	// closes — what a host killed mid-write leaves behind
	_, _ = h.stdin.Write([]byte{0x10, 0x08})
	_ = h.stdin.Close()

	select {
	case err := <-h.served:
		if err != nil {
			t.Fatalf("serve = %v, want nil for a mid-frame EOF", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("serve did not return")
	}
}

func TestServeReportsProtocolError(t *testing.T) {
	h := start(t, newStubTool())

	// varint length prefix claiming 5 MiB, past the transport's 4 MiB cap
	_, _ = h.stdin.Write([]byte{0x80, 0x80, 0xc0, 0x02})

	select {
	case err := <-h.served:
		if err == nil {
			t.Fatal("serve = nil, want an error for an oversized frame")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("serve did not return")
	}
}

func TestCancelAbortsAllInvocationsOfASession(t *testing.T) {
	stub := newStubTool()
	h := start(t, stub)

	h.request(t, invokeRequest(20, "session-e", "call-e1", "{}"))
	h.request(t, invokeRequest(21, "session-e", "call-e2", "{}"))
	for range 2 {
		<-stub.started
		<-stub.lastArgs
	}

	h.request(t, cancelRequest(22, "session-e"))

	byEvent := map[uint64]*pb.ResponseEvent{}
	for _, response := range h.awaitResponses(t, 3) {
		byEvent[response.GetEventId()] = response
	}
	if byEvent[22].GetCancelToolResponse() == nil {
		t.Error("cancel was not acknowledged")
	}
	for _, eventID := range []uint64{20, 21} {
		if byEvent[eventID].GetExtensionError() == nil {
			t.Errorf("invocation %d survived a session-wide cancel", eventID)
		}
	}
}
