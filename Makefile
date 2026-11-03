PREFIX ?= $(HOME)/.local
XDG_CONFIG_HOME ?= $(HOME)/.config
XDG_SYSTEMD_USER_DIR ?= $(HOME)/.config/systemd/user
UNITS := reclaim.service reclaim.timer reclaim-low-disk.service reclaim-low-disk.timer reclaim-scan.service
TIMERS := reclaim.timer reclaim-low-disk.timer

.PHONY: build test install install-widget enable uninstall

build:
	cargo build --release --locked

test:
	cargo fmt --check
	cargo test --locked
	cargo clippy --all-targets --locked -- -D warnings

install: build
	install -Dm755 target/release/reclaim "$(PREFIX)/bin/reclaim"
	for unit in $(UNITS); do install -Dm644 "systemd/$$unit" "$(XDG_SYSTEMD_USER_DIR)/$$unit"; done
	mkdir -p "$(XDG_CONFIG_HOME)/reclaim"
	test -e "$(XDG_CONFIG_HOME)/reclaim/config.toml" || \
		install -m644 config.example.toml "$(XDG_CONFIG_HOME)/reclaim/config.toml"
	systemctl --user daemon-reload
	@echo "Installed. Check 'reclaim status', then run: make enable"

enable:
	systemctl --user enable --now $(TIMERS)

uninstall:
	systemctl --user disable --now $(TIMERS) || true
	for unit in $(UNITS); do rm -f "$(XDG_SYSTEMD_USER_DIR)/$$unit"; done
	rm -f "$(PREFIX)/bin/reclaim"
	systemctl --user daemon-reload
	@echo "The config in $(XDG_CONFIG_HOME)/reclaim was left in place."

WIDGET_DIR := $(XDG_CONFIG_HOME)/omarchy/plugins/fl.reclaim

install-widget:
	omarchy plugin validate omarchy/fl.reclaim
	mkdir -p "$(WIDGET_DIR)"
	install -m644 omarchy/fl.reclaim/* "$(WIDGET_DIR)/"
	# The shell's plugin hot reload can keep old compiled QML and scripts, so restart it.
	omarchy restart shell
	@echo "Installed. If it is not in the bar yet: omarchy plugin enable fl.reclaim"
