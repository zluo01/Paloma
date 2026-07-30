package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log"
	"sync"

	pb "paloma/extensions/webfetch/internal/pb/schema/extension"
	"paloma/extensions/webfetch/internal/transport"
)

const protocolVersion = 1

type tool interface {
	facet() *pb.ToolFacet
	invoke(ctx context.Context, callID, arguments string) (*pb.ToolContent, error)
}

type invocation struct {
	session string
	event   uint64
}

type service struct {
	tool   tool
	writer *transport.Writer

	mu sync.Mutex
	// bridges the protocol's session-scoped cancel to Go's per-invocation
	// contexts, so one CancelToolRequest aborts every in-flight invocation
	// of that session
	inflight map[invocation]context.CancelFunc
	pending  sync.WaitGroup
}

func newService(t tool, w *transport.Writer) *service {
	return &service{
		tool:     t,
		writer:   w,
		inflight: make(map[invocation]context.CancelFunc),
	}
}

// serve reads frames until stdin closes; a host killed mid-frame reads as a
// shutdown, not an error.
func (s *service) serve(r *transport.Reader) error {
	var err error
	for {
		request := &pb.RequestEvent{}
		if readErr := r.Read(request); readErr != nil {
			if !errors.Is(readErr, io.EOF) && !errors.Is(readErr, io.ErrUnexpectedEOF) {
				err = readErr
			}
			break
		}
		s.handle(request)
	}

	// nobody is left to read a late result, so abort in-flight work instead
	// of waiting out fetch timeouts; the drain still lets each goroutine
	// write its final frame before exit
	s.cancelWhere(func(invocation) bool { return true })
	s.pending.Wait()
	return err
}

func (s *service) handle(request *pb.RequestEvent) {
	eventID := request.GetEventId()

	if id := request.GetCapabilityId(); id != "" && id != capabilityID {
		s.fail(eventID, fmt.Sprintf("unknown capability: %s", id))
		return
	}

	switch payload := request.GetPayload().(type) {
	case *pb.RequestEvent_HandshakeRequest:
		s.send(&pb.ResponseEvent{
			EventId: eventID,
			Payload: &pb.ResponseEvent_HandshakeResponse{HandshakeResponse: s.handshake()},
		})

	case *pb.RequestEvent_InvokeToolRequest:
		s.invoke(eventID, payload.InvokeToolRequest)

	case *pb.RequestEvent_CancelToolRequest:
		session := payload.CancelToolRequest.GetSessionId()
		s.cancelWhere(func(key invocation) bool { return key.session == session })
		s.send(&pb.ResponseEvent{
			EventId: eventID,
			Payload: &pb.ResponseEvent_CancelToolResponse{
				CancelToolResponse: &pb.CancelToolResponse{},
			},
		})

	default:
		s.fail(eventID, "unsupported or missing request payload")
	}
}

func (s *service) handshake() *pb.HandshakeResponse {
	return &pb.HandshakeResponse{
		Version:     protocolVersion,
		ExtensionId: extensionID,
		Description: description,
		Capabilities: []*pb.Capability{{
			CapabilityId: capabilityID,
			Description:  description,
			Tool:         s.tool.facet(),
		}},
	}
}

// invoke runs the tool on its own goroutine so the loop keeps reading and a
// cancel for this session can still arrive. Registration happens here on the
// loop goroutine, so a cancel on the very next frame is guaranteed to find
// it.
func (s *service) invoke(eventID uint64, request *pb.InvokeToolRequest) {
	key := invocation{session: request.GetSessionId(), event: eventID}
	ctx, cancel := context.WithCancel(context.Background())
	s.mu.Lock()
	s.inflight[key] = cancel
	s.mu.Unlock()

	// the worker captures plain strings rather than pinning the whole
	// request message for the duration of the fetch
	callID, arguments := request.GetCallId(), request.GetArguments()
	s.pending.Go(func() {
		defer cancel()
		defer s.forget(key)
		// the tool parses arbitrary web content; one poisoned page must
		// cost its own call an error, not the process and every session
		defer func() {
			if r := recover(); r != nil {
				s.fail(eventID, fmt.Sprintf("tool panicked: %v", r))
			}
		}()

		content, err := s.tool.invoke(ctx, callID, arguments)
		if err != nil {
			s.fail(eventID, err.Error())
			return
		}
		s.send(&pb.ResponseEvent{
			EventId: eventID,
			Payload: &pb.ResponseEvent_InvokeToolResponse{
				InvokeToolResponse: &pb.InvokeToolResponse{Content: content},
			},
		})
	})
}

func (s *service) forget(key invocation) {
	s.mu.Lock()
	delete(s.inflight, key)
	s.mu.Unlock()
}

// cancelWhere aborts every tracked invocation match selects, firing the
// cancels outside the lock since an aborted worker immediately calls forget.
func (s *service) cancelWhere(match func(invocation) bool) {
	s.mu.Lock()
	var cancels []context.CancelFunc
	for key, cancel := range s.inflight {
		if match(key) {
			cancels = append(cancels, cancel)
			delete(s.inflight, key)
		}
	}
	s.mu.Unlock()

	for _, cancel := range cancels {
		cancel()
	}
}

func (s *service) send(response *pb.ResponseEvent) {
	if err := s.writer.Write(response); err != nil {
		// the host is gone; the read loop will see EOF and unwind
		log.Printf("failed to write response %d: %v", response.GetEventId(), err)
	}
}

func (s *service) fail(eventID uint64, message string) {
	s.send(&pb.ResponseEvent{
		EventId: eventID,
		Payload: &pb.ResponseEvent_ExtensionError{
			ExtensionError: &pb.ExtensionError{Error: message},
		},
	})
}
