# Catapult Documentation

This directory contains the documentation for Catapult, built with [mdBook](https://rust-lang.github.io/mdBook/).

## Local Development

### Prerequisites

Install mdBook:

```bash
cargo install mdbook
```

### Build and Serve

```bash
# From the docs directory
cd docs

# Build the site
mdbook build

# Or serve with live reload
mdbook serve --open
```

The site will be available at `http://localhost:3000`.

## Deployment

### Option 1: Cloudflare Pages (Recommended)

The easiest way to deploy:

1. Go to [Cloudflare Dashboard](https://dash.cloudflare.com/) → Pages
2. Click "Create a project" → "Connect to Git"
3. Select your repository
4. Configure build settings:
   - **Build command:** `cargo install mdbook && cd docs && mdbook build`
   - **Build output directory:** `docs/dist`
5. Deploy!

Cloudflare Pages will automatically rebuild on every push.

### Option 2: Cloudflare Workers

For more control, use Workers with static assets:

```bash
# Install wrangler
npm install -g wrangler

# Login to Cloudflare
wrangler login

# Build the docs
cd docs
mdbook build

# Deploy
wrangler deploy
```

### Option 3: Manual Deployment

Build locally and upload:

```bash
cd docs
mdbook build
# Upload contents of ./dist to any static host
```

## Project Structure

```
docs/
├── book.toml           # mdBook configuration
├── wrangler.toml       # Cloudflare Workers config
├── worker.js           # Worker script for static serving
├── src/
│   ├── SUMMARY.md      # Table of contents
│   ├── introduction.md # Home page
│   ├── simulation/     # Simulation docs
│   ├── protocols/      # Protocol docs
│   ├── architecture/   # Architecture docs
│   └── development/    # Development docs
└── dist/               # Built output (gitignored)
```

## Writing Documentation

- All pages are Markdown files in `src/`
- Add new pages to `SUMMARY.md` to include them in the navigation
- Use relative links between pages: `[link](./other-page.md)`
- Code blocks support syntax highlighting: ` ```rust `

## Configuration

Edit `book.toml` to customize:
- Title and authors
- Theme settings
- Git repository link
- Search behavior
