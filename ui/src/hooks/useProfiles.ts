import { useAtom, useAtomValue } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import { activeProfileAtom, apiSettingsAtom } from "../state/atoms";
import { setApiConfig } from "../lib/api";
import {
  removeProfile,
  upsertProfile,
  type ApiSettings,
  type ServerProfile,
} from "../lib/profiles";

/** Saved server profiles, and the one safe way to change them.
 *
 * Every mutation goes through `apply`, because pointing the client at a new
 * server is three steps that must happen together: persist the choice,
 * repoint the module-level fetch config, and drop the react-query cache.
 * Query keys carry no notion of which server answered them, so skipping the
 * last step would show one server's traces under another's name. */
export function useServerProfiles() {
  const [settings, setSettings] = useAtom(apiSettingsAtom);
  const active = useAtomValue(activeProfileAtom);
  const qc = useQueryClient();

  const apply = (next: ApiSettings) => {
    setSettings(next);
    const nextActive =
      next.profiles.find((p) => p.id === next.activeId) ?? next.profiles[0];
    if (nextActive) setApiConfig(nextActive);
    qc.clear();
  };

  return {
    settings,
    profiles: settings.profiles,
    active,
    /** Make `id` the live server. */
    switchTo: (id: string) => {
      if (id !== settings.activeId) apply({ ...settings, activeId: id });
    },
    /** Add or edit a profile. Editing the active one re-applies it. */
    save: (p: ServerProfile) => apply(upsertProfile(settings, p)),
    /** Add a profile and immediately switch to it. */
    saveAndSwitch: (p: ServerProfile) =>
      apply({ ...upsertProfile(settings, p), activeId: p.id }),
    remove: (id: string) => apply(removeProfile(settings, id)),
  };
}
