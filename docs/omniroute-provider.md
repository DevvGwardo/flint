# OmniRoute provider

Hooter's native agent can use OmniRoute's OpenAI-compatible Chat Completions
API, including streaming and tools. External ACP agents keep their own provider
configuration.

1. Run OmniRoute separately (`omniroute`). Its default API URL is
   `http://127.0.0.1:20128/v1`.
2. Open Hooter **Settings → Provider → OmniRoute**.
3. Keep **Model** as `auto`, or enter an exact provider/model ID or combo name
   advertised by your router. Subagents inherit the selected parent model unless
   you supply a separate exact model.
4. For authenticated routing, set `OMNIROUTE_API_KEY` in Hooter's environment
   or select a private API-key file. Use an OmniRoute-issued key, not an
   upstream provider credential. The preset clears the draft's old key-file
   selection and never falls back to `FLINT_API_KEY`, `OPENAI_API_KEY`, or a
   legacy provider key.
5. If your local router has API-key enforcement disabled, explicitly enable
   **Local no-auth (loopback only)** instead. Hooter sends no Authorization
   header in this mode. It does not change OmniRoute's authentication settings,
   and rejects remote URLs in no-auth mode.
6. Use **Test connection (GET /models)**, then **Save**. Start a new native
   session; already running sessions keep their captured provider configuration.

Custom ports, reverse-proxy base paths, and remote authenticated OmniRoute
servers can use the editable **Base URL**. A successful model listing does not
prove that credentials, inference, tools, or the selected upstream route work.
Model listings may contain models that are not currently connected or healthy.

The preset and connection check do not launch OmniRoute, connect accounts,
change routing policy, copy credentials, or send a completion request.
