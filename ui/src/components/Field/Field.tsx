/** Filter-bar field: a small fixed label ABOVE the control (no HeroUI
 * floating-label behavior, so label and placeholder can never collide). */
export function Field({
  label,
  className,
  children,
}: {
  label: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={`flex flex-col gap-0.5 ${className ?? ""}`}>
      <span className="px-0.5 text-[10px] uppercase tracking-wide text-default-500">
        {label}
      </span>
      {children}
    </div>
  );
}
