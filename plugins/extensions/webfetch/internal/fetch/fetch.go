// Package fetch retrieves web pages under fixed safety limits: only http(s),
// bounded time, bounded body size, and no requests to private address space.
package fetch

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"syscall"
	"time"
)

const (
	DefaultTimeout = 30 * time.Second
	MaxTimeout     = 120 * time.Second

	// MaxBytes bounds the body read into memory. The host truncates for the
	// model and spills the rest, so this only has to stay sane.
	MaxBytes = 5 << 20

	maxRedirects = 5
	userAgent    = "Mozilla/5.0 (compatible; PalomaWebFetch/1.0)"
)

// ErrBlockedAddress is returned when a host resolves into private or loopback
// address space.
var ErrBlockedAddress = errors.New("refusing to fetch a private or loopback address")

type Result struct {
	// URL after any redirects.
	URL         string
	Status      int
	ContentType string
	Body        []byte
	// Truncated reports that Body is incomplete: it hit MaxBytes, or the
	// connection failed partway through the read.
	Truncated bool
	// ReadErr is the error that ended a partial body read early. Body holds
	// whatever arrived before it and Truncated is set.
	ReadErr error
}

type Client struct {
	http *http.Client
}

func NewClient() *Client {
	return newClient(blocked)
}

// newClient parameterizes the address screen so tests can reach loopback
// httptest servers.
func newClient(isBlocked func(netip.Addr) bool) *Client {
	dialer := &net.Dialer{
		Timeout:   10 * time.Second,
		KeepAlive: 30 * time.Second,
		// Control runs after DNS resolution with the address actually being
		// dialed, so it also covers redirects, every address a hostname
		// resolves to, and DNS rebinding between check and dial.
		Control: func(_, address string, _ syscall.RawConn) error {
			host, _, err := net.SplitHostPort(address)
			if err != nil {
				return err
			}
			addr, err := netip.ParseAddr(host)
			if err != nil {
				return fmt.Errorf("%w: unresolvable address %s", ErrBlockedAddress, address)
			}
			if isBlocked(addr) {
				return fmt.Errorf("%w: %s", ErrBlockedAddress, addr)
			}
			return nil
		},
	}

	return &Client{
		http: &http.Client{
			Transport: &http.Transport{
				// no Proxy on purpose: HTTP(S)_PROXY would route requests
				// around the dial-time address screen, whose whole value is
				// being the single chokepoint
				DialContext:         dialer.DialContext,
				TLSHandshakeTimeout: 10 * time.Second,
				IdleConnTimeout:     90 * time.Second,
				// no ResponseHeaderTimeout: the per-request context enforces
				// the caller's timeout_seconds, and a fixed cap here would
				// silently narrow that contract
				ForceAttemptHTTP2: true,
			},
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				// via includes the initial request, so the Nth redirect hop
				// sees len(via) == N; > (not >=) lets exactly maxRedirects
				// hops through
				if len(via) > maxRedirects {
					return fmt.Errorf("stopped after %d redirects", maxRedirects)
				}
				return checkScheme(req.URL.Scheme)
			},
		},
	}
}

// Get fetches rawURL, reading at most MaxBytes of the body. A read that fails
// partway through returns what arrived, flagged truncated, rather than
// discarding it.
func (c *Client) Get(ctx context.Context, rawURL string, timeout time.Duration) (*Result, error) {
	target, err := parseURL(rawURL)
	if err != nil {
		return nil, err
	}

	clamped := clampTimeout(timeout)
	ctx, cancel := context.WithTimeout(ctx, clamped)
	defer cancel()

	request, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("User-Agent", userAgent)
	request.Header.Set("Accept", "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.8")
	request.Header.Set("Accept-Language", "en-US,en;q=0.9")

	response, err := c.http.Do(request)
	if err != nil {
		return nil, requestError(err, clamped)
	}
	defer response.Body.Close()

	// read one byte past the cap so a body sitting exactly on it is not
	// reported as truncated; pre-size from Content-Length when sane so the
	// body lands in one allocation (the limit still holds when it lies)
	limit := int64(MaxBytes) + 1
	size := limit
	if n := response.ContentLength; n >= 0 && n < size {
		size = n
	}
	buf := bytes.NewBuffer(make([]byte, 0, size))
	_, readErr := buf.ReadFrom(io.LimitReader(response.Body, limit))
	body := buf.Bytes()
	truncated := len(body) > MaxBytes
	if truncated {
		body = body[:MaxBytes]
	}
	if readErr != nil {
		if len(body) == 0 {
			return nil, requestError(readErr, clamped)
		}
		truncated = true
	}
	if errors.Is(ctx.Err(), context.Canceled) {
		// the caller cancelled: partial content has no audience, and every
		// caller would otherwise need to remember this check itself
		return nil, context.Canceled
	}

	return &Result{
		URL:         response.Request.URL.String(),
		Status:      response.StatusCode,
		ContentType: response.Header.Get("Content-Type"),
		Body:        body,
		Truncated:   truncated,
		ReadErr:     readErr,
	}, nil
}

func parseURL(rawURL string) (string, error) {
	trimmed := strings.TrimSpace(rawURL)
	if trimmed == "" {
		return "", errors.New("url is required")
	}

	parsed, err := url.Parse(trimmed)
	if err != nil {
		return "", fmt.Errorf("invalid url: %w", err)
	}
	if err := checkScheme(parsed.Scheme); err != nil {
		return "", err
	}
	if parsed.Host == "" {
		return "", fmt.Errorf("url has no host: %s", trimmed)
	}
	return parsed.String(), nil
}

// checkScheme relies on url.Parse having lowercased the scheme, which it
// does for every URL reaching here.
func checkScheme(scheme string) error {
	switch scheme {
	case "http", "https":
		return nil
	default:
		return fmt.Errorf("unsupported url scheme %q: only http and https are allowed", scheme)
	}
}

func clampTimeout(timeout time.Duration) time.Duration {
	if timeout <= 0 {
		return DefaultTimeout
	}
	return min(timeout, MaxTimeout)
}

// blockedPrefixes is address space a web fetch has no business reaching from
// a user's machine. IPv4-mapped IPv6 is unmapped before matching, so the v4
// rows cover it.
var blockedPrefixes = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"),       // "this network", incl. the unspecified address
	netip.MustParsePrefix("10.0.0.0/8"),      // private
	netip.MustParsePrefix("100.64.0.0/10"),   // carrier-grade NAT
	netip.MustParsePrefix("127.0.0.0/8"),     // loopback
	netip.MustParsePrefix("169.254.0.0/16"),  // link-local, incl. cloud metadata endpoints
	netip.MustParsePrefix("172.16.0.0/12"),   // private
	netip.MustParsePrefix("192.0.0.0/24"),    // IETF protocol assignments
	netip.MustParsePrefix("192.0.2.0/24"),    // documentation (TEST-NET-1)
	netip.MustParsePrefix("192.168.0.0/16"),  // private
	netip.MustParsePrefix("198.18.0.0/15"),   // benchmarking
	netip.MustParsePrefix("198.51.100.0/24"), // documentation (TEST-NET-2)
	netip.MustParsePrefix("203.0.113.0/24"),  // documentation (TEST-NET-3)
	netip.MustParsePrefix("224.0.0.0/4"),     // multicast
	netip.MustParsePrefix("240.0.0.0/4"),     // reserved, incl. the broadcast address
	netip.MustParsePrefix("::/128"),          // unspecified
	netip.MustParsePrefix("::1/128"),         // loopback
	netip.MustParsePrefix("2001:db8::/32"),   // documentation
	netip.MustParsePrefix("fc00::/7"),        // unique-local
	netip.MustParsePrefix("fe80::/10"),       // link-local
	netip.MustParsePrefix("fec0::/10"),       // site-local (deprecated)
	netip.MustParsePrefix("ff00::/8"),        // multicast
}

// nat64WellKnown synthesizes IPv6 addresses from IPv4 ones on NAT64
// networks; the low 32 bits are the real IPv4 target.
var nat64WellKnown = netip.MustParsePrefix("64:ff9b::/96")

// NAT64 addresses are screened by the IPv4 address they embed, so NAT64-only
// networks keep working while 64:ff9b::7f00:1 cannot smuggle in loopback.
func blocked(addr netip.Addr) bool {
	// Prefix.Contains never matches zoned addresses, so strip the zone to
	// keep fe80::1%eth0 inside the link-local row
	addr = addr.Unmap().WithZone("")

	if nat64WellKnown.Contains(addr) {
		raw := addr.As16()
		return inBlockedPrefix(netip.AddrFrom4([4]byte(raw[12:])))
	}
	return inBlockedPrefix(addr)
}

func inBlockedPrefix(addr netip.Addr) bool {
	for _, prefix := range blockedPrefixes {
		if prefix.Contains(addr) {
			return true
		}
	}
	return false
}

// requestError surfaces the cause so a blocked address or a timeout reads as
// such rather than as a generic transport failure.
func requestError(err error, timeout time.Duration) error {
	switch {
	case errors.Is(err, ErrBlockedAddress):
		// strip the url.Error wrapper noise but keep the dial detail, which
		// names the blocked address
		var wrapped *url.Error
		if errors.As(err, &wrapped) {
			return wrapped.Err
		}
		return err
	case errors.Is(err, context.DeadlineExceeded):
		return fmt.Errorf("request timed out after %s", timeout)
	default:
		return err
	}
}
