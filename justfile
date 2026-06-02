_default:
    @just --list

# Install / sync dependencies
setup:
    cd apps/pum && uv sync

# Verify adapters on this host
doctor:
    python3 apps/pum/pum.py doctor

# Detect and inventory installed packages
scan:
    python3 apps/pum/pum.py scan

# Check for outdated packages
check:
    python3 apps/pum/pum.py check

# Print inventory report
report *FLAGS:
    python3 apps/pum/pum.py report {{FLAGS}}

# Run updates (--dry-run to preview)
update *FLAGS:
    python3 apps/pum/pum.py update {{FLAGS}}

# Self-update managers (--apply to run)
self *FLAGS:
    python3 apps/pum/pum.py self {{FLAGS}}

# Run unit tests
test:
    python3 -m unittest discover -s apps/pum/tests -p "test_*.py" -v

# Lint with ruff
lint:
    ruff check apps/pum/pum.py apps/pum/tests/

# Format with ruff
fmt:
    ruff format apps/pum/pum.py apps/pum/tests/

# Type + lint gate
ci: test lint
