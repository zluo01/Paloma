# Go compiles fully optimized by default; release differs from build only in
# packaging: no cgo (static binary, runs anywhere), no debug info or symbol
# table (-s -w), no local filesystem paths embedded (-trimpath).

.PHONY: build release test lint proto

BUF_VERSION := v1.72.0

build:
	go build -o webfetch .

release:
	CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o webfetch .

test:
	go test -race ./...

lint:
	@files="$$(gofmt -l .)"; \
	if [ -n "$$files" ]; then \
		echo "The following files need gofmt:"; \
		echo "$$files"; \
		exit 1; \
	fi
	go vet ./...

# Generated code is committed; run this only after changing the schema.
# protoc-gen-go is installed without @version so it resolves to go.mod's
# protobuf version, keeping the generator and runtime in lockstep. Buf is
# pinned so regeneration is reproducible locally and in CI.
proto:
	GOBIN=$(CURDIR)/.tools go install google.golang.org/protobuf/cmd/protoc-gen-go
	GOBIN=$(CURDIR)/.tools go install github.com/bufbuild/buf/cmd/buf@$(BUF_VERSION)
	PATH=$(CURDIR)/.tools:$$PATH $(CURDIR)/.tools/buf generate
