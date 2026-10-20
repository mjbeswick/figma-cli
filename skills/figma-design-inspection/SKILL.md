---
name: figma-design-inspection
description: Inspect accessible Figma files with the figma CLI to understand page and layer structure, component/layout properties, and find specific design elements.
---

# Figma design inspection

Use `figma inspect` to read Figma design structure as bounded JSON, suitable
for analysis and code implementation. This is read-only.

## Workflow

1. Confirm the design link or local API-shaped JSON export supplied by the
   user. Do not guess a file or search the account for unrelated designs.
2. For a remote file, verify that `FIGMA_API_KEY` or `FIGMA_ACCESS_TOKEN` is configured in the
   process environment. Never ask the user to paste a token in chat or include
   a token in a command argument. The token must have `file_content:read` and
   access to the requested file.
3. Start with a shallow, bounded inventory:

   ```sh
   figma inspect '<FIGMA_FILE_URL>' --depth 2 --max-nodes 250
   ```

4. Use a supplied node ID to narrow the request, or search the returned tree
   by name when locating a layer:

   ```sh
   figma inspect '<FIGMA_FILE_URL>' --node-id '1:2' --depth 3
   figma inspect '<FIGMA_FILE_URL>' --query 'Primary action' --depth 4
   ```

5. Increase depth or the node budget only as needed. Check `truncated` and
   `filters` in JSON before treating the result as a complete inventory.
6. Cite actual node names, IDs, and returned properties when describing the
   design. Distinguish observed API/export fields from implementation
   recommendations. A missing field or node in a bounded result is not proof
   it does not exist elsewhere in the file.

## Output

JSON is the default and the recommended agent format. Each response contains
file metadata, the applied filters, `returned_nodes`, a `truncated` flag, and
node summaries. Each node summary includes its ID, name, type, depth, child
count, and a `properties` object containing available common layout,
typography, appearance, visibility, transform, and component fields.

`--format text` is intended for quick human review. `--node-id` may be repeated
or given as a comma-separated list. `--query` is a case-insensitive substring
match on names.

## Authentication and boundaries

- Remote requests use the official Figma REST API and `X-Figma-Token` header.
- `FIGMA_API_KEY` (or `FIGMA_ACCESS_TOKEN`, which takes precedence) is read from the environment and is never saved or
  printed by this CLI. Never expose it in logs, prompts, or agent output.
- The configured API host must be Figma's HTTPS API host (including the
  Figma for Government API host).
- Local JSON inspection is offline; its origin and completeness are not
  verified by the CLI.
- The CLI does not control the Figma desktop app, edit files, render images,
  bypass access controls, or infer undocumented desktop behavior.
- API permissions, availability, rate limits, response depth and selected
  result limits constrain what can be inspected.
