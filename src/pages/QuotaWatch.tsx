import { useEffect, useState } from "react";
import { api, errText, isBilling, onQuotaWatchChanged, Probe, Provider, QuotaWatch, watchGroup } from "../api";
import { Icon } from "../icons";
import { locale, t } from "../i18n";

/** Live list of quota watches (reloads on "quota-watch-changed"). */
export function useQuotaWatches(onChange?: () => void): [QuotaWatch[], (l: QuotaWatch[]) => void] {
  const [list, setList] = useState<QuotaWatch[]>([]);
  useEffect(() => {
    let alive = true;
    const load = () => api.quotaWatchList().then((l) => { if (alive) setList(l); }).catch(() => {});
    load();
    const off = onQuotaWatchChanged(() => { load(); onChange?.(); });
    return () => { alive = false; off(); };
  }, []);
  return [list, setList];
}

export const hhmm = (iso: string) => new Date(iso).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });

/**
 * Action on an out-of-quota card: "Notify when it's back", or "Watching · next
 * check HH:MM" + Cancel when the group is watched. Always available (clicking
 * is consent); disabled for out-of-credit errors, where waiting won't help.
 * `onChange` runs when any watch changes (a check may have stored a new result).
 */
export function QuotaWatchAction({ provider, model, probe, notify, onChange }: { provider: Provider; model: string; probe: Probe; notify: (m: string) => void; onChange?: () => void }) {
  const [list, setList] = useQuotaWatches(onChange);
  const [busy, setBusy] = useState(false);
  const group = watchGroup(provider, model);
  const watch = list.find((w) => w.provider_id === provider.id && w.group === group);
  const billing = isBilling(probe.message);

  async function run(f: () => Promise<QuotaWatch[]>) {
    setBusy(true);
    try { setList(await f()); } catch (e) { notify(errText(e)); } finally { setBusy(false); }
  }

  if (watch) {
    return (
      <div className="qw-act">
        <span className="qw-on"><Icon name="clock" />{t("Watching · next check {time}", { time: hhmm(watch.next_at) })}</span>
        <button className="btn small ghost" disabled={busy} onClick={() => run(() => api.quotaWatchRemove(provider.id, group))}>{t("Cancel")}</button>
      </div>
    );
  }
  if (billing) {
    return (
      <div className="qw-act">
        <button className="btn small" disabled title={t("Out of credit: top up, waiting won't help")}>{t("Notify when it's back")}</button>
        <small className="muted">{t("Out of credit: top up, waiting won't help")}</small>
      </div>
    );
  }
  return (
    <div className="qw-act">
      <button className="btn small" disabled={busy} onClick={() => run(() => api.quotaWatchAdd(provider.id, model))}>
        <Icon name="bell" />{t("Notify when it's back")}
      </button>
    </div>
  );
}
