/**
 * Loading placeholders shaped like the content they stand in for.
 *
 * Built from the HeroUI semantic tokens (`default-*`, `divider`) rather than
 * fixed colours, so light and dark are both correct without this file knowing
 * which one is active — the theme switches a class on `<html>` and these
 * follow. That is also why there is no SVG placeholder library here: those
 * take literal `backgroundColor`/`foregroundColor` props, which would mean
 * restating the palette in JS and keeping it in step with the Tailwind config
 * by hand.
 *
 * Shown on first load only. React Query's `isLoading` is already exactly that
 * — pending with nothing cached — so a background refetch keeps the current
 * content on screen instead of flashing back to bars.
 */

/** One shimmering block. `className` sets its size and shape. */
export function Skeleton({ className = "" }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={`relative overflow-hidden rounded bg-default-200/50 ${className}`}
    >
      <div className="absolute inset-0 -translate-x-full animate-shimmer bg-gradient-to-r from-transparent via-default-100/60 to-transparent motion-reduce:animate-none" />
    </div>
  );
}

/**
 * Wraps a group of placeholders. Announces one polite "loading" to assistive
 * tech rather than letting every bar describe itself.
 */
function SkeletonGroup({
  label,
  className = "",
  children,
}: {
  label: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div role="status" aria-busy="true" aria-label={label} className={className}>
      {children}
    </div>
  );
}

/** Log and trace lists: a timestamp, a level, a service chip and a message. */
export function SkeletonRows({ rows = 12, label = "loading" }: { rows?: number; label?: string }) {
  return (
    <SkeletonGroup label={label}>
      {Array.from({ length: rows }, (_, i) => (
        <div
          key={i}
          className="flex items-center gap-2 border-b border-divider/40 px-3 py-1.5"
        >
          <Skeleton className="h-3 w-20 shrink-0" />
          <Skeleton className="h-3 w-10 shrink-0" />
          <Skeleton className="h-3 w-24 shrink-0" />
          {/* Varying widths so it reads as text rather than a table. */}
          <Skeleton
            className="h-3 flex-1"
            {...{ style: { maxWidth: `${55 + ((i * 37) % 40)}%` } }}
          />
        </div>
      ))}
    </SkeletonGroup>
  );
}

/** A sidebar list of names, as on the metrics view. */
export function SkeletonList({ rows = 10, label = "loading" }: { rows?: number; label?: string }) {
  return (
    <SkeletonGroup label={label}>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="border-b border-divider/40 px-3 py-2">
          <Skeleton
            className="h-3"
            {...{ style: { width: `${50 + ((i * 29) % 45)}%` } }}
          />
        </div>
      ))}
    </SkeletonGroup>
  );
}

/** A plot area with axis ticks, for the metric explorer. */
export function SkeletonChart({ label = "loading" }: { label?: string }) {
  return (
    <SkeletonGroup label={label} className="flex h-full flex-col gap-3 p-4">
      <Skeleton className="h-4 w-48" />
      <Skeleton className="min-h-[8rem] flex-1 rounded-md" />
      <div className="flex gap-3">
        {Array.from({ length: 4 }, (_, i) => (
          <Skeleton key={i} className="h-3 w-16" />
        ))}
      </div>
    </SkeletonGroup>
  );
}

/** The stacked severity bars above the log list. */
export function SkeletonHistogram({ bars = 60 }: { bars?: number }) {
  return (
    <SkeletonGroup
      label="loading the log histogram"
      className="flex h-16 items-end gap-[2px] px-1 py-2"
    >
      {Array.from({ length: bars }, (_, i) => (
        <Skeleton
          key={i}
          className="w-full rounded-sm"
          // A fixed pattern rather than random, so the bars do not reshuffle
          // on every render while the data is still on its way.
          {...{ style: { height: `${25 + ((i * 17) % 70)}%` } }}
        />
      ))}
    </SkeletonGroup>
  );
}

/** A table body: a wide first column of names, then narrow figures. */
export function SkeletonTable({
  rows = 8,
  cols = 5,
  label = "loading",
}: {
  rows?: number;
  cols?: number;
  label?: string;
}) {
  return (
    <SkeletonGroup label={label}>
      {Array.from({ length: rows }, (_, r) => (
        <div key={r} className="flex items-center gap-2 border-b border-divider/40 py-2">
          <Skeleton
            className="h-3"
            {...{ style: { width: `${28 + ((r * 13) % 20)}%` } }}
          />
          <div className="ml-auto flex gap-2">
            {Array.from({ length: cols }, (_, c) => (
              <Skeleton key={c} className="h-3 w-12" />
            ))}
          </div>
        </div>
      ))}
    </SkeletonGroup>
  );
}

/** Service cards: a title, a couple of figures and a sparkline. */
export function SkeletonCards({ cards = 6 }: { cards?: number }) {
  return (
    <SkeletonGroup
      label="loading services"
      className="grid gap-3 p-3 [grid-template-columns:repeat(auto-fill,minmax(15rem,1fr))]"
    >
      {Array.from({ length: cards }, (_, i) => (
        <div
          key={i}
          className="flex flex-col gap-3 rounded-md border border-divider bg-content1 p-3"
        >
          <Skeleton className="h-4 w-32" />
          <div className="flex gap-4">
            <Skeleton className="h-3 w-14" />
            <Skeleton className="h-3 w-14" />
            <Skeleton className="h-3 w-14" />
          </div>
          <Skeleton className="h-10 w-full rounded" />
        </div>
      ))}
    </SkeletonGroup>
  );
}

/** A trace waterfall: nested bars of decreasing width. */
export function SkeletonWaterfall({ rows = 10 }: { rows?: number }) {
  return (
    <SkeletonGroup label="loading the trace" className="flex flex-col gap-2 p-4">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-2">
          <Skeleton className="h-3 w-40 shrink-0" />
          <div className="flex-1">
            <Skeleton
              className="h-3"
              // Indented and shortened with depth, the shape a waterfall takes.
              {...{
                style: {
                  marginLeft: `${(i % 4) * 8}%`,
                  width: `${70 - (i % 4) * 12}%`,
                },
              }}
            />
          </div>
        </div>
      ))}
    </SkeletonGroup>
  );
}
