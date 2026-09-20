export function Logo() {
  return (
    <div className="flex select-none items-center gap-2">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden>
        {/* stylized telescope-lens rings in neon cyan/magenta */}
        <circle cx="12" cy="12" r="9" stroke="#05D9E8" strokeWidth="2" />
        <circle cx="12" cy="12" r="4.5" stroke="#FF2A6D" strokeWidth="2" />
        <circle cx="12" cy="12" r="1.2" fill="#F5F5FF" />
      </svg>
      <span className="text-sm font-semibold tracking-widest">
        otel<span className="text-neon-magenta">view</span>
      </span>
    </div>
  );
}
