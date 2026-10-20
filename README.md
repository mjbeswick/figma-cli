# figma-design-inspector

A small Rust CLI that gives coding agents a bounded, structured view of Figma
design files. It reads a local Figma JSON export or fetches a file through the
official Figma REST API, then summarizes layers and inspectable properties as
JSON (the default) or readable text.

## Install

```sh
cargo install --path .
```

The installed binary is `figma`.

## Authentication

Remote inspection requires a Figma access token with the `file_content:read`
scope. Supply it through the environment; the CLI never accepts or prints the
token as a command-line argument:

```sh
export FIGMA_API_KEY='…'
```

`FIGMA_ACCESS_TOKEN` is also accepted and takes precedence if both variables
are set. Neither variable is stored or printed by the CLI.

Personal access tokens, OAuth access tokens, and plan access tokens are all
supported by the API. For Figma for Government, set `FIGMA_API_BASE_URL` to
`https://api.figma-gov.com`. The default is `https://api.figma.com`.

## Usage

```sh
# Inspect a Figma file URL (JSON is the agent-friendly default)
figma inspect 'https://www.figma.com/design/FILE_KEY/Checkout?node-id=1%3A2'
figma inspect 'https://www.figma.com/proto/FILE_KEY/Checkout?node-id=1-2'

# Limit the API response to a node and return a shallow layer inventory
figma inspect 'https://www.figma.com/design/FILE_KEY/Checkout' \
  --node-id '1:2' --depth 2

# Search layer names within the selected traversal depth
figma inspect FILE_KEY --query button --max-nodes 100

# Inspect the included Figma-shaped JSON fixture without network access
figma inspect ./examples/figma-file.json --query 'pay now'

# Print agent instructions (Markdown by default; use --format json for integrations)
figma skills
figma skills --format json
```

`figma skills` returns the bundled `figma-design-inspection` agent skill,
including a safe inspection workflow, output field guide, and authentication
boundaries. Install the crate to keep that guidance version-matched with the
CLI; agents may also read `skills/figma-design-inspection/SKILL.md` from source.

Use `--node-id` more than once or provide a comma-separated list. The API
supports `ids` and `depth` on the file endpoint; design, prototype, and standard
file URLs resolve to the file key. The CLI also applies exact
node-ID and case-insensitive name filters to the returned tree. Node summaries
include IDs, names, types, common inspectable properties, and child counts when
those properties are present. The `properties` object preserves known layout,
typography, appearance, visibility, transform, and component fields.
`--max-nodes`
limits result size and `truncated` indicates when the depth or node limit
stopped traversal.

## Scope and limitations

This is a read-only inspection tool, not an unofficial desktop protocol
client. It does not attach to or control the Figma desktop app, edit designs,
render screenshots, or bypass file permissions. Remote access follows the
Figma REST API's token scopes, sharing permissions, and rate limits. Local
JSON mode accepts an API-shaped response (or a node tree) and does not validate
that the data originated from Figma.

The companion REA investigation observed Figma Desktop 126.10.10 and its
declared `.fig`, `.jam`, and `.deck` document types. REA could not inventory the
packaged ASAR (malformed artifact result), and native analysis was unavailable
because no Ghidra/Hopper/IDA/Kuna provider was configured. Therefore this CLI
uses the documented official REST API rather than claiming reverse-engineered
desktop internals.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
