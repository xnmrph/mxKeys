/** Line icons on a 24px grid, round joins (MX Nexus iconography). */
const PATHS: Record<string, string> = {
  keyboard: "M3 7.5A1.5 1.5 0 0 1 4.5 6h15A1.5 1.5 0 0 1 21 7.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 16.5zM7 10h.01M11 10h.01M15 10h.01M17 13h.01M7 13h.01M9 15h6",
  keymap: "M4 5h4v4H4zM10 5h4v4h-4zM16 5h4v4h-4zM4 11h4v4H4zM10 11h4v4h-4zM16 11h4v4h-4zM7 17h10v2H7z",
  screen: "M7 3h10a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM6 7h12M9.5 14l2-2.5 2 2.5 1-1.2 1.5 2.2h-8z",
  settings: "M4 7h9M17 7h3M4 17h3M11 17h9M15 5v4M9 15v4",
  install: "M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14",
  lock: "M7 11V8a5 5 0 0 1 10 0v3M6 11h12v9H6zM12 15v2",
  folder: "M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z",
  warn: "M12 4 3 19h18zM12 10v4M12 16.5h.01",
  usb: "M12 3v14M9 6l3-3 3 3M8 11v2l4 2M16 9v3l-4 2M12 17a2 2 0 1 0 0 4 2 2 0 0 0 0-4z",
  bluetooth: "M7 7l10 10-5 4V3l5 4L7 17",
  branch: "M7 4v16M7 8a4 4 0 0 0 4 4h2a4 4 0 0 1 4 4M17 4v12",
};

export function Icon({ name, size = 22, className }: { name: string; size?: number; className?: string }) {
  return (
    <svg className={`icon${className ? ` ${className}` : ""}`} width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d={PATHS[name] ?? ""} />
    </svg>
  );
}
