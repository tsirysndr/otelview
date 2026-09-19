import { useEffect, useState } from "react";
import { Command } from "cmdk";
import { useAtom, useSetAtom } from "jotai";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  IconAlignLeft,
  IconBolt,
  IconChartLine,
  IconKeyboard,
  IconLayoutSidebar,
  IconLayoutSidebarRight,
  IconMoonStars,
  IconRefresh,
  IconRoute,
  IconSearch,
  IconServer,
  IconSettings,
} from "@tabler/icons-react";
import {
  helpOpenAtom,
  inspectorOpenAtom,
  liveAtom,
  logFiltersAtom,
  lookbackAtom,
  openTraceIdAtom,
  paletteOpenAtom,
  railVisibleAtom,
  selectedMetricAtom,
  selectedSpanIdAtom,
  themeAtom,
  traceFiltersAtom,
  viewAtom,
} from "../state/atoms";
import { api } from "../lib/api";
import { plainTextField } from "../lib/inputProps";
import { fmtAgo, fmtDuration } from "../lib/format";
import { serviceColor } from "../lib/colors";

// Raycast-style global search ("/" or ⌘K): fuzzy commands + live search over
// services, operations, metrics and traces. cmdk filters the static entries;
// trace results come from the API as you type.
export function CommandPalette() {
  const [open, setOpen] = useAtom(paletteOpenAtom);
  const [theme, setTheme] = useAtom(themeAtom);
  const [search, setSearch] = useState("");
  const setView = useSetAtom(viewAtom);
  const setRail = useSetAtom(railVisibleAtom);
  const setInspector = useSetAtom(inspectorOpenAtom);
  const setHelp = useSetAtom(helpOpenAtom);
  const setLive = useSetAtom(liveAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);
  const [traceFilters, setTraceFilters] = useAtom(traceFiltersAtom);
  const [logFilters, setLogFilters] = useAtom(logFiltersAtom);
  const setSelectedMetric = useSetAtom(selectedMetricAtom);
  const setLookback = useSetAtom(lookbackAtom);
  const qc = useQueryClient();

  useEffect(() => {
    if (!open) setSearch("");
  }, [open]);

  const { data: services = [] } = useQuery({
    queryKey: ["services"],
    queryFn: api.services,
    enabled: open,
  });
  const { data: operations = [] } = useQuery({
    queryKey: ["operations", ""],
    queryFn: () => api.operations(""),
    enabled: open,
  });
  const { data: metrics = [] } = useQuery({
    queryKey: ["metrics"],
    queryFn: api.metrics,
    enabled: open,
  });
  // Live trace search against the API while typing.
  const q = search.trim();
  const { data: traceHits = [] } = useQuery({
    queryKey: ["palette-traces", q],
    queryFn: () => api.traces({ q, limit: 6, lookback: "all" }),
    enabled: open && q.length >= 2 && !/^[0-9a-f]{16,32}$/.test(q),
  });
  const looksLikeTraceId = /^[0-9a-f]{16,32}$/i.test(q);

  if (!open) return null;

  const run = (fn: () => void) => {
    setOpen(false);
    fn();
  };
  const openTrace = (id: string) =>
    run(() => {
      setView("traces");
      setSelectedSpan(null);
      setOpenTrace(id);
    });

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 p-4 pt-[15vh]"
      onClick={() => setOpen(false)}
    >
      <Command
        label="Search and commands"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-xl overflow-hidden rounded-large border border-content3 bg-content1 shadow-[0_0_30px_rgba(255,42,109,0.15)]"
      >
        <div className="flex items-center gap-2 border-b border-content3 px-4">
          <IconSearch size={16} className="shrink-0 text-default-400" />
          <Command.Input
            {...plainTextField}
            autoFocus
            value={search}
            onValueChange={setSearch}
            placeholder="Search traces, services, metrics — or type a command…"
            className="w-full bg-transparent py-3 text-sm text-foreground outline-none placeholder:text-default-400"
          />
        </div>
        <Command.List className="max-h-96 overflow-y-auto p-2">
          <Command.Empty className="px-3 py-6 text-center text-sm text-default-400">
            No results.
          </Command.Empty>

          {looksLikeTraceId && (
            <Command.Group heading="Trace id" className={GROUP}>
              <Command.Item
                value={`open-trace ${q}`}
                onSelect={() => openTrace(q.toLowerCase())}
                className={ITEM}
              >
                <span className="text-secondary">
                  <IconRoute size={16} />
                </span>
                open trace <span className="truncate text-default-400">{q}</span>
              </Command.Item>
            </Command.Group>
          )}

          {traceHits.length > 0 && (
            <Command.Group heading="Traces" className={GROUP}>
              {traceHits.map((t) => (
                <Command.Item
                  key={t.trace_id}
                  value={`trace ${t.root_name} ${t.trace_id}`}
                  onSelect={() => openTrace(t.trace_id)}
                  className={ITEM}
                >
                  <span
                    className="h-2.5 w-1 shrink-0 rounded-sm"
                    style={{ background: serviceColor(t.root_service) }}
                  />
                  <span className="min-w-0 flex-1 truncate">{t.root_name}</span>
                  <span className="shrink-0 text-[11px] text-neon-cyan">
                    {fmtDuration(t.duration_nanos)}
                  </span>
                  <span className="shrink-0 text-[11px] text-default-400">
                    {fmtAgo(t.start_time_unix_nano)}
                  </span>
                </Command.Item>
              ))}
            </Command.Group>
          )}

          {q.length >= 1 && (
            <Command.Group heading="Logs" className={GROUP}>
              <Command.Item
                value={`search-logs ${q}`}
                onSelect={() =>
                  run(() => {
                    setView("logs");
                    setLogFilters({ ...logFilters, search: q });
                  })
                }
                className={ITEM}
              >
                <span className="text-warning">
                  <IconAlignLeft size={16} />
                </span>
                search logs for “{q}”
              </Command.Item>
            </Command.Group>
          )}

          <Command.Group heading="Services" className={GROUP}>
            {services.map((s) => (
              <Command.Item
                key={s}
                value={`service ${s}`}
                onSelect={() =>
                  run(() => {
                    setView("traces");
                    setOpenTrace(null);
                    setTraceFilters({ ...traceFilters, service: s, operation: "" });
                  })
                }
                className={ITEM}
              >
                <span
                  className="inline-block h-2 w-2 shrink-0 rounded-full"
                  style={{ background: serviceColor(s) }}
                />
                {s}
                <span className="ml-auto text-[11px] text-default-400">traces</span>
              </Command.Item>
            ))}
          </Command.Group>

          <Command.Group heading="Operations" className={GROUP}>
            {operations.slice(0, 12).map((o) => (
              <Command.Item
                key={o}
                value={`operation ${o}`}
                onSelect={() =>
                  run(() => {
                    setView("traces");
                    setOpenTrace(null);
                    setTraceFilters({ ...traceFilters, operation: o });
                  })
                }
                className={ITEM}
              >
                <span className="text-secondary">
                  <IconBolt size={16} />
                </span>
                <span className="truncate">{o}</span>
              </Command.Item>
            ))}
          </Command.Group>

          <Command.Group heading="Metrics" className={GROUP}>
            {metrics.map((m) => (
              <Command.Item
                key={m.name}
                value={`metric ${m.name}`}
                onSelect={() =>
                  run(() => {
                    setView("metrics");
                    setSelectedMetric(m.name);
                  })
                }
                className={ITEM}
              >
                <span className="text-primary">
                  <IconChartLine size={16} />
                </span>
                <span className="truncate">{m.name}</span>
                <span className="ml-auto text-[11px] text-default-400">
                  {m.metric_type}
                </span>
              </Command.Item>
            ))}
          </Command.Group>

          <Command.Group heading="Go to" className={GROUP}>
            <Item icon={<IconRoute size={16} />} onSelect={() => run(() => setView("traces"))}>
              Traces
            </Item>
            <Item icon={<IconAlignLeft size={16} />} onSelect={() => run(() => setView("logs"))}>
              Logs
            </Item>
            <Item icon={<IconChartLine size={16} />} onSelect={() => run(() => setView("metrics"))}>
              Metrics
            </Item>
            <Item icon={<IconSettings size={16} />} onSelect={() => run(() => setView("settings"))}>
              Settings
            </Item>
          </Command.Group>

          <Command.Group heading="Commands" className={GROUP}>
            <Item
              icon={<IconLayoutSidebar size={16} />}
              onSelect={() => run(() => setRail((v) => !v))}
            >
              Toggle left rail
            </Item>
            <Item
              icon={<IconLayoutSidebarRight size={16} />}
              onSelect={() => run(() => setInspector((v) => !v))}
            >
              Toggle inspector panel
            </Item>
            <Item icon={<IconRefresh size={16} />} onSelect={() => run(() => qc.invalidateQueries())}>
              Refresh
            </Item>
            <Item
              icon={<IconBolt size={16} />}
              onSelect={() => run(() => setLive((v) => !v))}
            >
              Toggle live refresh
            </Item>
            <Item
              icon={<IconMoonStars size={16} />}
              onSelect={() => run(() => setTheme(theme === "dark" ? "light" : "dark"))}
            >
              Toggle theme
            </Item>
            <Item
              icon={<IconServer size={16} />}
              onSelect={() => run(() => setLookback("all"))}
            >
              Lookback: all time
            </Item>
            <Item icon={<IconKeyboard size={16} />} onSelect={() => run(() => setHelp(true))}>
              Keyboard shortcuts
            </Item>
          </Command.Group>
        </Command.List>
      </Command>
    </div>
  );
}

const GROUP = "px-1 text-[11px] uppercase tracking-wide text-default-400";
const ITEM =
  "flex items-center gap-2 rounded-md px-3 py-2 text-sm text-default-600";

function Item({
  icon,
  children,
  onSelect,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
  onSelect: () => void;
}) {
  return (
    <Command.Item onSelect={onSelect} className={ITEM}>
      <span className="text-primary">{icon}</span>
      {children}
    </Command.Item>
  );
}
