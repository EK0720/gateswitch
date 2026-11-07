//! Image generation guidance for Codex: a marked block in ~/.codex/AGENTS.md
//! that tells Codex how to create images through a gateway's
//! `/images/generations` endpoint. Ported from the Python app (0.3.6).
//!
//! Nothing gateway-specific is written into AGENTS.md. At run time the helper
//! follows whatever provider Codex is configured to use (`model_provider` in
//! config.toml → its `base_url` and `env_key`), and looks the image model up
//! for that base URL in `model-switcher-images.json` next to config.toml.
//! Switching Codex to another gateway switches image generation with it.
//! No key is stored anywhere: the helper reads the provider's env var.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const START: &str = "<!-- model-switcher:image-generation:start -->";
pub const END: &str = "<!-- model-switcher:image-generation:end -->";
/// Present in blocks before 0.2, which pinned one gateway URL and model.
const OLD_PINNED: &str = "Configuration (contains no secret): `";

/// AGENTS.override.md wins over AGENTS.md in Codex when it has content.
pub fn agents_path(home: &Path) -> PathBuf {
    let over = home.join("AGENTS.override.md");
    match std::fs::read_to_string(&over) {
        Ok(t) if !t.trim().is_empty() => over,
        _ => home.join("AGENTS.md"),
    }
}

/// Image model per gateway base URL.
pub fn models_path(home: &Path) -> PathBuf {
    home.join("model-switcher-images.json")
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Default)]
pub struct Installed {
    pub path: String,
    pub installed: bool,
    /// Old block that pins one gateway; setting up again replaces it.
    pub outdated: bool,
    /// Gateway base URL → image model.
    pub models: BTreeMap<String, String>,
}

pub fn read_models(text: &str) -> BTreeMap<String, String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("models").and_then(|m| m.as_object()).cloned())
        .map(|m| m.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect())
        .unwrap_or_default()
}

pub fn write_models(models: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = serde_json::to_vec_pretty(&serde_json::json!({ "v": 1, "models": models })).unwrap_or_default();
    out.push(b'\n');
    out
}

pub fn status(home: &Path) -> Installed {
    let path = agents_path(home);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let block = match (text.find(START), text.find(END)) {
        (Some(a), Some(b)) if a < b => Some(&text[a..b]),
        _ => None,
    };
    Installed {
        path: path.display().to_string(),
        installed: block.is_some(),
        outdated: block.map(|b| b.contains(OLD_PINNED) || b.contains("in Model Switcher")).unwrap_or(false),
        models: read_models(&std::fs::read_to_string(models_path(home)).unwrap_or_default()),
    }
}

fn check_markers(text: &str) -> Result<(), String> {
    let (s, e) = (text.matches(START).count(), text.matches(END).count());
    if s != e || s > 1 {
        return Err("The image guidance block in AGENTS.md is broken or duplicated. Fix the file first.".into());
    }
    if let (Some(a), Some(b)) = (text.find(START), text.find(END)) {
        if b < a {
            return Err("The image guidance markers in AGENTS.md are out of order. Fix the file first.".into());
        }
    }
    Ok(())
}

pub fn valid_model(model: &str) -> Result<&str, String> {
    let m = model.trim();
    if m.is_empty() || m.len() > 200 || m.chars().any(|c| c.is_whitespace() || c.is_control() || "`<>\"\\".contains(c)) {
        return Err("Choose an image model".into());
    }
    Ok(m)
}

/// AGENTS.md text with the block added or replaced (same text if current).
pub fn install(before: &str) -> Result<String, String> {
    check_markers(before)?;
    let block = block();
    Ok(match (before.find(START), before.find(END)) {
        (Some(a), Some(b)) => format!("{}{}{}", &before[..a], block, &before[b + END.len()..]),
        _ if before.is_empty() => format!("{block}\n"),
        _ => format!("{}{}{}\n", before, if before.ends_with("\n\n") { "" } else if before.ends_with('\n') { "\n" } else { "\n\n" }, block),
    })
}

/// AGENTS.md text without the block; None if there was none.
pub fn remove(before: &str) -> Result<Option<String>, String> {
    check_markers(before)?;
    let (Some(a), Some(b)) = (before.find(START), before.find(END)) else { return Ok(None) };
    let head = before[..a].trim_end_matches(['\n', '\r']);
    let tail = before[b + END.len()..].trim_start_matches(['\n', '\r']);
    Ok(Some(match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => format!("{head}\n"),
        (false, false) => format!("{head}\n\n{tail}"),
    }))
}

fn block() -> String {
    format!("{START}\n{BODY}\n{END}")
}

const BODY: &str = r#"## Image generation via the configured gateway

When the user asks to CREATE a raster image, use this gateway workflow by default.
This preference applies only to image generation. Do not change the coding model or provider.
Do not generate images merely because a chat starts. Explicit user choices take precedence.
Use the imagegen skill for prompt composition and visual QA when available; the user has
chosen this custom API workflow instead of the built-in image generation service.

The gateway is not fixed here: the helper below uses the provider Codex is currently
configured with (`model_provider` in config.toml, its `base_url` and `env_key`) and the
image model chosen for that gateway in Gateswitch (`model-switcher-images.json` next
to config.toml). If it reports that no image model is set, tell the user to choose one in
Gateswitch (Codex > Image generation); do not guess a model name.

Transparent background default: default to PNG with a real transparent alpha channel for
generated images, especially character art, costume variants, sprites, avatars, and separate
Spine layers. An explicit request for a background, scene, or opaque image takes precedence.
Include in the prompt: "Output a PNG with true transparent background (RGBA alpha). No painted
checkerboard, solid-color backdrop, background scenery, or baked background shadow. Preserve
fine hair strands, clean antialiased edges, and the complete requested silhouette." When
supported by the selected gateway/endpoint, explicitly request `background: "transparent"`
and PNG output in the API payload. Do not assume the custom gateway supports these parameters
merely because another provider does. Preserve the configured gateway and its
error/no-auto-retry policy. Verify the returned file has actual transparent pixels outside
the subject, not just an alpha channel that is fully opaque or a painted transparency grid.
Inspect edges on black, white, green, and yellow backgrounds at normal size and enlarged;
check for matte halos, jagged edges, missing strands, and stray pixels. Prefer native
generated transparency over chroma-keying or automatic background removal. Do not silently
replace requested transparency with a green/black background, cut away hair, or claim a
checkerboard image is transparent. If the endpoint returns an opaque image, report that
limitation and preserve the original output. For Spine layers, keep the approved reference
identity, scale, alignment, and attachment landmarks; request complete hidden portions
needed for motion and compare the assembled layers against the approved concept before
accepting them.

The helper reads the API key at execution time from the provider's `env_key` variable (on
Windows the USER variable in HKCU\Environment first, then the process environment). Never
print the key, put it in a prompt, or save it in a script, Markdown, project, or command line.

Prepare a suitable image prompt. Save the following standard-library Python helper in
the current task's work directory, find an available Python 3 interpreter, and run it
with a UTF-8 prompt file path and a NEW absolute PNG output path as its arguments. Add a
third argument `opaque` only when the user explicitly asked for a background, scene, or
opaque image; otherwise the helper requests `background: "transparent"` and PNG output.
If the gateway rejects those parameters (HTTP 4xx), report the error to the user; do not
silently retry without them. The helper prints whether the PNG has an alpha channel; that
is not proof of transparency, so still inspect pixels and edges as described above.
Use the task's designated output directory (otherwise outputs/). No pip packages needed.
Only the image prompt is sent, not unrelated chat history or project files.

```python
import base64
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

home = Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")
text = (home / "config.toml").read_text(encoding="utf-8-sig")
try:
    import tomllib
    config = tomllib.loads(text)
except ImportError:  # Python < 3.11: read only the keys needed here
    config, target = {"model_providers": {}}, None
    target = config
    for raw in text.splitlines():
        line = raw.strip()
        head = re.match(r'^\[model_providers\.(?:"([^"]+)"|([A-Za-z0-9_-]+))\]', line)
        if head:
            target = config["model_providers"].setdefault(head.group(1) or head.group(2), {})
        elif line.startswith("["):
            target = {}
        else:
            pair = re.match(r'^([A-Za-z_]+)\s*=\s*"([^"]*)"', line)
            if pair:
                target[pair.group(1)] = pair.group(2)
provider = config.get("model_providers", {}).get(config.get("model_provider", ""), {})
base_url = str(provider.get("base_url", "")).rstrip("/")
env_key = str(provider.get("env_key", ""))
if not base_url or not env_key:
    raise SystemExit("Codex is not set up with a gateway provider; set one up in Gateswitch.")
models = {}
for folder in (home, Path.home() / ".codex"):
    try:
        models = json.loads((folder / "model-switcher-images.json").read_text(encoding="utf-8")).get("models", {})
        break
    except (OSError, ValueError):
        pass
model = models.get(base_url)
if not model:
    raise SystemExit("No image model set for " + base_url
                     + "; choose one in Gateswitch (Codex > Image generation).")
key = None
if sys.platform == "win32":
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as registry:
            key = str(winreg.QueryValueEx(registry, env_key)[0])
    except FileNotFoundError:
        pass
if not key:
    key = os.environ.get(env_key, "")
if not key:
    raise SystemExit("Missing gateway key; configure it in Gateswitch.")
prompt_path, output_path = map(Path, sys.argv[1:3])
transparent = (sys.argv[3:4] or ["transparent"])[0].lower() != "opaque"
if output_path.exists():
    raise SystemExit("Output exists; choose a new filename.")
payload = {"model": model, "prompt": prompt_path.read_text(encoding="utf-8-sig"),
           "n": 1, "size": "1536x1024"}
if transparent:
    payload.update(background="transparent", output_format="png")
request = urllib.request.Request(
    base_url + "/images/generations",
    data=json.dumps(payload).encode("utf-8"),
    headers={"Authorization": "Bearer " + key, "Content-Type": "application/json",
             "Accept": "application/json", "User-Agent": "Gateswitch-Image/1"})
print("gateway: " + base_url + "  model: " + model)
try:
    with urllib.request.urlopen(request, timeout=180) as response:
        result = json.load(response)
except urllib.error.HTTPError as error:
    detail = error.read(500).decode("utf-8", "replace").replace(key, "***")
    raise SystemExit("Image gateway HTTP " + str(error.code) + ": " + detail
                     + " Do not automatically retry.") from None
except (urllib.error.URLError, TimeoutError):
    raise SystemExit("Image gateway connection failed; do not automatically retry.") from None
encoded = result.get("data", [{}])[0].get("b64_json")
if not encoded:
    raise SystemExit("No inline image returned; do not automatically retry.")
image = base64.b64decode(encoded, validate=True)
if not image.startswith(b"\x89PNG\r\n\x1a\n"):
    raise SystemExit("Unexpected image format; inspect before saving.")
output_path.parent.mkdir(parents=True, exist_ok=True)
with output_path.open("xb") as output:
    output.write(image)
print(str(output_path.resolve()))
color_type = image[25] if len(image) > 25 else -1
has_alpha = color_type in (4, 6) or b"tRNS" in image[:image.find(b"IDAT")]
print("alpha channel: " + ("yes" if has_alpha else "NO"))
if transparent and not has_alpha:
    print("WARNING: transparency was requested but the gateway returned an opaque PNG; "
          "report this limitation and keep the original output.")
```

Inspect the saved image and display it inline with its absolute path. Link the original
file and preserve the prompt. Report errors honestly; do not silently change provider or
retry a timed-out paid request. The gateway may route the requested model ID to a separate
image backend; do not claim which underlying model produced the image. A false imageOutput
flag in /models alone does not prove this endpoint is unsupported."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_keeps_user_text_and_is_gateway_free() {
        let user = "# My rules\nBe brief.\n";
        let a = install(user).unwrap();
        assert!(a.starts_with(user));
        assert_eq!(a.matches(START).count(), 1);
        assert!(!a.contains("https://"), "no gateway URL in AGENTS.md");
        assert_eq!(install(&a).unwrap(), a, "idempotent");
        assert_eq!(remove(&a).unwrap().unwrap(), user);
    }

    #[test]
    fn old_pinned_block_is_replaced() {
        let old = format!("x\n\n{START}\nConfiguration (contains no secret): `{{\"base_url\":\"https://g/v1\"}}`\n{END}\n");
        let dir = std::env::temp_dir().join(format!("ms-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), &old).unwrap();
        assert!(status(&dir).outdated);
        let new = install(&old).unwrap();
        assert!(!new.contains("https://g/v1") && new.starts_with("x\n\n"));
        std::fs::write(dir.join("AGENTS.md"), &new).unwrap();
        let s = status(&dir);
        assert!(s.installed && !s.outdated);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn models_file_round_trip() {
        let mut m = BTreeMap::new();
        m.insert("https://a/v1".to_string(), "img-a".to_string());
        assert_eq!(read_models(&String::from_utf8(write_models(&m)).unwrap()), m);
        assert!(read_models("nope").is_empty());
    }

    #[test]
    fn rejects_broken_markers_and_bad_model() {
        assert!(install(&format!("{START}\n{START}\n{END}")).is_err());
        assert!(valid_model("a b").is_err());
        assert!(valid_model("").is_err());
        assert_eq!(valid_model(" cx/gpt-image ").unwrap(), "cx/gpt-image");
        assert_eq!(remove("plain").unwrap(), None);
    }
}
