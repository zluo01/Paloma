// Command webfetch is a Scry extension that fetches a URL for the model.
//
// It speaks the extension protocol over stdio: varint-delimited RequestEvent
// frames in on stdin, ResponseEvent frames out on stdout. Stdout carries
// protocol data only; logging goes to stderr.
package main

import (
	"log"
	"os"

	"scry/extensions/webfetch/internal/transport"
)

func main() {
	log.SetFlags(0)
	log.SetPrefix("webfetch: ")

	// the protocol owns the real stdout; repointing the package variable
	// sends stray prints from dependencies (html-to-markdown warns via
	// fmt.Println) to stderr instead of corrupting the frame stream. The
	// dependency graph was audited: nothing captures os.Stdout before main
	// or writes to fd 1 directly, so this covers every writer that exists.
	protocolOut := os.Stdout
	os.Stdout = os.Stderr

	service := newService(newWebFetch(), transport.NewWriter(protocolOut))

	if err := service.serve(transport.NewReader(os.Stdin)); err != nil {
		log.Printf("fatal: %v", err)
		os.Exit(1)
	}
}
