# Change these variables as necessary.
binary_name := katsanto-server
# The release tag is the version in Cargo.toml, and nothing else.
app_version := $(shell cargo pkgid -p katsanto-server | sed 's/.*[\#@]//')
branch := $(shell git branch --show-current)

# ==================================================================================== #
# HELPERS
# ==================================================================================== #

## help: print this help message
.PHONY: help
help:
	@echo 'Usage:'
	@sed -n 's/^##//p' ${MAKEFILE_LIST} | column -t -s ':' |  sed -e 's/^/ /'

.PHONY: confirm
confirm:
	@echo -n 'Are you sure? [y/N] ' && read ans && [ $${ans:-N} = y ]

.PHONY: no-dirty
no-dirty:
	@test -z "$(shell git status --porcelain)"


# ==================================================================================== #
# QUALITY CONTROL
# ==================================================================================== #

## audit: run quality control checks
.PHONY: audit
audit: test
	cargo check --workspace --locked
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo audit
	cd web && npm run lint && npm run check

## test: run all tests
.PHONY: test
test:
	cargo test --workspace
	cd web && npm test


# ==================================================================================== #
# DEVELOPMENT
# ==================================================================================== #

## tidy: format the source
.PHONY: tidy
tidy:
	cargo fmt --all
	cd web && npm run format

## tag: tag HEAD with the version in Cargo.toml
.PHONY: tag
tag: no-dirty
	@if git rev-parse -q --verify refs/tags/${app_version} >/dev/null; then \
		test "$$(git rev-list -n1 ${app_version})" = "$$(git rev-parse HEAD)" || \
			{ echo "Tag ${app_version} is at another commit; bump the version in Cargo.toml"; exit 1; }; \
	else \
		git tag ${app_version} && echo "Tagged ${app_version}"; \
	fi

## atomic-push: push the branch together with the tag of the Cargo.toml version
.PHONY: atomic-push
atomic-push: tag
	git push --atomic origin ${branch} ${app_version}

## build: build the application to target/release/${binary_name}
.PHONY: build
build:
	cd web && npm ci && npm run build
	cargo build --release --locked -p katsanto-server

## run: run the application bound to 0.0.0.0:13000; pass its arguments as ARGS="openapi"
.PHONY: run
run: build
	KATSANTO_BIND=0.0.0.0:13000 \
	KATSANTO_PUBLIC_URL=http://$$(hostname -I | cut -d' ' -f1):13000 \
	target/release/${binary_name} ${ARGS}


# ==================================================================================== #
# OPERATIONS
# ==================================================================================== #

## push: push changes to the remote Git repository
.PHONY: push
push: confirm audit no-dirty
	git push

## production/deploy: deploy the application to production
.PHONY: production/deploy
production/deploy: confirm audit no-dirty build
	# Include additional deployment steps here...
	@echo Done


