package fetch

import (
	"compress/gzip"
	"context"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"
)

// testClient skips the private-address screen so tests can reach httptest,
// which always listens on loopback.
func testClient() *Client {
	return newClient(func(netip.Addr) bool { return false })
}

func get(t *testing.T, url string) (*Result, error) {
	t.Helper()
	return testClient().Get(context.Background(), url, 5*time.Second)
}

func TestGetReturnsBodyAndMetadata(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		_, _ = w.Write([]byte("<h1>hello</h1>"))
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if got.Status != http.StatusOK {
		t.Errorf("status = %d, want 200", got.Status)
	}
	if string(got.Body) != "<h1>hello</h1>" {
		t.Errorf("body = %q", got.Body)
	}
	if !strings.HasPrefix(got.ContentType, "text/html") {
		t.Errorf("content type = %q", got.ContentType)
	}
	if got.Truncated {
		t.Error("small body reported as truncated")
	}
}

func TestOversizedBodyIsTruncated(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		chunk := strings.Repeat("a", 1<<20)
		for range 6 { // 6 MiB, past MaxBytes
			_, _ = w.Write([]byte(chunk))
		}
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if !got.Truncated {
		t.Error("oversized body not flagged as truncated")
	}
	if len(got.Body) != MaxBytes {
		t.Errorf("body = %d bytes, want %d", len(got.Body), MaxBytes)
	}
}

func TestBodyExactlyAtCapIsNotTruncated(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(strings.Repeat("a", MaxBytes)))
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if got.Truncated {
		t.Error("body sitting exactly on the cap reported as truncated")
	}
	if len(got.Body) != MaxBytes {
		t.Errorf("body = %d bytes, want %d", len(got.Body), MaxBytes)
	}
}

func TestInterruptedBodyKeepsPartialContent(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		// promise more than is written; closing the connection mid-body
		// gives the client an unexpected EOF after the partial read
		w.Header().Set("Content-Length", "1000")
		_, _ = w.Write([]byte("partial data"))
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if string(got.Body) != "partial data" {
		t.Errorf("body = %q, want the partial content", got.Body)
	}
	if !got.Truncated {
		t.Error("interrupted body not flagged as truncated")
	}
	if got.ReadErr == nil {
		t.Error("interrupted body did not surface its read error")
	}
}

func TestRedirectsAreFollowedAndFinalURLReported(t *testing.T) {
	final := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("arrived"))
	}))
	defer final.Close()

	entry := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, final.URL, http.StatusFound)
	}))
	defer entry.Close()

	got, err := get(t, entry.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if string(got.Body) != "arrived" {
		t.Errorf("body = %q", got.Body)
	}
	if got.URL != final.URL {
		t.Errorf("url = %q, want the redirect target %q", got.URL, final.URL)
	}
}

func TestExactlyMaxRedirectsAreFollowed(t *testing.T) {
	final := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("arrived"))
	}))
	defer final.Close()

	chain := func(hops int) string {
		next := final.URL
		for range hops {
			target := next
			hop := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				http.Redirect(w, r, target, http.StatusFound)
			}))
			t.Cleanup(hop.Close)
			next = hop.URL
		}
		return next
	}

	got, err := get(t, chain(maxRedirects))
	if err != nil {
		t.Fatalf("a chain of exactly %d redirects should succeed: %v", maxRedirects, err)
	}
	if string(got.Body) != "arrived" {
		t.Errorf("body = %q", got.Body)
	}

	if _, err := get(t, chain(maxRedirects+1)); err == nil {
		t.Fatalf("a chain of %d redirects should be refused", maxRedirects+1)
	}
}

func TestRedirectLoopIsStopped(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, server.URL, http.StatusFound)
	}))
	defer server.Close()

	if _, err := get(t, server.URL); err == nil {
		t.Fatal("redirect loop did not error")
	}
}

func TestRedirectToDisallowedSchemeIsRejected(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "ftp://example.com/file", http.StatusFound)
	}))
	defer server.Close()

	_, err := get(t, server.URL)

	if err == nil || !strings.Contains(err.Error(), "scheme") {
		t.Fatalf("err = %v, want a scheme rejection", err)
	}
}

func TestCancellationStopsTheRequest(t *testing.T) {
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		<-release
	}))
	defer server.Close()
	defer close(release)

	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		time.Sleep(20 * time.Millisecond)
		cancel()
	}()

	_, err := testClient().Get(ctx, server.URL, MaxTimeout)

	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestTransportPolicyPins(t *testing.T) {
	transport := testClient().http.Transport.(*http.Transport)

	// deliberate: honoring HTTP(S)_PROXY would route requests around the
	// dial-time address screen
	if transport.Proxy != nil {
		t.Error("proxy support would bypass the address screen; its absence is a policy pin")
	}
	if transport.IdleConnTimeout == 0 {
		t.Error("idle connections must not pool forever")
	}
	// the request context enforces timeout_seconds; a fixed header timeout
	// would silently narrow that contract
	if got := transport.ResponseHeaderTimeout; got != 0 {
		t.Errorf("ResponseHeaderTimeout = %v, want none", got)
	}
}

func TestCancellationDiscardsPartialBody(t *testing.T) {
	release := make(chan struct{})
	started := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("partial"))
		w.(http.Flusher).Flush()
		close(started)
		<-release
	}))
	defer server.Close()
	defer close(release)

	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		<-started
		cancel()
	}()

	// partial content survives network failures, but not an explicit
	// cancel: nobody is waiting for the result
	_, err := testClient().Get(ctx, server.URL, MaxTimeout)

	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestTimeoutIsReported(t *testing.T) {
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		<-release
	}))
	defer server.Close()
	defer close(release)

	_, err := testClient().Get(context.Background(), server.URL, 30*time.Millisecond)

	if err == nil || !strings.Contains(err.Error(), "timed out") {
		t.Fatalf("err = %v, want a timeout", err)
	}
}

func TestLoopbackIsBlockedByDefaultClient(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("should never be reached"))
	}))
	defer server.Close()

	_, err := NewClient().Get(context.Background(), server.URL, 5*time.Second)

	if !errors.Is(err, ErrBlockedAddress) {
		t.Fatalf("err = %v, want ErrBlockedAddress", err)
	}
	if !strings.Contains(err.Error(), "127.0.0.1") {
		t.Errorf("err = %v, want the blocked address named", err)
	}
}

func TestIPv6LoopbackLiteralRoundTrip(t *testing.T) {
	listener, err := net.Listen("tcp", "[::1]:0")
	if err != nil {
		t.Skipf("ipv6 loopback unavailable: %v", err)
	}
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("over v6"))
	}))
	server.Listener.Close()
	server.Listener = listener
	server.Start()
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get %s: %v", server.URL, err)
	}
	if string(got.Body) != "over v6" {
		t.Errorf("body = %q", got.Body)
	}
	if !strings.Contains(got.URL, "[::1]") {
		t.Errorf("url = %q, want the bracket literal preserved", got.URL)
	}

	if _, err := NewClient().Get(context.Background(), server.URL, 5*time.Second); !errors.Is(err, ErrBlockedAddress) {
		t.Errorf("default client err = %v, want ErrBlockedAddress for [::1]", err)
	}
}

func TestNon2xxStatusReturnsBodyNotError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "no such page", http.StatusNotFound)
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if got.Status != http.StatusNotFound {
		t.Errorf("status = %d, want 404", got.Status)
	}
	if !strings.Contains(string(got.Body), "no such page") {
		t.Errorf("body = %q, want the error page content", got.Body)
	}
}

func TestGzipBodyIsTransparentlyDecompressed(t *testing.T) {
	plain := strings.Repeat("hello compressed world ", 100)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Encoding", "gzip")
		zw := gzip.NewWriter(w)
		_, _ = zw.Write([]byte(plain))
		_ = zw.Close()
	}))
	defer server.Close()

	got, err := get(t, server.URL)
	if err != nil {
		t.Fatalf("get: %v", err)
	}

	if string(got.Body) != plain {
		t.Errorf("body was not decompressed: got %d bytes, want %d", len(got.Body), len(plain))
	}
}

func TestParseURLRejectsBadInput(t *testing.T) {
	cases := map[string]string{
		"empty":          "",
		"blank":          "   ",
		"no scheme":      "example.com/page",
		"file scheme":    "file:///etc/passwd",
		"ftp scheme":     "ftp://example.com",
		"missing host":   "http://",
		"javascript uri": "javascript:alert(1)",
	}

	for name, raw := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := parseURL(raw); err == nil {
				t.Errorf("parseURL(%q) succeeded, want an error", raw)
			}
		})
	}
}

func TestParseURLAcceptsHTTPAndHTTPS(t *testing.T) {
	for _, raw := range []string{"http://example.com", "https://example.com/a?b=c", "  https://example.com  "} {
		if _, err := parseURL(raw); err != nil {
			t.Errorf("parseURL(%q) = %v", raw, err)
		}
	}
}

func TestBlockedCoversPrivateSpace(t *testing.T) {
	blockedAddrs := []string{
		"127.0.0.1", "::1", // loopback
		"10.0.0.5", "192.168.1.1", "172.16.0.1", "fd00::1", // private
		"169.254.169.254", // link-local, the cloud metadata endpoint
		"0.0.0.0", "::",   // unspecified
		"100.64.0.1",        // carrier-grade NAT
		"224.0.0.1",         // multicast
		"255.255.255.255",   // broadcast
		"198.18.0.5",        // benchmarking
		"192.0.2.1",         // documentation
		"::ffff:127.0.0.1",  // ipv4-mapped loopback
		"fe80::1%eth0",      // zoned link-local
		"fec0::1",           // deprecated site-local
		"2001:db8::1",       // documentation
		"64:ff9b::7f00:1",   // NAT64 embedding loopback 127.0.0.1
		"64:ff9b::a00:0001", // NAT64 embedding private 10.0.0.1
	}
	for _, raw := range blockedAddrs {
		if !blocked(netip.MustParseAddr(raw)) {
			t.Errorf("blocked(%s) = false, want true", raw)
		}
	}

	allowedAddrs := []string{
		"1.1.1.1", "93.184.216.34", "2606:4700:4700::1111", "100.128.0.1",
		"64:ff9b::808:808", // NAT64 embedding public 8.8.8.8
	}
	for _, raw := range allowedAddrs {
		if blocked(netip.MustParseAddr(raw)) {
			t.Errorf("blocked(%s) = true, want false", raw)
		}
	}
}

func TestClampTimeout(t *testing.T) {
	cases := []struct {
		in   time.Duration
		want time.Duration
	}{
		{0, DefaultTimeout},
		{-1 * time.Second, DefaultTimeout},
		{10 * time.Second, 10 * time.Second},
		{MaxTimeout + time.Minute, MaxTimeout},
	}
	for _, c := range cases {
		if got := clampTimeout(c.in); got != c.want {
			t.Errorf("clampTimeout(%v) = %v, want %v", c.in, got, c.want)
		}
	}
}
