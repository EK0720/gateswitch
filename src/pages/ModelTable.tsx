import { useEffect, useMemo, useState } from "react";
import { api, Caps, errText, Probe, ProbeStatus, Provider } from "../api";
import { CapPills, MAX_CAP_MODELS } from "./CapabilityTest";
import { Icon } from "../icons";
import { locale, t, tr } from "../i18n";

/** Rough cost of one probe: a "ping" prompt plus up to 16 output tokens. */
export const TOKENS_PER_PROBE = 20;

// Vietnamese keys; t() runs at call time so a language switch applies.
const LABEL: Record<ProbeStatus, string> = {
  ok: "Chạy", quota: "Hết quota", auth: "Key bị từ chối", missing: "Gateway không có model",
  upstream: "Upstream lỗi", timeout: "Quá 30 giây", error: "Lỗi",
};
const DOT: Record<ProbeStatus, string> = { ok: "ok", quota: "warn", auth: "bad", missing: "bad", upstream: "bad", timeout: "warn", error: "bad" };

export const groupOf = (p: Provider, m: string) => p.info?.[m]?.owned_by ?? t("other");

/** Readable reason from a raw gateway error: the JSON "detail"/"message" if there is one. */
export function shortReason(msg: string) {
  const m = msg.match(/"(?:detail|message)"\s*:\s*"((?:[^"\\]|\\.)*)"/);
  return m ? m[1].replace(/\\"/g, '"') : msg;
}

export function statusText(r: Probe) {
  return r.status === "ok" && r.latency_ms != null ? t("Works, {ms} ms", { ms: r.latency_ms }) : t(LABEL[r.status]);
}

type Scope = { kind: "all" } | { kind: "current" } | { kind: "group"; group: string };

export function ModelTable({ provider, notify, onChange }: { provider: Provider; notify: (t: string) => void; onChange: () => void }) {
  const [inUse, setInUse] = useState<string[]>([]);
  const [asking, setAsking] = useState(false);
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  // Capability test: tick models in the table, then test them together.
  const [picked, setPicked] = useState<string[]>([]);
  const [caps, setCaps] = useState<Record<string, Caps>>({});
  const [capBusy, setCapBusy] = useState(false);

  useEffect(() => { setPicked([]); setCaps({}); }, [provider.id]);

  const togglePick = (m: string) =>
    setPicked((p) => (p.includes(m) ? p.filter((x) => x !== m) : p.length >= MAX_CAP_MODELS ? p : [...p, m]));

  async function testCaps() {
    if (!confirm(t("Send about 6 real requests per model ({n} models, a few thousand tokens each because of the cache test). Continue?", { n: picked.length }))) return;
    setCapBusy(true); setError("");
    try {
      const rs = await api.capabilityTest(provider.id, picked);
      setCaps((c) => ({ ...c, ...Object.fromEntries(rs) }));
      setPicked([]);
    } catch (e) {
      setError(errText(e));
    } finally {
      setCapBusy(false);
    }
  }

  useEffect(() => {
    api.codexCurrent()
      .then((c) => setInUse(c.provider_id === provider.id && c.model ? [c.model] : []))
      .catch(() => setInUse([]));
  }, [provider.id]);

  const groups = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const m of provider.models) {
      const g = groupOf(provider, m);
      map.set(g, [...(map.get(g) ?? []), m]);
    }
    return [...map.entries()];
  }, [provider]);
  const grouped = groups.length > 1;

  const targets =
    scope.kind === "all" ? provider.models
    : scope.kind === "current" ? inUse.filter((m) => provider.models.includes(m))
    : provider.models.filter((m) => groupOf(provider, m) === scope.group);

  const [trying, setTrying] = useState("");
  async function tryOne(m: string) {
    setTrying(m); setError("");
    try {
      const p = await api.probeOne(provider.id, m);
      notify(statusText(p.probes[m] ?? { status: "error" } as Probe));
      onChange();
    } catch (e) {
      setError(errText(e));
    } finally {
      setTrying("");
    }
  }

  async function run() {
    setRunning(true); setError("");
    try {
      const p = await api.probeModels(provider.id, scope.kind === "all" ? null : targets);
      const rs = targets.map((m) => p.probes[m]).filter(Boolean);
      notify(t("{ok}/{n} models work", { ok: rs.filter((r) => r.status === "ok").length, n: rs.length }));
      setAsking(false);
      onChange();
    } catch (e) {
      setError(errText(e));
    } finally {
      setRunning(false);
    }
  }

  const lastRun = Object.values(provider.probes ?? {}).map((r) => r.checked_at).sort().pop();
  const scopeValue = scope.kind === "group" ? "group:" + scope.group : scope.kind;

  return (
    <>
      <div className="probe-bar">
        {picked.length > 0 ? (
          <>
            <span><b>{t("{n} selected", { n: picked.length })}</b> <span className="muted">· {t("up to {max}", { max: MAX_CAP_MODELS })}</span></span>
            <button className="btn small ghost" disabled={capBusy} onClick={() => setPicked([])}>{t("Clear")}</button>
            <button className="btn small primary" style={{ marginLeft: 0 }} disabled={capBusy || !provider.has_key} onClick={testCaps}
              title={t("Stream, tools, cache, images: about 6 requests per model")}>
              {capBusy ? t("Testing…") : t("Test capabilities")}
            </button>
          </>
        ) : (
          <span className="muted">
            {lastRun ? t("Last tested {time}", { time: new Date(lastRun).toLocaleString(locale()) }) : t("No models tested yet")}
          </span>
        )}
        {!asking && !picked.length && (
          <button className="btn small" disabled={!provider.has_key} onClick={() => { setAsking(true); setError(""); }}>
            <Icon name="check" />{t("Test models")}
          </button>
        )}
      </div>

      {asking && (
        <div className="probe-ask">
          <select className="select" value={scopeValue} disabled={running} aria-label={t("Choose models to test")}
            onChange={(e) => {
              const v = e.target.value;
              setScope(v === "all" ? { kind: "all" } : v === "current" ? { kind: "current" } : { kind: "group", group: v.slice(6) });
            }}>
            <option value="all">{t("All models")}</option>
            <option value="current" disabled={!inUse.length}>{inUse.length ? t("Model Codex is using ({model})", { model: inUse[0] }) : t("Model Codex is using")}</option>
            {grouped && groups.map(([g, ms]) => <option key={g} value={"group:" + g}>{t("Group {g} ({n})", { g, n: ms.length })}</option>)}
          </select>
          <span className="probe-cost">{t("Sends {n} small requests, about {tokens} tokens", { n: targets.length, tokens: targets.length * TOKENS_PER_PROBE })}</span>
          <div className="probe-acts">
            <button className="btn small ghost" disabled={running} onClick={() => setAsking(false)}>{t("Cancel")}</button>
            <button className="btn small primary" disabled={running || !targets.length} onClick={run}>
              {running ? t("Testing…") : t("Test {n} models", { n: targets.length })}
            </button>
          </div>
          {error && <div className="err probe-err">{error}</div>}
        </div>
      )}

      {error && !asking && <div className="banner err">{error}</div>}
      <table className={`models mt ${picked.length ? "picking" : ""}`}>
        <colgroup>
          <col className="c-pick" /><col /><col className="c-stat" /><col className="c-lat" /><col className="c-act" />
        </colgroup>
        <thead>
          <tr>
            <th className="pick" title={t("Tick models to test capabilities")} />
            <th>Model</th><th>{t("Status")}</th><th className="num">{t("Latency")}</th><th />
          </tr>
        </thead>
        {groups.map(([g, ms]) => {
          const out = ms.filter((m) => provider.probes?.[m]?.status === "quota").length;
          return (
            <tbody key={g}>
              {grouped && (
                <tr className="grp">
                  <th colSpan={5} scope="rowgroup">
                    <span className="grp-in">
                      <b>{g}</b><span className="muted">{t("{n} models", { n: ms.length })}</span>
                      {out > 0 && (
                        <span className="grp-chip" title={t("A model is out of quota; others in this group may run out soon too")}>
                          <i className="dot warn" />{t("{n}/{total} out of quota", { n: out, total: ms.length })}
                        </span>
                      )}
                    </span>
                  </th>
                </tr>
              )}
              {ms.map((m) => {
                const r = provider.probes?.[m];
                const on = picked.includes(m);
                const note = !r ? "" : r.status === "ok" ? (g === "combo" ? t("at least 1 upstream is working") : "")
                  : r.status !== "quota" && r.message ? shortReason(tr(r.message)) : "";
                return (
                  <tr key={m} className={on ? "picked" : undefined}>
                    <td className="pick">
                      <input type="checkbox" className="row-check" checked={on}
                        disabled={!on && picked.length >= MAX_CAP_MODELS}
                        onChange={() => togglePick(m)} aria-label={t("Select {name}", { name: m })} />
                    </td>
                    <td className="mname">
                      <span className="mid" title={m}>{m}</span>
                      {inUse.includes(m) && <b className="tag">Codex</b>}
                      {caps[m] && <CapPills caps={caps[m]} />}
                    </td>
                    <td>
                      {r ? (
                        <span className="pstat" title={r.message ? tr(r.message) : undefined}>
                          <span className="pstat-l"><i className={`dot ${DOT[r.status]}`} />{t(LABEL[r.status])}</span>
                          {note && <small>{note}</small>}
                        </span>
                      ) : <span className="muted">–</span>}
                    </td>
                    <td className="num muted">{r?.status === "ok" && r.latency_ms != null ? `${r.latency_ms} ms` : "–"}</td>
                    <td className="act">
                      <button
                        className={`btn small ghost try ${trying === m ? "busy" : ""}`}
                        disabled={running || (trying !== "" && trying !== m)}
                        title={t("Send one small request (~{n} tokens) to this model", { n: TOKENS_PER_PROBE })}
                        onClick={() => tryOne(m)}
                      >
                        {trying === m ? t("Trying…") : t("Try")}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          );
        })}
      </table>
      {Object.keys(caps).length > 0 && (
        <p className="muted cap-note">{t("Hover a result to see the reason or latency. Cache ✗ means the gateway breaks prompt caching, which costs many times more tokens.")}</p>
      )}
    </>
  );
}
