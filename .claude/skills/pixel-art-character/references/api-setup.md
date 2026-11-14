# Image API setup

`generate_image` calls an image model from the Gateway (MCP server / editors). It supports:

| provider | endpoint | auth |
| --- | --- | --- |
| `openai` (and OpenAI-compatible servers) | `POST {base}/v1/images/generations` | `Authorization: Bearer <key>` |
| `gemini` | `POST {base}/v1beta/models/{model}:generateContent` | `x-goog-api-key: <key>` |

## Configure

Either environment variables (they win over the file):

```
PIXEL_STUDIO_IMAGE_PROVIDER=openai|gemini
PIXEL_STUDIO_IMAGE_MODEL=<model id from the provider's docs>
PIXEL_STUDIO_IMAGE_KEY=<api key>
PIXEL_STUDIO_IMAGE_URL=<optional base URL, e.g. a proxy or compatible server>
PIXEL_STUDIO_IMAGE_SIZE=1024x1024
```

or the editors: Windows *File → AI image model → Image API settings…*, Android overflow menu
*Image API settings*. They write `image_api.json` in the workspace with owner-only permissions.

Model ids change often (several were retired in 2026); look up the current image model on the
provider's documentation page instead of guessing. `image_api_status` shows provider, model and
whether a key is set — never the key itself.

## Rules for the agent

- Never print, log or repeat an API key. Don't ask the user to paste a key into the chat; point
  them to the settings dialog or the environment variables.
- Each `generate_image` call costs money and needs the user's approval; the gateway asks for it
  (the tool is marked destructive). Batch requests: one turnaround sheet instead of eight single
  views.
- Raw generated images are saved to the workspace's Exports folder, so a re-import doesn't need a
  new paid call.
- On errors: 401/403 → key or model access; 400 → model id or size not supported; timeouts →
  try again later or a smaller size.
