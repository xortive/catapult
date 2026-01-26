# Contributing

Thank you for your interest in contributing to Catapult!

## Getting Started

1. Fork the repository
2. Clone your fork
3. Create a feature branch
4. Make your changes
5. Submit a pull request

## Development Setup

```bash
# Clone your fork
git clone https://github.com/YOUR-USERNAME/catapult.git
cd catapult

# Add upstream remote
git remote add upstream https://github.com/your-org/catapult.git

# Create a branch
git checkout -b feature/my-feature
```

## Code Style

We follow standard Rust conventions:

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Write tests for new functionality

```bash
# Format code
cargo fmt --all

# Run linter
cargo clippy --workspace -- -D warnings

# Run tests
cargo test --workspace
```

## Commit Messages

Follow conventional commit format:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `refactor`: Code refactoring
- `test`: Adding tests
- `chore`: Maintenance

Examples:
```
feat(mux): add lockout timer for switch debouncing
fix(protocol): handle CI-V frames with missing end byte
docs: update switching modes documentation
```

## Pull Request Process

1. Update documentation for user-facing changes
2. Add tests for new functionality
3. Ensure CI passes (tests, formatting, clippy)
4. Request review from maintainers

## Adding Protocol Support

To add a new radio protocol:

1. Create a new module in `cat-protocol/src/`
2. Implement `parse()` and `encode()` functions
3. Map to/from `RadioCommand` enum
4. Add to the `Protocol` enum
5. Write tests with real command examples

For GUI integration, the relevant files in `cat-desktop` are:
- `app/radio.rs` - Radio connection and state handling
- `app/amplifier.rs` - Amplifier integration
- `traffic_monitor/ingest.rs` - Protocol-specific traffic parsing

## Reporting Issues

When reporting bugs, include:

- Catapult version
- Operating system
- Radio model(s)
- Steps to reproduce
- Expected vs actual behavior
- Log output (if available)

## Feature Requests

Open an issue with:

- Clear description of the feature
- Use case / motivation
- Proposed implementation (if any)

## License

By contributing, you agree that your contributions will be licensed under the same license as the project.
